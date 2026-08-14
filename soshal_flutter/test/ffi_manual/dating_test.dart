import 'package:flutter_test/flutter_test.dart';
import 'helpers/test_env.dart';

import 'package:soshal_flutter/ffi/dating.dart';

void main() {
  test('dating wrappers call api', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiDatingDatingFetchProfiles', '[]');
    api.stubString('crateFfiDatingDatingGetProfile', '{}');
    api.stubString('crateFfiDatingDatingCreateProfile', '{}');
    api.stubBool('crateFfiDatingDatingUpdateProfile', true);
    api.stubBool('crateFfiDatingDatingLike', true);
    api.stubBool('crateFfiDatingDatingPass', true);
    api.stubString('crateFfiDatingDatingFetchLikes', '[]');
    api.stubString('crateFfiDatingDatingFetchMatches', '[]');
    api.stub('crateFfiDatingDatingCalculateScore', (_) => 0.5);

    final pf = datingFetchProfiles(userPubkey: 'u', limit: 10);
    expect(pf, '[]');

    final prof = datingGetProfile(profileId: 'id');
    expect(prof, '{}');

    final created = datingCreateProfile(userPubkey: 'u', name: 'n', age: 30, location: 'l', bio: 'b', imagesJson: '[]', interestsJson: '[]');
    expect(created, '{}');

    final liked = datingLike(userPubkey: 'u', profileId: 'id');
    expect(liked, true);

    final score = datingCalculateScore(userPubkey: 'u', targetPubkey: 't', preferencesJson: '{}');
    expect(score, 0.5);

    expect(api.callCount('crateFfiDatingDatingFetchProfiles'), 1);
  });
}
