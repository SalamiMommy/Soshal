import 'package:flutter/foundation.dart' show visibleForTesting;
import 'package:geolocator/geolocator.dart';

// ignore_for_file: invalid_use_of_internal_member
import '../ffi/permissions.dart' as ffi;
import '../frb_generated.dart';

/// Outcome of a permission request.
class PermissionResult {
  final bool granted;
  final String reason;

  const PermissionResult.granted()
      : granted = true,
        reason = '';

  const PermissionResult.denied(this.reason) : granted = false;
}

/// A location fix: coordinates or an honest failure reason.
class LocationResult {
  final double? latitude;
  final double? longitude;
  final String? error;

  const LocationResult.coords(this.latitude, this.longitude) : error = null;

  const LocationResult.failed(this.error)
      : latitude = null,
        longitude = null;

  bool get ok => latitude != null && longitude != null;
}

/// Runtime permission + device-location access.
///
/// Android: Rust JNI (`ffi/permissions.dart`) for camera/mic + location
/// permissions; `geolocator` remains ONLY for the GPS fix (its own
/// permission flow is bypassed in favor of the Rust surface). Linux
/// desktop: XDG Desktop Portal location via ashpd (Rust); camera/mic have
/// no Linux capture path (no PipeWire/v4l2 render stack in the Flutter
/// embedder) and report honest failure. Every call is a safe no-op on
/// other platforms.
class PermissionsService {
  PermissionsService._();

  static const String _linuxCameraMicUnsupported =
      'Camera and microphone are not supported on Linux desktop '
      '(no camera capture path in the Linux build).';

  static const String _androidSettingsHint = ' - enable it in app settings';

  /// Test hooks: override platform detection without touching the real OS.
  @visibleForTesting
  static bool? debugPlatformIsAndroid;

  @visibleForTesting
  static bool? debugPlatformIsLinux;

  /// Test hook: shrink the permission-result poll interval.
  @visibleForTesting
  static Duration debugPollDelay = const Duration(milliseconds: 250);

  static String? _hostPlatform;

  static String get _platform =>
      _hostPlatform ??= ffi.permissionsPlatformCurrent();

  static bool get isAndroid => debugPlatformIsAndroid ?? _platform == 'android';

  static bool get isLinux => debugPlatformIsLinux ?? _platform == 'linux';

  /// Request camera + microphone together (grouped dialog on Android 12+).
  static Future<PermissionResult> ensureCameraMic() async {
    if (!isAndroid) {
      return const PermissionResult.denied(_linuxCameraMicUnsupported);
    }
    return _ensureCameraMic();
  }

  static Future<PermissionResult> ensureCamera() async {
    if (!isAndroid) {
      return const PermissionResult.denied(_linuxCameraMicUnsupported);
    }
    return _ensureCameraMic();
  }

  static Future<PermissionResult> ensureMic() async {
    if (!isAndroid) {
      return const PermissionResult.denied(_linuxCameraMicUnsupported);
    }
    return _ensureCameraMic();
  }

  static Future<PermissionResult> _ensureCameraMic() async {
    try {
      if (ffi.permissionsCameraMicGranted()) {
        return const PermissionResult.granted();
      }
      if (!ffi.permissionsCameraMicRequest()) {
        return const PermissionResult.denied('Permission request unavailable');
      }
      for (var i = 0; i < 20; i++) {
        await Future<void>.delayed(debugPollDelay);
        if (ffi.permissionsCameraMicGranted()) {
          return const PermissionResult.granted();
        }
      }
      if (ffi.permissionsCameraMicPermanentlyDenied()) {
        return PermissionResult.denied(
            'Permission permanently denied$_androidSettingsHint');
      }
      return const PermissionResult.denied(
          'camera/microphone permission denied');
    } catch (e) {
      return PermissionResult.denied('Permission request failed: $e');
    }
  }

  /// True when a camera/mic permission is permanently denied (Android).
  static Future<bool> isPermanentlyDenied() async {
    if (!isAndroid) return false;
    try {
      return ffi.permissionsCameraMicPermanentlyDenied();
    } catch (_) {
      return false;
    }
  }

  /// Open the OS app-settings page (Android). Returns false elsewhere.
  static Future<bool> openSettings() async {
    if (!isAndroid) return false;
    try {
      return ffi.permissionsOpenSettings();
    } catch (_) {
      return false;
    }
  }

