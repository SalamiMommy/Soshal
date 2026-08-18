// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/daemon_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-daemon');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  test('extractDaemons returns true on success', () async {
    api.stubBool('crateFfiDaemonDaemonExtractDaemons', true);

    final ok = await DaemonService.extractDaemons();
    expect(ok, true);
  });

  test('getDaemonPath forwards name and returns string', () async {
    api.stubString('crateFfiDaemonDaemonGetDaemonPath', '/tmp/i2pd');

    final path = await DaemonService.getDaemonPath('i2pd');
    expect(path, '/tmp/i2pd');
    final inv = api.callsOf('crateFfiDaemonDaemonGetDaemonPath').single;
    expect(api.namedArg(inv, 'daemonName'), 'i2pd');
  });

  test('getDaemonPath returns null when Rust returns empty', () async {
    api.stubString('crateFfiDaemonDaemonGetDaemonPath', '');

    expect(await DaemonService.getDaemonPath('i2pd'), isNull);
  });

  test('areDaemonsAvailable returns bool', () async {
    api.stubBool('crateFfiDaemonDaemonAreDaemonsAvailable', true);

    final ok = await DaemonService.areDaemonsAvailable();
    expect(ok, true);
  });

  test('getDaemonStatus maps JSON to DaemonStatus', () async {
    api.stubString(
      'crateFfiDaemonDaemonGetDaemonStatus',
      '{"i2pd":true,"freenet":false,"reticulum":true}',
    );

    final map = await DaemonService.getDaemonStatus();
    expect(map['i2pd'], true);

    final status = DaemonStatus.fromMap(map);
    expect(status.i2pdAvailable, true);
    expect(status.availableCount, 2);
    expect(status.allAvailable, false);
  });

  test('start/stop/isRunning wrappers return false on exception', () async {
    api.stub('crateFfiDaemonDaemonStartDaemons', (_) {
      throw Exception('native failure');
    });
    api.stub('crateFfiDaemonDaemonStopDaemons', (_) {
      throw Exception('native failure');
    });
    api.stub('crateFfiDaemonDaemonIsI2PdRunning', (_) {
      throw Exception('native failure');
    });
    api.stub('crateFfiDaemonDaemonIsRnsdRunning', (_) {
      throw Exception('native failure');
    });

    expect(await DaemonService.startDaemons(), false);
    expect(await DaemonService.stopDaemons(), false);
    expect(await DaemonService.isI2pdRunning(), false);
    expect(await DaemonService.isRnsdRunning(), false);
  });

  test('startDaemons returns true on success', () async {
    api.stubBool('crateFfiDaemonDaemonStartDaemons', true);

    expect(await DaemonService.startDaemons(), true);
  });

  test('daemon name constants align with Rust status keys', () {
    expect(DaemonService.i2pd, 'i2pd');
    expect(DaemonService.freenet, 'freenet');
    expect(DaemonService.reticulum, 'rnsd');
  });
}