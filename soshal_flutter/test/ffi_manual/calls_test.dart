// Manual ffi tests for calls
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/calls.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-manual');
  final api = env.$1;

  test('callsSendSignal and fetch/sanitize/ice', () async {
    api.stub('crateFfiCallsCallsSendSignal', (_) => Future.value('ev'));
    api.stubString('crateFfiCallsCallsFetchSignals', '[]');
    api.stubString('crateFfiCallsCallsSanitizeSdp', 'sdp');
    api.stubString('crateFfiCallsCallsIceConfig', 'ice');

    final ev = await callsSendSignal(signalType: 'offer', targetPubkey: 't', callId: 'c');
    final sigs = callsFetchSignals(myPubkey: 'me');
    final s = callsSanitizeSdp(sdp: 's', forceRelay: true);
    final ice = callsIceConfig(privacyLevel: 'low', stunUrl: 'stun');

    expect(ev, 'ev');
    expect(sigs, '[]');
    expect(s, 'sdp');
    expect(ice, 'ice');
    expect(api.callCount('crateFfiCallsCallsSendSignal'), 1);
  });
}
