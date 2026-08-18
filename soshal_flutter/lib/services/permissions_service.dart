import 'dart:io' show Platform;

import 'package:flutter/foundation.dart' show visibleForTesting;
import 'package:flutter/services.dart';
import 'package:geolocator/geolocator.dart';
import 'package:permission_handler/permission_handler.dart' as ph;

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
/// Android: `permission_handler` for camera/mic, `geolocator` for location
/// (its own permission flow + GPS fix). Linux desktop: XDG Desktop Portal
/// location via the native `com.soshal/portal` channel (`my_application.cc`);
/// camera/mic have no Linux capture path (no PipeWire/v4l2 render stack in
/// the Flutter embedder) and report honest failure. Every call is a safe
/// no-op on other platforms.
class PermissionsService {
  PermissionsService._();

  static const MethodChannel _portalChannel =
      MethodChannel('com.soshal/portal');

  static const String _linuxCameraMicUnsupported =
      'Camera and microphone are not supported on Linux desktop '
      '(no camera capture path in the Linux build).';

  /// Test hooks: override platform detection without touching the real OS.
  @visibleForTesting
  static bool? debugPlatformIsAndroid;

  @visibleForTesting
  static bool? debugPlatformIsLinux;

  static bool get _isAndroid => debugPlatformIsAndroid ?? Platform.isAndroid;

  static bool get _isLinux => debugPlatformIsLinux ?? Platform.isLinux;

  /// Request camera + microphone together (grouped dialog on Android 12+).
  static Future<PermissionResult> ensureCameraMic() async {
    if (!_isAndroid) {
      return const PermissionResult.denied(_linuxCameraMicUnsupported);
    }
    final camera = await _request(ph.Permission.camera);
    if (!camera.granted) return camera;
    return _request(ph.Permission.microphone);
  }

  static Future<PermissionResult> ensureCamera() async {
    if (!_isAndroid) {
      return const PermissionResult.denied(_linuxCameraMicUnsupported);
    }
    return _request(ph.Permission.camera);
  }

  static Future<PermissionResult> ensureMic() async {
    if (!_isAndroid) {
      return const PermissionResult.denied(_linuxCameraMicUnsupported);
    }
    return _request(ph.Permission.microphone);
  }

  static Future<PermissionResult> _request(ph.Permission permission) async {
    try {
      final status = await permission.request();
      if (status.isGranted) return const PermissionResult.granted();
      if (status.isPermanentlyDenied) {
        return PermissionResult.denied(
            'Permission permanently denied - enable it in app settings');
      }
      return PermissionResult.denied(
          '${permission.toString().split('.').last} permission denied');
    } catch (e) {
      return PermissionResult.denied('Permission request failed: $e');
    }
  }

  /// True when a camera/mic permission is permanently denied (Android).
  static Future<bool> isPermanentlyDenied() async {
    if (!_isAndroid) return false;
    try {
      return await ph.Permission.camera.isPermanentlyDenied ||
          await ph.Permission.microphone.isPermanentlyDenied;
    } catch (_) {
      return false;
    }
  }

  /// Open the OS app-settings page (Android). Returns false elsewhere.
  static Future<bool> openSettings() async {
    if (!_isAndroid) return false;
    try {
      await ph.openAppSettings();
      return true;
    } catch (_) {
      return false;
    }
  }

  /// Request location access (geolocator dialog on Android; portal grants
  /// on demand on Linux, so no static prompt here).
  static Future<PermissionResult> ensureLocation() async {
    if (!_isAndroid && !_isLinux) {
      return const PermissionResult.denied(
          'Location unavailable on this platform');
    }
    if (_isLinux) return const PermissionResult.granted();
    try {
      final status = await Geolocator.checkPermission();
      if (status == LocationPermission.whileInUse ||
          status == LocationPermission.always) {
        return const PermissionResult.granted();
      }
      if (status == LocationPermission.deniedForever) {
        return const PermissionResult.denied(
            'Location permission permanently denied - enable it in app settings');
      }
      final requested = await Geolocator.requestPermission();
      if (requested == LocationPermission.whileInUse ||
          requested == LocationPermission.always) {
        return const PermissionResult.granted();
      }
      return const PermissionResult.denied('Location permission denied');
    } catch (e) {
      return PermissionResult.denied('Location permission failed: $e');
    }
  }

  /// Current device position, or a failure reason.
  ///
  /// Android: geolocator GPS fix (location service must be enabled).
  /// Linux: XDG Desktop Portal location via `com.soshal/portal`.
  static Future<LocationResult> currentPosition() async {
    if (_isAndroid) {
      final granted = await ensureLocation();
      if (!granted.granted) return LocationResult.failed(granted.reason);
      try {
        if (!await Geolocator.isLocationServiceEnabled()) {
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
    if (_isLinux) {
      try {
        final map = await _portalChannel.invokeMapMethod<String, double>(
          'requestLocation',
        );
        if (map == null) {
          return const LocationResult.failed('Location portal unavailable');
        }
        final lat = map['latitude'];
        final lng = map['longitude'];
        if (lat == null || lng == null) {
          return const LocationResult.failed('Location portal returned no fix');
        }
        return LocationResult.coords(lat, lng);
      } on PlatformException catch (e) {
        return LocationResult.failed(
            '${e.code == 'DENIED' ? 'Location denied' : 'Location unavailable'}: '
            '${e.message ?? e.code}');
      } catch (e) {
        return LocationResult.failed('Location portal failed: $e');
      }
    }
    return const LocationResult.failed('Location unavailable on this platform');
  }
}
