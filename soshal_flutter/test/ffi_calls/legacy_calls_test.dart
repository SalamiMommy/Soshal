// Generated callable ffi tests for legacy
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/legacy.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-legacy');
  final api = env.$1;

  test('legacyStreamChatClear calls bool legacyStreamChatClear({required String streamId}) => RustLib.instance.api', () {
    api.stubBool('bool legacyStreamChatClear({required String streamId}) => RustLib.instance.api', true);
    final res = legacyStreamChatClear(streamId}: "x");
    expect(res, true);
    expect(api.callCount('bool legacyStreamChatClear({required String streamId}) => RustLib.instance.api'), 1);
  });

  test('legacyProfileNodes calls String legacyProfileNodes({required String userPubkey}) => RustLib.instance.api', () {
    api.stubString('String legacyProfileNodes({required String userPubkey}) => RustLib.instance.api', 'stub');
    final res = legacyProfileNodes(userPubkey}: "x");
    expect(res, 'stub');
    expect(api.callCount('String legacyProfileNodes({required String userPubkey}) => RustLib.instance.api'), 1);
  });

}
