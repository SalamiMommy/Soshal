import 'package:flutter_test/flutter_test.dart';
import './helpers/test_env.dart';

import 'package:soshal_flutter/ffi/moderation.dart';

void main() {
  test('moderation wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubBool('crateFfiModerationModerationMuteUser', true);
    api.stubListString('crateFfiModerationModerationGetMuted', ['u1']);
    api.stubBool('crateFfiModerationModerationShouldFilter', false);
    api.stubString('crateFfiModerationModerationCreateJuryCase', '{}');

    final m = moderationMuteUser(muterPubkey: 'a', targetPubkey: 'b');
    expect(m, true);

    final list = moderationGetMuted(userPubkey: 'a');
    expect(list, ['u1']);

    final f = moderationShouldFilter(content: 'x', userPubkey: 'a');
    expect(f, false);

    final c = moderationCreateJuryCase(caseId: 'c', targetPubkey: 't', reason: 'r', threshold: 3, totalJurors: 5, groupPubkey: 'g');
    expect(c, '{}');

    expect(api.callCount('crateFfiModerationModerationMuteUser'), 1);
  });
}
