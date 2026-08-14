// Manual ffi tests for identity
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/identity.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-identity-manual');
  final api = env.$1;

  test('profile/store/search/trust/wot/follow/unfollow', () {
    api.stubString('crateFfiIdentityIdentityGetProfile', '{}');
    api.stubBool('crateFfiIdentityIdentityStoreProfile', true);
    api.stubString('crateFfiIdentityIdentitySearchUsers', '[]');
    api.stub('crateFfiIdentityIdentityGetTrustScore', (_) => 0.5);
    api.stubString('crateFfiIdentityIdentityGetWotStatus', 'trusted');
    api.stubString('crateFfiIdentityIdentityFollowUser', 'evt');
    api.stubBool('crateFfiIdentityIdentityUnfollowUser', true);
    api.stubString('crateFfiIdentityIdentityFetchFollows', '[]');
    api.stubString('crateFfiIdentityIdentityPublishRelayList', 'rid');
    api.stubString('crateFfiIdentityIdentityPublishCustomProfile', 'cid');
    api.stubBool('crateFfiIdentityIdentityBlockUser', true);
    api.stubListString('crateFfiIdentityIdentityGetBlockedUsers', ['b']);
    api.stubBool('crateFfiIdentityIdentityIsBlocked', false);

    final p = identityGetProfile(pubkey: 'u');
    final s = identityStoreProfile(profile: '{}');
    final res = identitySearchUsers(query: 'q', limit: 10);
    final trust = identityGetTrustScore(sourcePubkey: 'a', targetPubkey: 'b');
    final wot = identityGetWotStatus(targetPubkey: 'b', viewerPubkey: 'a');
    final f = identityFollowUser(pubkey: 'b');
    final unf = identityUnfollowUser(pubkey: 'b');
    final follows = identityFetchFollows(pubkey: 'u');
    final rel = identityPublishRelayList(relayUrls: ['wss://x']);
    final cp = identityPublishCustomProfile(pubkey: 'u', profileJson: '{}');
    final block = identityBlockUser(blockerPubkey: 'a', targetPubkey: 'b');
    final blocked = identityGetBlockedUsers(pubkey: 'u');
    final isBlocked = identityIsBlocked(checkerPubkey: 'a', targetPubkey: 'b');

    expect(p, '{}');
    expect(s, true);
    expect(res, '[]');
    expect(trust, 0.5);
    expect(wot, 'trusted');
    expect(f, 'evt');
    expect(unf, true);
    expect(follows, '[]');
    expect(rel, 'rid');
    expect(cp, 'cid');
    expect(block, true);
    expect(blocked, ['b']);
    expect(isBlocked, false);
    expect(api.callCount('crateFfiIdentityIdentityGetProfile'), 1);
  });
}