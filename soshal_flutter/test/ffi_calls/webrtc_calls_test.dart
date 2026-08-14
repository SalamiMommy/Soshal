// Generated callable ffi tests for webrtc
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/webrtc.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-webrtc');
  final api = env.$1;

  test('webrtcGetTurnServers calls String webrtcGetTurnServers({String? authToken}) => RustLib.instance.api', () {
    api.stubString('String webrtcGetTurnServers({String? authToken}) => RustLib.instance.api', 'stub');
    final res = webrtcGetTurnServers(authToken}: "x");
    expect(res, 'stub');
    expect(api.callCount('String webrtcGetTurnServers({String? authToken}) => RustLib.instance.api'), 1);
  });

}
