// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:geolocator/geolocator.dart';
import 'package:geolocator_platform_interface/geolocator_platform_interface.dart';
import 'package:soshal_flutter/ffi/permissions.dart' show LocationFixDto;
import 'package:soshal_flutter/services/permissions_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

class _MockGeolocator extends GeolocatorPlatform {
  final Position position;

  _MockGeolocator(this.position);

  @override
  Future<Position> getCurrentPosition({
    LocationSettings? locationSettings,
  }) async =>
      position;
}

Position _pos(double lat, double lng) => Position(
      latitude: lat,
      longitude: lng,
      timestamp: DateTime.fromMillisecondsSinceEpoch(0),
      accuracy: 10.0,
      altitude: 0.0,
      altitudeAccuracy: 0.0,
      heading: 0.0,
      headingAccuracy: 0.0,
      speed: 0.0,
      speedAccuracy: 0.0,
    );

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final env = bootstrapTestEnv('test-permissions');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
    PermissionsService.debugPollDelay = Duration.zero;
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

    test('ensureCameraMic granted when already granted, no dialog fired',
        () async {
      api.stubBool('crateFfiPermissionsPermissionsCameraMicGranted', true);

      final result = await PermissionsService.ensureCameraMic();
      expect(result.granted, isTrue);
      expect(
        api.callCount('crateFfiPermissionsPermissionsCameraMicRequest'),
        0,
      );
    });

    test('ensureCameraMic requests then polls until granted', () async {
      api.stub('crateFfiPermissionsPermissionsCameraMicGranted', (_) {
        return api.callCount('crateFfiPermissionsPermissionsCameraMicGranted') >
            1;
      });
      api.stubBool('crateFfiPermissionsPermissionsCameraMicRequest', true);

      final result = await PermissionsService.ensureCameraMic();
      expect(result.granted, isTrue);
      expect(
        api.callCount('crateFfiPermissionsPermissionsCameraMicRequest'),
        1,
      );
    });

    test('ensureCameraMic denied with reason when still denied', () async {
      api.stubBool('crateFfiPermissionsPermissionsCameraMicGranted', false);
      api.stubBool('crateFfiPermissionsPermissionsCameraMicRequest', true);
      api.stubBool(
          'crateFfiPermissionsPermissionsCameraMicPermanentlyDenied', false);

      final result = await PermissionsService.ensureCameraMic();
      expect(result.granted, isFalse);
      expect(result.reason, contains('camera/microphone'));
    });

    test('ensureCameraMic reports permanent denial', () async {
      api.stubBool('crateFfiPermissionsPermissionsCameraMicGranted', false);
      api.stubBool('crateFfiPermissionsPermissionsCameraMicRequest', true);
      api.stubBool(
          'crateFfiPermissionsPermissionsCameraMicPermanentlyDenied', true);

      final result = await PermissionsService.ensureCameraMic();
      expect(result.granted, isFalse);
      expect(result.reason, contains('permanently denied'));
    });

    test('ensureNotifications granted when already granted', () async {
      api.stubBool('crateFfiPermissionsPermissionsNotificationsGranted', true);

      final result = await PermissionsService.ensureNotifications();
      expect(result.granted, isTrue);
      expect(
        api.callCount('crateFfiPermissionsPermissionsNotificationsRequest'),
        0,
      );
    });

    test('ensureNotifications requests then polls until granted', () async {
      api.stub('crateFfiPermissionsPermissionsNotificationsGranted', (_) {
        return api.callCount(
                'crateFfiPermissionsPermissionsNotificationsGranted') >
            1;
      });
      api.stubBool('crateFfiPermissionsPermissionsNotificationsRequest', true);

      final result = await PermissionsService.ensureNotifications();
      expect(result.granted, isTrue);
      expect(
        api.callCount('crateFfiPermissionsPermissionsNotificationsRequest'),
        1,
      );
    });

    test('ensureNotifications denied with reason when still denied', () async {
      api.stubBool('crateFfiPermissionsPermissionsNotificationsGranted', false);
      api.stubBool('crateFfiPermissionsPermissionsNotificationsRequest', true);
      api.stubBool(
          'crateFfiPermissionsPermissionsNotificationsPermanentlyDenied',
          false);

      final result = await PermissionsService.ensureNotifications();
      expect(result.granted, isFalse);
      expect(result.reason, contains('Notification permission denied'));
    });

    test('isPermanentlyDenied mirrors FFI state', () async {
      api.stubBool(
          'crateFfiPermissionsPermissionsCameraMicPermanentlyDenied', true);

      expect(await PermissionsService.isPermanentlyDenied(), isTrue);
    });

    test('openSettings calls FFI', () async {
      api.stubBool('crateFfiPermissionsPermissionsOpenSettings', true);

      expect(await PermissionsService.openSettings(), isTrue);
    });

    test('currentPosition returns coords when permission + service ok',
        () async {
      api.stubBool('crateFfiPermissionsPermissionsLocationGranted', true);
      api.stubBool('crateFfiPermissionsPermissionsLocationEnabled', true);
      GeolocatorPlatform.instance = _MockGeolocator(_pos(52.52, 13.40));

      final location = await PermissionsService.currentPosition();
      expect(location.ok, isTrue);
      expect(location.latitude, closeTo(52.52, 1e-9));
      expect(location.longitude, closeTo(13.40, 1e-9));
    });

    test('currentPosition fails when permission denied', () async {
      api.stubBool('crateFfiPermissionsPermissionsLocationGranted', false);
      api.stubBool('crateFfiPermissionsPermissionsLocationRequest', true);

      final location = await PermissionsService.currentPosition();
      expect(location.ok, isFalse);
      expect(location.error, contains('denied'));
    });

    test('currentPosition fails when location service off', () async {
      api.stubBool('crateFfiPermissionsPermissionsLocationGranted', true);
      api.stubBool('crateFfiPermissionsPermissionsLocationEnabled', false);
      GeolocatorPlatform.instance = _MockGeolocator(_pos(0, 0));

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

    test('ensureNotifications no-op granted off-Android', () async {
      final result = await PermissionsService.ensureNotifications();
      expect(result.granted, isTrue);
    });

    test('currentPosition reads portal fix coords', () async {
      api.stub('crateFfiPermissionsPermissionsLocationPortalFix', (_) async {
        return const LocationFixDto(latitude: 51.5, longitude: -0.12);
      });

      final location = await PermissionsService.currentPosition();
      expect(location.ok, isTrue);
      expect(location.latitude, closeTo(51.5, 1e-9));
      expect(location.longitude, closeTo(-0.12, 1e-9));
    });

    test('currentPosition surfaces portal denial', () async {
      api.stub('crateFfiPermissionsPermissionsLocationPortalFix', (_) {
        throw Exception('DENIED: Location permission denied');
      });

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