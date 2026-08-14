// Generated callable ffi tests for network
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/network.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-network');
  final api = env.$1;

  test('i2PStartSession calls String i2PStartSession({String? destination}) => RustLib.instance.api', () {
    api.stubString('String i2PStartSession({String? destination}) => RustLib.instance.api', 'stub');
    final res = i2PStartSession(destination}: "x");
    expect(res, 'stub');
    expect(api.callCount('String i2PStartSession({String? destination}) => RustLib.instance.api'), 1);
  });

  test('i2PStopSession calls crateFfiNetworkI2PStopSession', () {
    api.stubBool('crateFfiNetworkI2PStopSession', true);
    final res = i2PStopSession();
    expect(res, true);
    expect(api.callCount('crateFfiNetworkI2PStopSession'), 1);
  });

  test('networkReticulumAnnounce calls bool networkReticulumAnnounce({required String pubkey}) => RustLib.instance.api', () {
    api.stubBool('bool networkReticulumAnnounce({required String pubkey}) => RustLib.instance.api', true);
    final res = networkReticulumAnnounce(pubkey}: "x");
    expect(res, true);
    expect(api.callCount('bool networkReticulumAnnounce({required String pubkey}) => RustLib.instance.api'), 1);
  });

}
