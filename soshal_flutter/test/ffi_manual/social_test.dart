import 'package:flutter_test/flutter_test.dart';
import '../helpers/test_env.dart';

import 'package:soshal_flutter/ffi/social.dart';

void main() {
  test('socialFriendSuggestions forwards to api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1;

    api.stubListString('crateFfiSocialSocialFriendSuggestions', ['alice', 'bob']);

    final res = socialFriendSuggestions();
    expect(res, ['alice', 'bob']);
    expect(api.callCount('crateFfiSocialSocialFriendSuggestions'), 1);
  });
}
