import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:geolocator/geolocator.dart';
import 'package:geolocator_platform_interface/geolocator_platform_interface.dart';
import 'package:permission_handler_platform_interface/permission_handler_platform_interface.dart';
import 'package:soshal_flutter/services/permissions_service.dart';

class _MockPermissionHandler extends PermissionHandlerPlatform {
  final PermissionStatus status;

  _MockPermissionHandler(this.status);

  @override
  Future<Map<Permission, PermissionStatus>> requestPermissions(
      List<Permission> permissions) async {
    return {for (final p in permissions) p: status};
  }

  @override
  Future<PermissionStatus> checkPermissionStatus(Permission permission) async {
    return status;
  }
}

class _MockGeolocator extends GeolocatorPlatform {
  final LocationPermission permission;
  final bool serviceEnabled;
  final Position position;

  _MockGeolocator({
    required this.permission,
    required this.serviceEnabled,
    required this.position,
  });

  @override
  Future<LocationPermission> checkPermission() async => permission;

  @override
  Future<LocationPermission> requestPermission() async => permission;

  @override
  Future<bool> isLocationServiceEnabled() async => serviceEnabled;

  @override
  Future<Position> getCurrentPosition({
    LocationSettings? locationSettings,
  }) async =>
      position;
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() {
    PermissionsService.debugPlatformIsAndroid = null;
    PermissionsService.debugPlatformIsLinux = null;
  });

  tearDown(() {
    PermissionsService.debugPlatformIsAndroid = null;
    PermissionsService.debugPlatformIsLinux = null;
  });

  group('Android', () {
    setUp(() {
      PermissionsService.debugPlatformIsAndroid = true;
      PermissionsService.debugPlatformIsLinux = false;
    });

    test('ensureCameraMic granted when both permissions granted', () async {
      PermissionHandlerPlatform.instance = _MockPermissionHandler(
          PermissionStatus.granted);
      final result = await PermissionsService.ensureCameraMic();
      expect(result.granted, isTrue);
    });

    test('ensureCameraMic denied with reason when mic denied', () async {
      final handler = _MockPermissionHandler(PermissionStatus.granted);
      var micRequests = 0;
      PermissionHandlerPlatform.instance =
          _RequestTrackingPermissionHandler(
              status: PermissionStatus.denied,
              onMic: () => micRequests++,
              delegate: handler);
      final result = await PermissionsService.ensureCameraMic();
      expect(result.granted, isFalse);
      expect(result.reason, contains('microphone'));
      expect(micRequests, 1);
    });

    test('currentPosition returns coords when permission + service ok',
        () async {
      PermissionHandlerPlatform.instance =
          _MockPermissionHandler(PermissionStatus.granted);
      GeolocatorPlatform.instance = _MockGeolocator(
        permission: LocationPermission.whileInUse,
        serviceEnabled: true,
        position: Position(
          latitude: 52.52,
          longitude: 13.40,
          timestamp: DateTime.fromMillisecondsSinceEpoch(0),
          accuracy: 10.0,
          altitude: 0.0,
          altitudeAccuracy: 0.0,
          heading: 0.0,
          headingAccuracy: 0.0,
          speed: 0.0,
          speedAccuracy: 0.0,
        ),
      );
      final location = await PermissionsService.currentPosition();
      expect(location.ok, isTrue);
      expect(location.latitude, closeTo(52.52, 1e-9));
      expect(location.longitude, closeTo(13.40, 1e-9));
    });

    test('currentPosition fails when permission denied forever', () async {
      PermissionHandlerPlatform.instance =
          _MockPermissionHandler(PermissionStatus.permanentlyDenied);
      GeolocatorPlatform.instance = _MockGeolocator(
        permission: LocationPermission.deniedForever,
        serviceEnabled: true,
        position: Position(
          latitude: 0,
          longitude: 0,
          timestamp: DateTime.fromMillisecondsSinceEpoch(0),
          accuracy: 0,
          altitude: 0,
          altitudeAccuracy: 0,
          heading: 0,
          headingAccuracy: 0,
          speed: 0,
          speedAccuracy: 0,
        ),
      );
      final location = await PermissionsService.currentPosition();
      expect(location.ok, isFalse);
      expect(location.error, contains('permanently denied'));
    });

    test('currentPosition fails when location service off', () async {
      PermissionHandlerPlatform.instance =
          _MockPermissionHandler(PermissionStatus.granted);
      GeolocatorPlatform.instance = _MockGeolocator(
        permission: LocationPermission.whileInUse,
        serviceEnabled: false,
        position: Position(
          latitude: 0,
          longitude: 0,
          timestamp: DateTime.fromMillisecondsSinceEpoch(0),
          accuracy: 0,
          altitude: 0,
          altitudeAccuracy: 0,
          heading: 0,
          headingAccuracy: 0,
          speed: 0,
          speedAccuracy: 0,
        ),
      );
      final location = await PermissionsService.currentPosition();
      expect(location.ok, isFalse);
      expect(location.error, contains('Location service is off'));
    });
  });

  group('Linux', () {
    setUp(() {
      PermissionsService.debugPlatformIsAndroid = false;
      PermissionsService.debugPlatformIsLinux = true;
    });

    test('camera/mic honest unsupported', () async {
      final result = await PermissionsService.ensureCameraMic();
      expect(result.granted, isFalse);
      expect(result.reason, contains('Linux'));
    });

    test('currentPosition reads portal channel coords', () async {
      const channel = MethodChannel('com.soshal/portal');
      final binding = TestDefaultBinaryMessengerBinding.instance;
      binding.defaultBinaryMessenger.setMockMethodCallHandler(
        channel,
        (call) async {
          expect(call.method, 'requestLocation');
          return <String, double>{'latitude': 51.5, 'longitude': -0.12};
        },
      );
      addTearDown(() => binding.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, null));
      final location = await PermissionsService.currentPosition();
      expect(location.ok, isTrue);
      expect(location.latitude, closeTo(51.5, 1e-9));
      expect(location.longitude, closeTo(-0.12, 1e-9));
    });

    test('currentPosition surfaces portal denial', () async {
      const channel = MethodChannel('com.soshal/portal');
      final binding = TestDefaultBinaryMessengerBinding.instance;
      binding.defaultBinaryMessenger.setMockMethodCallHandler(
        channel,
        (call) async => throw PlatformException(
          code: 'DENIED',
          message: 'Location permission denied',
        ),
      );
      addTearDown(() => binding.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, null));
      final location = await PermissionsService.currentPosition();
      expect(location.ok, isFalse);
      expect(location.error, contains('Location denied'));
    });
  });

  test('other platforms honest unsupported', () async {
    PermissionsService.debugPlatformIsAndroid = false;
    PermissionsService.debugPlatformIsLinux = false;
    final location = await PermissionsService.currentPosition();
    expect(location.ok, isFalse);
    expect(location.error, contains('unavailable on this platform'));
  });
}

class _RequestTrackingPermissionHandler extends PermissionHandlerPlatform {
  final PermissionStatus status;
  final void Function() onMic;
  final _MockPermissionHandler delegate;

  _RequestTrackingPermissionHandler({
    required this.status,
    required this.onMic,
    required this.delegate,
  });

  @override
  Future<Map<Permission, PermissionStatus>> requestPermissions(
      List<Permission> permissions) async {
    if (permissions.contains(Permission.microphone)) onMic();
    return {
      for (final p in permissions)
        p: p == Permission.microphone ? status : PermissionStatus.granted,
    };
  }

  @override
  Future<PermissionStatus> checkPermissionStatus(Permission permission) async {
    return delegate.checkPermissionStatus(permission);
  }
}
