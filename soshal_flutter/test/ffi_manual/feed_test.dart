// Manual ffi tests for feed
import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/feed.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-feed-manual');
  final api = env.$1;

  test('feedAggregateChatReactions calls crateFfiFeedFeedAggregateChatReactions', () {
    api.stubString('crateFfiFeedFeedAggregateChatReactions', 'agg');
    final res = feedAggregateChatReactions(input: '{ }');
    expect(res, 'agg');
    expect(api.callCount('crateFfiFeedFeedAggregateChatReactions'), 1);
  });

  test('feedRankPosts returns Future and calls crateFfiFeedFeedRankPosts', () async {
    api.stub('crateFfiFeedFeedRankPosts', (_) => Future.value('ranked'));
    final res = await feedRankPosts(eventsJson: '[]');
    expect(res, 'ranked');
    expect(api.callCount('crateFfiFeedFeedRankPosts'), 1);
  });

  test('feedValidateNote calls crateFfiFeedFeedValidateNote', () {
    api.stubBool('crateFfiFeedFeedValidateNote', true);
    final ok = feedValidateNote(content: 'ok');
    expect(ok, true);
    expect(api.callCount('crateFfiFeedFeedValidateNote'), 1);
  });

  test('feedCompressEvent and feedDecompressEvent call respective methods', () async {
    api.stub('crateFfiFeedFeedCompressEvent', (_) => Future.value(Uint8List.fromList([1,2,3])));
    api.stub('crateFfiFeedFeedDecompressEvent', (_) => Future.value('decomp'));
    final compressed = await feedCompressEvent(eventJson: '{}');
    final decompressed = await feedDecompressEvent(compressed: [1,2,3]);
    expect(compressed, isA<Uint8List>());
    expect(decompressed, 'decomp');
    expect(api.callCount('crateFfiFeedFeedCompressEvent'), 1);
    expect(api.callCount('crateFfiFeedFeedDecompressEvent'), 1);
  });
}
