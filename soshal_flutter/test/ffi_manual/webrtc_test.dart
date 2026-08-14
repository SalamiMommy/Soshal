import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/test/helpers/test_env.dart';

import 'package:soshal_flutter/ffi/webrtc.dart';

void main() {
  test('webrtc wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiWebrtcWebrtcGetIceConfig', '{"ice":true}');
    api.stubListString('crateFfiWebrtcWebrtcGetStunServers', ['stun:1']);
    api.stubBool('crateFfiWebrtcWebrtcValidateSdp', true);

    final cfg = webrtcGetIceConfig(privacyLevel: 'public');
    expect(cfg, '{"ice":true}');

    final stuns = webrtcGetStunServers();
    expect(stuns, ['stun:1']);

    final ok = webrtcValidateSdp(sdp: 'sdp', forceRelay: false);
    expect(ok, true);

    expect(api.callCount('crateFfiWebrtcWebrtcGetIceConfig'), 1);
  });
}
