// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/bookmarks_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-bookmarks');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('BookmarksService', () {
    test('save returns bookmark id and passes pubkey/eventId through',
        () async {
      final bookmarks = BookmarksService();
      var notified = 0;
      bookmarks.addListener(() => notified++);
      api.stubString('crateFfiBookmarksBookmarksSave', 'bm-1');

      final id = await bookmarks.save('pk-1', 'ev-1');
      expect(id, 'bm-1');
      expect(bookmarks.lastError, isNull);
      expect(notified, 1);

      final inv = api.callsOf('crateFfiBookmarksBookmarksSave').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk-1');
      expect(api.namedArg(inv, 'eventId'), 'ev-1');
    });

    test('list parses rows, updates bookmarks and notifies', () async {
      final bookmarks = BookmarksService();
      var notified = 0;
      bookmarks.addListener(() => notified++);
      api.stubString(
        'crateFfiBookmarksBookmarksList',
        '[{"id":"bm-1","pubkey":"pk-1","event_id":"ev-1",'
            '"created_at":1700000001}]',
      );

      final rows = await bookmarks.list('pk-1', limit: 50, offset: 10);
      expect(rows.single.id, 'bm-1');
      expect(rows.single.pubkey, 'pk-1');
      expect(rows.single.eventId, 'ev-1');
      expect(rows.single.createdAt, 1700000001);
      expect(bookmarks.bookmarks.single.eventId, 'ev-1');
      expect(bookmarks.lastError, isNull);
      // `list` passes `onNotify: notifyDeferred` and now has an *async* body,
      // so `guard` takes its `r.then` path and the notify lands a microtask
      // after this resumption instead of before it. A sync body called
      // `onNotify()` synchronously. Same convention as the messaging tests.
      await flushMicrotasks();
      expect(notified, 1);

      final inv = api.callsOf('crateFfiBookmarksBookmarksList').single;
      expect(api.namedArg(inv, 'pubkey'), 'pk-1');
      expect(api.namedArg(inv, 'limit'), 50);
      expect(api.namedArg(inv, 'offset'), 10);
    });

    test('list with non-list payload resets bookmarks to empty', () async {
      final bookmarks = BookmarksService();
      api.stubString('crateFfiBookmarksBookmarksList', '{"not":"a list"}');

      final rows = await bookmarks.list('pk-1');
      expect(rows, isEmpty);
      expect(bookmarks.bookmarks, isEmpty);
    });

    test('delete removes bookmark by id', () async {
      final bookmarks = BookmarksService();
      api.stubBool('crateFfiBookmarksBookmarksDelete', true);

      final removed = await bookmarks.delete('bm-1');
      expect(removed, isTrue);
      final inv = api.callsOf('crateFfiBookmarksBookmarksDelete').single;
      expect(api.namedArg(inv, 'id'), 'bm-1');
    });

    test('save failure sets lastError and rethrows', () async {
      final bookmarks = BookmarksService();
      api.stub('crateFfiBookmarksBookmarksSave',
          (_) => throw Exception('write failed'));

      await expectLater(bookmarks.save('pk-1', 'ev-1'), throwsException);
      expect(bookmarks.lastError, contains('write failed'));
    });

    test('delete failure sets lastError and rethrows', () async {
      final bookmarks = BookmarksService();
      api.stub('crateFfiBookmarksBookmarksDelete',
          (_) => throw Exception('delete failed'));

      await expectLater(bookmarks.delete('bm-1'), throwsException);
      expect(bookmarks.lastError, contains('delete failed'));
    });

    test('resolvePosts builds posts from both map shapes and skips others',
        () async {
      final bookmarks = BookmarksService();
      api.stubString(
        'crateFfiBookmarksBookmarksResolvePosts',
        jsonEncode({
          'ev-1': {
            'event_id': 'ev-1',
            'pubkey': 'pk-1',
            'content': 'hello',
            'created_at': 1700000000,
            'reactions': 3,
          },
          // Decoded by `jsonDecode` as a plain `Map<dynamic, dynamic>`, which
          // the parser has to re-key — the value is a valid post.
          'ev-2': {
            'event_id': 'ev-2',
            'pubkey': 'pk-2',
            'content': 'second',
            'created_at': 1700000001,
          },
          // Not an object at all — skipped, not thrown.
          'ev-3': 'garbage',
        }),
      );

      final out = await bookmarks.resolvePosts(['ev-1', 'ev-2', 'ev-3']);

      expect(out.keys.toSet(), {'ev-1', 'ev-2'});
      expect(out['ev-1']!.content, 'hello');
      expect(out['ev-1']!.reactions, 3);
      expect(out['ev-2']!.content, 'second');
      expect(bookmarks.lastError, isNull);

      final inv = api.callsOf('crateFfiBookmarksBookmarksResolvePosts').single;
      expect(jsonDecode(api.namedArg(inv, 'idsJson') as String),
          ['ev-1', 'ev-2', 'ev-3']);
    });

    test('resolvePosts short-circuits an empty id list without a bridge call',
        () async {
      final bookmarks = BookmarksService();

      expect(await bookmarks.resolvePosts(const []), isEmpty);
      expect(api.callCount('crateFfiBookmarksBookmarksResolvePosts'), 0);
    });

    test('resolvePosts failure returns empty and records the error', () async {
      final bookmarks = BookmarksService();
      api.stub('crateFfiBookmarksBookmarksResolvePosts',
          (_) => throw Exception('resolve down'));

      final out = await bookmarks.resolvePosts(['ev-1']);

      expect(out, isEmpty);
      expect(bookmarks.lastError.toString(), contains('resolve down'));
    });
  });
}
