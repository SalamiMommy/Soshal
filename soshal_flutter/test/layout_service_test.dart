// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/feed_service.dart';
import 'package:soshal_flutter/services/layout_service.dart';

import 'package:soshal_flutter/test/helpers/test_env.dart';

late FakeApi api;

FeedPost post(String id) => FeedPost(
      eventId: id,
      pubkey: 'pk-$id',
      content: 'hello $id',
      createdAt: 1,
      reactions: 0,
      replies: 0,
      reposts: 0,
      liked: false,
    );

void main() {
  final env = bootstrapTestEnv('test-layout');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('LayoutService', () {
    test('refresh computes heights, stores media heights, notifies', () async {
      final layout = LayoutService();
      var notifications = 0;
      layout.addListener(() => notifications++);
      api.stubString(
        'crateFfiFeedFeedComputeCardLayouts',
        '[{"id":"a","height_px":120.5,"media_height_px":80.0},'
        '{"id":"b","height_px":200.0,"media_height_px":0.0}]',
      );

      await layout.refresh([post('a'), post('b')]);

      expect(layout.ready, isTrue);
      expect(layout.heights, {'a': 120.5, 'b': 200.0});
      expect(layout.heightFor(post('a')), 120.5);
      expect(layout.heightFor(post('b')), 200.0);
      expect(notifications, 1);
    });

    test('request payload carries text metrics, chrome and media list',
        () async {
      final layout = LayoutService();
      api.stubString('crateFfiFeedFeedComputeCardLayouts', '[]');
      await layout.refresh([post('x')], screenWidth: 600, textScale: 1.5);

      final inv = api.callsOf('crateFfiFeedFeedComputeCardLayouts').single;
      final req =
          (jsonDecode(api.namedArg(inv, 'requestsJson') as String) as List)[0]
              as Map<String, dynamic>;
      final text = req['text'] as Map<String, dynamic>;
      expect(req['id'], 'x');
      expect(text['font_size_px'], 14.0 * 1.5);
      expect(text['line_height_factor'], 1.4);
      expect(text['max_width_px'], 600 - 16);
      expect(req['media'], isEmpty);
      final chrome = req['chrome'] as Map<String, dynamic>;
      expect(chrome['header_px'], 48.0);
      expect(chrome['action_px'], 40.0);
      expect(chrome['max_media_height_px'], 480.0);
      expect(layout.ready, isTrue);
    });

    test('empty posts refresh green with no heights', () async {
      final layout = LayoutService();
      api.stubString('crateFfiFeedFeedComputeCardLayouts', '[]');
      await layout.refresh(const []);
      expect(layout.ready, isTrue);
      expect(layout.heights, isEmpty);
    });

    test('ffi error swallowed: ready stays false, heights stay empty',
        () async {
      final layout = LayoutService();
      api.stub('crateFfiFeedFeedComputeCardLayouts',
          (_) => throw Exception('boom'));
      await layout.refresh([post('a')]);
      expect(layout.ready, isFalse);
      expect(layout.heights, isEmpty);
      expect(api.callCount('crateFfiFeedFeedComputeCardLayouts'), 1);
    });

    test('malformed result entries skipped, null height maps to zero',
        () async {
      final layout = LayoutService();
      api.stubString(
        'crateFfiFeedFeedComputeCardLayouts',
        '[{"height_px": 10.0}, {"id":"b"}, {"id":"c","height_px":null}]',
      );
      await layout.refresh([post('a'), post('b'), post('c')]);
      expect(layout.heights, {'b': 0.0, 'c': 0.0});
    });

    test('extentFor maps index to post height, footer to null', () async {
      final layout = LayoutService();
      api.stubString(
        'crateFfiFeedFeedComputeCardLayouts',
        '[{"id":"a","height_px":100.0}]',
      );
      final posts = [post('a')];
      await layout.refresh(posts);
      expect(layout.extentFor(0, posts), 100.0);
      expect(layout.extentFor(1, posts), isNull,
          reason: 'index beyond posts maps to null');
      expect(layout.extentFor(0, [post('zzz')]), isNull,
          reason: 'no height for unknown id');
    });

    test('refresh params win over updateViewMetrics', () async {
      final layout = LayoutService();
      api.stubString('crateFfiFeedFeedComputeCardLayouts', '[]');
      layout.updateViewMetrics(screenWidth: 500, textScale: 2.0);
      // refresh re-applies its own defaults/args before building requests.
      await layout.refresh([post('a')]);

      final inv = api.callsOf('crateFfiFeedFeedComputeCardLayouts').single;
      final req =
          (jsonDecode(api.namedArg(inv, 'requestsJson') as String) as List)[0]
              as Map<String, dynamic>;
      expect((req['text'] as Map)['font_size_px'], 14.0);
      expect((req['text'] as Map)['max_width_px'], 360 - 16);

      await layout.refresh([post('a')], screenWidth: 500, textScale: 2.0);
      final second =
          (jsonDecode(api.namedArg(api.callsOf('crateFfiFeedFeedComputeCardLayouts').last,
                  'requestsJson') as String) as List)[0]
              as Map<String, dynamic>;
      expect((second['text'] as Map)['font_size_px'], 28.0);
      expect((second['text'] as Map)['max_width_px'], 484.0);
    });
  });
}