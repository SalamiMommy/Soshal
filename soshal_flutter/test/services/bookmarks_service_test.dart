// ignore_for_file: invalid_use_of_internal_member
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
  });
}
