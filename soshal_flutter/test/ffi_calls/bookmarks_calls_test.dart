// Generated callable ffi tests for bookmarks
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/bookmarks.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-bookmarks');
  final api = env.$1;

  test('bookmarksResolvePost calls String bookmarksResolvePost({required String eventId}) => RustLib.instance.api', () {
    api.stubString('String bookmarksResolvePost({required String eventId}) => RustLib.instance.api', 'stub');
    final res = bookmarksResolvePost(eventId}: "x");
    expect(res, 'stub');
    expect(api.callCount('String bookmarksResolvePost({required String eventId}) => RustLib.instance.api'), 1);
  });

}