  /// Request POST_NOTIFICATIONS (Android 13+; the daemon foreground
  /// service notification needs it to be visible — the service itself runs
  /// regardless). No-op granted on older platforms.
  static Future<PermissionResult> ensureNotifications() async {
    if (!isAndroid) return const PermissionResult.granted();
    try {
      if (ffi.permissionsNotificationsGranted()) {
        return const PermissionResult.granted();
      }
      if (!ffi.permissionsNotificationsRequest()) {
        return const PermissionResult.denied(
            'Notification permission request unavailable');
      }
      for (var i = 0; i < 20; i++) {
        await Future<void>.delayed(debugPollDelay);
        if (ffi.permissionsNotificationsGranted()) {
          return const PermissionResult.granted();
        }
      }
      if (ffi.permissionsNotificationsPermanentlyDenied()) {
        return PermissionResult.denied(
            'Notification permission permanently denied$_androidSettingsHint');
      }
      return const PermissionResult.denied('Notification permission denied');
    } catch (e) {
      return PermissionResult.denied('Notification permission failed: $e');
    }
  }

  /// Request location access (Rust dialog on Android; portal grants on
  /// demand on Linux, so no static prompt here).
  static Future<PermissionResult> ensureLocation() async {
    if (!isAndroid && !isLinux) {
      return const PermissionResult.denied(
          'Location unavailable on this platform');
    }
    if (isLinux) return const PermissionResult.granted();
    try {
      if (ffi.permissionsLocationGranted()) {
        return const PermissionResult.granted();
      }
      if (!ffi.permissionsLocationRequest()) {
        return const PermissionResult.denied('Location permission failed');
      }
      for (var i = 0; i < 20; i++) {
        await Future<void>.delayed(debugPollDelay);
        if (ffi.permissionsLocationGranted()) {
          return const PermissionResult.granted();
        }
      }
      return const PermissionResult.denied(
          'Location permission denied$_androidSettingsHint');
    } catch (e) {
      return PermissionResult.denied('Location permission failed: $e');
    }
  }

  /// Current device position, or a failure reason.
  ///
  /// Android: geolocator GPS fix (location service must be enabled; the
  /// permission flow itself is Rust). Linux: XDG Desktop Portal location
  /// via ashpd.
  static Future<LocationResult> currentPosition() async {
    if (isAndroid) {
      final granted = await ensureLocation();
      if (!granted.granted) return LocationResult.failed(granted.reason);
      try {
        if (!ffi.permissionsLocationEnabled()) {
          return const LocationResult.failed('Location service is off');
        }
        final pos = await Geolocator.getCurrentPosition(
          locationSettings: const LocationSettings(
            accuracy: LocationAccuracy.medium,
            timeLimit: Duration(seconds: 30),
          ),
        );
        return LocationResult.coords(pos.latitude, pos.longitude);
      } catch (e) {
        return LocationResult.failed('GPS fix failed: $e');
      }
    }
    if (isLinux) {
      // Desktop location switch off → the XDG portal rejects with
      // "NotAllowed: Location services disabled" before any permission
      // dialog can appear. Pre-check for an actionable message.
      if (!ffi.permissionsLocationEnabled()) {
        return const LocationResult.failed(
            'Location services are off — enable Location Services in '
            'system settings (e.g. Settings \u2192 Privacy \u2192 Location '
            'Services), then retry.');
      }
      try {
        final fix = await ffi.permissionsLocationPortalFix();
        if (fix == null) {
          return const LocationResult.failed('Location portal unavailable');
        }
        return LocationResult.coords(fix.latitude, fix.longitude);
      } catch (e) {
        final msg = e.toString();
        return LocationResult.failed(
            '${msg.contains('DENIED') ? 'Location denied' : 'Location unavailable'}: '
            '$msg');
      }
    }
    return const LocationResult.failed('Location unavailable on this platform');
  }

  /// Approximate coordinates from the egress IP (city-level accuracy).
  ///
  /// Fallback when the OS location service is off — typical desktop Linux,
  /// where the XDG portal rejects before any dialog can appear. Callers
  /// MUST show a consent dialog first: the query discloses the user's
  /// public IP to a third-party provider (ipwho.is).
  static Future<LocationResult> ipLocation() async {
    try {
      final fix = await RustLib.instance.api.crateFfiGeolocGeolocIpLookup();
      return LocationResult.coords(fix.latitude, fix.longitude);
    } catch (e) {
      return LocationResult.failed(
          'IP location unavailable: $e\n\nTip: enter coordinates manually — '
          'geohash encoding needs no location service.');
    }
  }
}
