// Generated callable ffi tests for dating
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/dating.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-dating');
  final api = env.$1;

  test('datingGetOwnProfile calls String datingGetOwnProfile({required String userPubkey}) => RustLib.instance.api', () {
    api.stubString('String datingGetOwnProfile({required String userPubkey}) => RustLib.instance.api', 'stub');
    final res = datingGetOwnProfile(userPubkey}: "x");
    expect(res, 'stub');
    expect(api.callCount('String datingGetOwnProfile({required String userPubkey}) => RustLib.instance.api'), 1);
  });

  test('datingDeleteProfile calls bool datingDeleteProfile({required String userPubkey}) => RustLib.instance.api', () {
    api.stubBool('bool datingDeleteProfile({required String userPubkey}) => RustLib.instance.api', true);
    final res = datingDeleteProfile(userPubkey}: "x");
    expect(res, true);
    expect(api.callCount('bool datingDeleteProfile({required String userPubkey}) => RustLib.instance.api'), 1);
  });

  test('datingFetchMatches calls String datingFetchMatches({required String userPubkey}) => RustLib.instance.api', () {
    api.stubString('String datingFetchMatches({required String userPubkey}) => RustLib.instance.api', 'stub');
    final res = datingFetchMatches(userPubkey}: "x");
    expect(res, 'stub');
    expect(api.callCount('String datingFetchMatches({required String userPubkey}) => RustLib.instance.api'), 1);
  });

}
