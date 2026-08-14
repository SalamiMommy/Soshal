// Manual ffi tests for bookmarks
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/bookmarks.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-bookmarks-manual');
  final api = env.$1;

  test('save/list/delete/resolve', () {
    api.stubString('crateFfiBookmarksBookmarksSave', 'evt1');
    api.stubString('crateFfiBookmarksBookmarksList', '[]');
    api.stubBool('crateFfiBookmarksBookmarksDelete', true);
    api.stubString('crateFfiBookmarksBookmarksResolvePost', '{}');

    final id = bookmarksSave(pubkey: 'p', eventId: 'e');
    final list = bookmarksList(pubkey: 'p', limit: 10, offset: 0);
    final deleted = bookmarksDelete(id: 'x');
    final post = bookmarksResolvePost(eventId: 'e');

    expect(id, 'evt1');
    expect(list, '[]');
    expect(deleted, true);
    expect(post, '{}');
    expect(api.callCount('crateFfiBookmarksBookmarksSave'), 1);
  });
}
