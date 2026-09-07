// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/feed_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-feed');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('FeedService', () {
    test('fetchFeed fetches and stores posts', () async {
      final feed = FeedService();
      var notified = 0;
      feed.addListener(() => notified++);

      const postJson =
          '[{"id":"p1","pubkey":"pk1","content":"hello","created_at":1000}]';
      api.stubString('crateFfiFeedFeedFetchEvents', postJson);
      api.stubStringBuilder('crateFfiContentContentDecompressJsonDict',
          (inv) => api.namedArg(inv, 'encoded') as String);

      expect(feed.isLoading, false);
      final result = await feed.fetchFeed();
      // One microtask tick: decode now yields to the event loop (background
      // isolate in prod, main-thread fallback under test), so the deferred
      // notify lands after the awaited call.
      await Future<void>.delayed(Duration.zero);
      expect(feed.isLoading, false);
      expect(result.length, 1);
      expect(feed.posts.length, 1);
      expect(feed.posts.first.content, 'hello');
      expect(notified, 1);
      expect(feed.lastError, isNull);
    });

    test('fetchFeed with offset appends posts', () async {
      final feed = FeedService();

      const post1Json =
          '[{"id":"p1","pubkey":"pk1","content":"msg1","created_at":1000}]';
      const post2Json =
          '[{"id":"p2","pubkey":"pk1","content":"msg2","created_at":1001}]';
      api.stubString('crateFfiFeedFeedFetchEvents', post1Json);

      await feed.fetchFeed(offset: 0);
      expect(feed.posts.length, 1);

      api.stubString('crateFfiFeedFeedFetchEvents', post2Json);
      await feed.fetchFeed(offset: 1);
      expect(feed.posts.length, 2);
    });

    test('fetchFeed limits total posts to 100', () async {
      final feed = FeedService();

      // Fetch 100 posts
      final postList = List.generate(100, (i) => {
            'id': 'p$i',
            'pubkey': 'pk1',
            'content': 'msg $i',
            'created_at': 1000 + i,
          });
      api.stubString('crateFfiFeedFeedFetchEvents', jsonEncode(postList));
      await feed.fetchFeed(offset: 0);
      expect(feed.posts.length, 100);

      // Fetch 20 more
      final newPosts =
          List.generate(20, (i) => {
                'id': 'new$i',
                'pubkey': 'pk1',
                'content': 'new $i',
                'created_at': 2000 + i,
              });
      api.stubString('crateFfiFeedFeedFetchEvents', jsonEncode(newPosts));
      await feed.fetchFeed(offset: 100);

      expect(feed.posts.length, 100);
      expect(feed.posts.first.eventId, 'p20');
      expect(feed.posts.last.eventId, 'new19');
    });

    test('fetchFeed sets error on exception', () async {
      final feed = FeedService();
      api.stub('crateFfiFeedFeedFetchEvents', (_) {
        throw Exception('Fetch failed');
      });

      expect(() => feed.fetchFeed(), throwsException);
      expect(feed.lastError, isNotNull);
      expect(feed.isLoading, false);
    });

    test('fetchWindow fetches and stores posts', () async {
      final feed = FeedService();
      const postJson =
          '[{"id":"p1","pubkey":"pk1","content":"hello","created_at":1000}]';
      api.stubString('crateFfiFeedFeedFetchWindow', postJson);

      final result = await feed.fetchWindow(startIndex: 0, limit: 10);

      expect(result.length, 1);
      expect(feed.posts.first.eventId, 'p1');
      final inv = api.callsOf('crateFfiFeedFeedFetchWindow').single;
      expect(api.namedArg(inv, 'startIndex'), 0);
      expect(api.namedArg(inv, 'limit'), 10);
    });

    test('fetchAuthorPosts fetches posts without modifying global feed', () async {
      final feed = FeedService();
      var notified = 0;
      feed.addListener(() => notified++);

      const postJson =
          '[{"id":"p1","pubkey":"pk_target","content":"user post","created_at":1000},'
          '{"id":"p2","pubkey":"pk_other","content":"other post","created_at":999}]';
      api.stubString('crateFfiFeedFeedFetchWindow', postJson);

      final result = await feed.fetchAuthorPosts('pk_target', limit: 20);

      expect(result.length, 1);
      expect(result.first.eventId, 'p1');
      expect(result.first.pubkey, 'pk_target');
      // Global feed state must NOT be modified
      expect(feed.posts.isEmpty, true);
      expect(notified, 0);

      final inv = api.callsOf('crateFfiFeedFeedFetchWindow').single;
      expect(api.namedArg(inv, 'startIndex'), 0);
      expect(api.namedArg(inv, 'limit'), 20);
      expect(api.namedArg(inv, 'audience'), 'pk_target');
    });

    test('enqueueOutboxPost creates offline post', () async {
      final feed = FeedService();
      var notified = 0;
      feed.addListener(() => notified++);

      api.stubString('crateFfiSyncSyncEnqueueOutbox', 'outbox_id_123');
      api.stubStringBuilder('crateFfiContentContentCompressJsonDict',
          (inv) => api.namedArg(inv, 'data') as String);

      final id = await feed.enqueueOutboxPost('My post content');

      expect(id, 'outbox_id_123');
      expect(notified, 1);
      final inv = api.callsOf('crateFfiSyncSyncEnqueueOutbox').single;
      expect(api.namedArg(inv, 'actionType'), 'post');
      expect(api.namedArg(inv, 'payloadJson'), contains('My post content'));
    });

    test('enqueueOutboxPost with media passes mediaPath', () async {
      final feed = FeedService();
      api.stubString('crateFfiSyncSyncEnqueueOutbox', 'id123');

      await feed.enqueueOutboxPost('post', mediaPath: '/path/to/image.jpg');

      final inv = api.callsOf('crateFfiSyncSyncEnqueueOutbox').single;
      expect(api.namedArg(inv, 'mediaPath'), '/path/to/image.jpg');
    });

    test('loadMore appends more posts with offset', () async {
      final feed = FeedService();
      const postJson =
          '[{"id":"p1","pubkey":"pk1","content":"msg","created_at":1000}]';
      api.stubString('crateFfiFeedFeedFetchEvents', postJson);

      await feed.fetchFeed(limit: 20, offset: 0);
      expect(feed.posts.length, 1);

      api.stubString('crateFfiFeedFeedFetchEvents', postJson);
      await feed.loadMore(limit: 20);

      final calls = api.callsOf('crateFfiFeedFeedFetchEvents');
      expect(calls.length, 2);
    });

    test('loadPinnedPosts loads from settings', () async {
      final feed = FeedService();
      var notified = 0;
      feed.addListener(() => notified++);

      const pinnedJson = '["post1", "post2", "post3"]';
      api.stubString('crateFfiDbDbGetSetting', pinnedJson);

      final result = await feed.loadPinnedPosts();

      expect(result.length, 3);
      expect(result, contains('post1'));
      expect(feed.pinnedPosts, result);
      expect(notified, 1);
    });

    test('loadPinnedPosts handles empty settings', () async {
      final feed = FeedService();
      api.stubString('crateFfiDbDbGetSetting', '');

      final result = await feed.loadPinnedPosts();

      expect(result.isEmpty, true);
    });

    test('loadPinnedPosts sets error gracefully', () async {
      final feed = FeedService();
      api.stub('crateFfiDbDbGetSetting', (_) {
        throw Exception('Settings error');
      });

      final result = await feed.loadPinnedPosts();

      expect(result.isEmpty, true);
      expect(feed.lastError, isNotNull);
    });

    test('isPinned checks pinned status', () async {
      final feed = FeedService();
      const pinnedJson = '["post1", "post2"]';
      api.stubString('crateFfiDbDbGetSetting', pinnedJson);

      await feed.loadPinnedPosts();

      expect(feed.isPinned('post1'), true);
      expect(feed.isPinned('post3'), false);
    });

    test('togglePin adds new post to pinned', () async {
      final feed = FeedService();
      var notified = 0;
      feed.addListener(() => notified++);

      api.stubString('crateFfiDbDbGetSetting', '[]');
      api.stub("crateFfiDbDbSetSetting", (_) => true);

      await feed.loadPinnedPosts();
      final result = await feed.togglePin('newpost');

      expect(result, true);
      expect(feed.isPinned('newpost'), true);
      expect(notified, 2);
    });

    test('togglePin removes pinned post', () async {
      final feed = FeedService();
      api.stubString('crateFfiDbDbGetSetting', '["post1"]');
      api.stub("crateFfiDbDbSetSetting", (_) => true);

      await feed.loadPinnedPosts();
      expect(feed.isPinned('post1'), true);

      final result = await feed.togglePin('post1');

      expect(result, false);
      expect(feed.isPinned('post1'), false);
    });

    test('validateNote delegates to FFI', () {
      final feed = FeedService();
      api.stubBool('crateFfiFeedFeedValidateNote', true);
      expect(feed.validateNote('clean content'), isTrue);

      api.stubBool('crateFfiFeedFeedValidateNote', false);
      expect(feed.validateNote('bad content'), isFalse);
    });

    test('insertLivePost filters moderated content', () {
      final feed = FeedService();
      api.stubBool('crateFfiFeedFeedValidateNote', false);
      final badPost = FeedPost(
        eventId: 'bad1',
        pubkey: 'pk1',
        content: 'selling cp pack',
        createdAt: 1000,
        reactions: 0,
        replies: 0,
        reposts: 0,
        liked: false,
      );
      feed.insertLivePost(badPost);
      expect(feed.posts.isEmpty, isTrue);

      api.stubBool('crateFfiFeedFeedValidateNote', true);
      final goodPost = FeedPost(
        eventId: 'good1',
        pubkey: 'pk1',
        content: 'Clean post',
        createdAt: 1000,
        reactions: 0,
        replies: 0,
        reposts: 0,
        liked: false,
      );
      feed.insertLivePost(goodPost);
      expect(feed.posts.length, 1);
      expect(feed.posts.first.eventId, 'good1');
    });

    test('publishTextNote and publishReply pass content', () async {
      final feed = FeedService();
      api.stubString('crateFfiFeedFeedPublishTextNote', 'signed_event_json');
      api.stubString('crateFfiFeedFeedPublishReply', 'signed_reply_json');

      final res1 = await feed.publishTextNote('hello', [['t', 'rust']], 'pk1');
      expect(res1, 'signed_event_json');

      final res2 = await feed.publishReply('reply text', 'root1', 'parent1', 'pk1');
      expect(res2, 'signed_reply_json');
    });

    test('FeedPost model parses and serializes', () {
      const json = {
        'id': 'p1',
        'pubkey': 'pk1',
        'content': 'hello',
        'created_at': 1000,
      };
      final post = FeedPost.fromJson(json);
      expect(post.eventId, 'p1');
      expect(post.pubkey, 'pk1');
      expect(post.content, 'hello');
      expect(post.createdAt, 1000);
    });
  });
}

