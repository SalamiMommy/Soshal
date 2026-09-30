// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/music_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

/// Music FFI fns are async-only — handlers must return a Future.
void stubFutureString(String method, String result) {
  api.stub(method, (_) async => result);
}

void main() {
  final env = bootstrapTestEnv('test-music');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('MusicService', () {
    test('fetchTracks parses tracks, updates state and notifies', () async {
      final music = MusicService();
      var notified = 0;
      music.addListener(() => notified++);
      stubFutureString(
        'crateFfiMusicMusicFetch',
        '[{"id":"tr-1","pubkey":"pk-1","audioUrl":"ipfs://a",'
            '"blobHash":"abababababababababababababababababababababababababababababababab",'
            '"mediaSize":4096,'
            '"title":"Night Drive","thumbnail":"ipfs://t",'
            '"hashtags":["synth","chill"],"d":"d-1",'
            '"audience":"public","createdAt":1700000001}]',
      );

      final tracks = await music.fetchTracks(limit: 5);
      final track = tracks.single;
      expect(track.id, 'tr-1');
      expect(track.pubkey, 'pk-1');
      expect(track.audioUrl, 'ipfs://a');
      expect(track.blobHash,
          'abababababababababababababababababababababababababababababababab');
      expect(track.mediaSize, 4096);
      expect(track.title, 'Night Drive');
      expect(track.hashtags, ['synth', 'chill']);
      expect(track.audience, 'public');
      expect(track.createdAt, 1700000001);
      expect(music.tracks.single.id, 'tr-1');
      expect(music.lastError, isNull);
      expect(notified, 1);

      final inv = api.callsOf('crateFfiMusicMusicFetch').single;
      expect(api.namedArg(inv, 'limit'), BigInt.from(5));
      expect(api.namedArg(inv, 'author'), isNull);
    });

    test('fetchTracks with author filter passes it through', () async {
      final music = MusicService();
      stubFutureString('crateFfiMusicMusicFetch', '[]');

      await music.fetchTracks(author: 'pk-42');
      final inv = api.callsOf('crateFfiMusicMusicFetch').single;
      expect(api.namedArg(inv, 'author'), 'pk-42');
      expect(music.tracks, isEmpty);
    });

    test('publishTrack returns id and forwards args', () async {
      final music = MusicService();
      stubFutureString('crateFfiMusicMusicPublish', 'ev-pub-1');

      final id = await music.publishTrack(
        mediaSource: '/tmp/night-drive.mp3',
        title: 'Night Drive',
        thumbnail: 'ipfs://t',
        hashtags: const ['synth'],
        audience: 'followers',
      );
      expect(id, 'ev-pub-1');
      expect(music.lastError, isNull);

      final inv = api.callsOf('crateFfiMusicMusicPublish').single;
      expect(api.namedArg(inv, 'mediaSource'), '/tmp/night-drive.mp3');
      expect(api.namedArg(inv, 'title'), 'Night Drive');
      expect(api.namedArg(inv, 'thumbnail'), 'ipfs://t');
      expect(api.namedArg(inv, 'hashtags'), ['synth']);
      expect(api.namedArg(inv, 'audience'), 'followers');
    });

    test('shareToFeed and comment return event ids', () async {
      final music = MusicService();
      stubFutureString('crateFfiMusicMusicShareToFeed', 'ev-share-1');
      stubFutureString('crateFfiMusicMusicComment', 'ev-comment-1');

      final shareId = await music.shareToFeed(
        trackId: 'tr-1',
        trackPubkey: 'pk-1',
        trackD: 'd-1',
        message: 'check this',
        hashtags: const ['music'],
      );
      expect(shareId, 'ev-share-1');
      final shareInv = api.callsOf('crateFfiMusicMusicShareToFeed').single;
      expect(api.namedArg(shareInv, 'trackId'), 'tr-1');
      expect(api.namedArg(shareInv, 'trackD'), 'd-1');
      expect(api.namedArg(shareInv, 'trackPubkey'), 'pk-1');
      expect(api.namedArg(shareInv, 'message'), 'check this');

      final commentId = await music.comment(
        trackPubkey: 'pk-1',
        trackD: 'd-1',
        content: 'nice',
      );
      expect(commentId, 'ev-comment-1');
      final commentInv = api.callsOf('crateFfiMusicMusicComment').single;
      expect(api.namedArg(commentInv, 'trackKind'), 31022);
      expect(api.namedArg(commentInv, 'trackD'), 'd-1');
      expect(api.namedArg(commentInv, 'content'), 'nice');
    });

    test('fetchComments parses mini event outputs', () async {
      final music = MusicService();
      stubFutureString(
        'crateFfiMusicMusicComments',
        '[{"id":"c-1","pubkey":"pk-9","textOverlay":"fire track",'
            '"createdAt":1700000002}]',
      );

      final comments =
          await music.fetchComments(trackPubkey: 'pk-1', trackD: 'd-1');
      final comment = comments.single;
      expect(comment.id, 'c-1');
      expect(comment.pubkey, 'pk-9');
      expect(comment.content, 'fire track');
      expect(comment.createdAt, 1700000002);
      expect(music.lastError, isNull);
    });

    test('errors set lastError, notify and rethrow', () async {
      final music = MusicService();
      var notified = 0;
      music.addListener(() => notified++);
      api.stub('crateFfiMusicMusicFetch', (_) => throw Exception('relay down'));

      await expectLater(music.fetchTracks(), throwsException);
      expect(music.lastError, contains('relay down'));
      expect(notified, 1);

      api.stub(
          'crateFfiMusicMusicPublish', (_) => throw Exception('sign failed'));
      await expectLater(
          music.publishTrack(mediaSource: '/tmp/x.mp3'), throwsException);
      expect(music.lastError, contains('sign failed'));
    });

    // ─── the saved-id mirror (6.4) ─────────────────────────────────────────
    //
    // `isTrackSaved` was `_savedTracks.any((t) => t.id == id)`, and four
    // screens call it per visible row, two of them through
    // `watch<MusicService>()`. It is now a set lookup, and the set has to stay
    // in step with the list at its two writers.

    test('isTrackSaved tracks the saved list across fetch and account switch',
        () async {
      final music = MusicService();
      expect(music.isTrackSaved('t-1'), isFalse);

      api.stubString('crateFfiMusicMusicSaved', _trackJson(['t-1', 't-2']));
      await music.fetchSavedTracks();
      expect(music.isTrackSaved('t-1'), isTrue);
      expect(music.isTrackSaved('t-2'), isTrue);
      expect(music.isTrackSaved('t-3'), isFalse);

      // A re-fetch replaces the list, so the mirror must replace too rather
      // than accumulate -- this is the case that separates a mirror from a
      // set that is only ever added to.
      api.stubString('crateFfiMusicMusicSaved', _trackJson(['t-2', 't-3']));
      await music.fetchSavedTracks();
      expect(music.isTrackSaved('t-1'), isFalse);
      expect(music.isTrackSaved('t-3'), isTrue);

      music.resetForAccountSwitch();
      expect(music.isTrackSaved('t-2'), isFalse);
      expect(music.isTrackSaved('t-3'), isFalse);
      expect(music.savedTracks, isEmpty);
    });

    test('a failed fetch leaves the previous mirror intact', () async {
      final music = MusicService();
      api.stubString('crateFfiMusicMusicSaved', _trackJson(['t-1']));
      await music.fetchSavedTracks();
      expect(music.isTrackSaved('t-1'), isTrue);

      api.stub('crateFfiMusicMusicSaved', (_) => throw Exception('db down'));
      await expectLater(music.fetchSavedTracks(), throwsException);

      // The list kept its old contents because the assignment never ran, so
      // the mirror has to agree with it rather than clearing.
      expect(music.savedTracks.map((t) => t.id), contains('t-1'));
      expect(music.isTrackSaved('t-1'), isTrue);
    });

    test('the mirror agrees with the list for every id', () async {
      final music = MusicService();
      api.stubString('crateFfiMusicMusicSaved', _trackJson(['a', 'b']));
      await music.fetchSavedTracks();

      final fromList = music.savedTracks.map((t) => t.id).toSet();
      for (final id in ['a', 'b', 'c', 'absent']) {
        expect(music.isTrackSaved(id), fromList.contains(id),
            reason: 'isTrackSaved($id) disagrees with savedTracks');
      }
    });
  });
}

/// A `crateFfiMusicMusicSaved` payload for the given ids. Only `id` matters
/// here, since the mirror keys on it.
String _trackJson(List<String> ids) {
  return '[${ids.map((id) => '{"id":"$id","pubkey":"pk-1",'
      '"audioUrl":"blob://x","blobHash":"h","mediaSize":1,'
      '"title":"t","thumbnail":"","hashtags":[],"d":"",'
      '"audience":"public","createdAt":1700000000}').join(',')}]';
}
