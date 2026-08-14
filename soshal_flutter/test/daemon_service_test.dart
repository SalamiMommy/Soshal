// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/daemon_service.dart';

void main() {
  const channel = MethodChannel('com.example.soshal/daemons');

  setUp(() {
    TestWidgetsFlutterBinding.ensureInitialized();
    channel.setMockMethodCallHandler(null);
  });

  tearDown(() async {
    channel.setMockMethodCallHandler(null);
  });

  test('extractDaemons returns true on success', () async {
    channel.setMockMethodCallHandler((call) async {
      if (call.method == 'extractDaemons') return true;
      return null;
    });

    final ok = await DaemonService.extractDaemons();
    expect(ok, true);
  });

  test('getDaemonPath returns string and forwards arg', () async {
    channel.setMockMethodCallHandler((call) async {
      expect(call.method, 'getDaemonPath');
      expect(call.arguments['daemonName'], 'i2pd');
      return '/tmp/i2pd';
    });

    final path = await DaemonService.getDaemonPath('i2pd');
    expect(path, '/tmp/i2pd');
  });

  test('areDaemonsAvailable returns bool', () async {
    channel.setMockMethodCallHandler((call) async => true);
    final ok = await DaemonService.areDaemonsAvailable();
    expect(ok, true);
  });

  test('getDaemonStatus maps to DaemonStatus', () async {
    channel.setMockMethodCallHandler((call) async => {
          'i2pd': true,
          'freenet': false,
          'reticulum': true,
        });

    final map = await DaemonService.getDaemonStatus();
    expect(map['i2pd'], true);

    final status = DaemonStatus.fromMap(map);
    expect(status.i2pdAvailable, true);
    expect(status.availableCount, 2);
    expect(status.allAvailable, false);
  });

  test('start/stop/isRunning wrappers return false on exception', () async {
    channel.setMockMethodCallHandler((call) async {
      throw Exception('native failure');
    });

    expect(await DaemonService.startDaemons(), false);
    expect(await DaemonService.stopDaemons(), false);
    expect(await DaemonService.isI2pdRunning(), false);
    expect(await DaemonService.isRnsdRunning(), false);
  });
}
