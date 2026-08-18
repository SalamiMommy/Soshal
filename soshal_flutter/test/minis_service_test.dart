// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/minis_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-minis');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('MinisService', () {
    test('fetchMinis parses registry JSON into MiniItems', () {
      final minis = MinisService();
      api.stubString(
        'crateFfiMinisMinisFetch',
        '[{"id":"m-1","pubkey":"pk-1","videoUrl":"blob://'
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",'
        '"blobHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",'
        '"mediaSize":2048,"textOverlay":"first","thumbnail":"","audience":"public",'
        '"createdAt":1700000001}]',
      );

      final result = minis.fetchMinis();
      expect(result, hasLength(1));
      final item = result.single;
      expect(item.id, 'm-1');
      expect(item.pubkey, 'pk-1');
      expect(item.blobHash,
          'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');
      expect(item.mediaSize, 2048);
      expect(item.textOverlay, 'first');
      expect(item.videoUrl, startsWith('blob://'));
      expect(minis.lastError, isNull);
      expect(api.callCount('crateFfiMinisMinisFetch'), 1);
    });

    test('publishMini forwards mediaSource and returns id', () async {
      final minis = MinisService();
      api.stub('crateFfiMinisMinisPublish', (_) async => 'ev-mini-1');

      final id = await minis.publishMini(
        mediaSource: '/tmp/clip.mp4',
        textOverlay: 'hello',
      );
      expect(id, 'ev-mini-1');
      final inv = api.callsOf('crateFfiMinisMinisPublish').single;
      expect(api.namedArg(inv, 'mediaSource'), '/tmp/clip.mp4');
      expect(api.namedArg(inv, 'textOverlay'), 'hello');
    });

    test('runFilter passes pluginId/text/wasmBytesHex, returns result', () {
      final minis = MinisService();
      api.stubStringBuilder('crateFfiMinisMinisWasmExecuteFilter', (inv) {
        expect(api.namedArg(inv, 'pluginId'), 'filter-v1');
        expect(api.namedArg(inv, 'text'), 'hello');
        expect(api.namedArg(inv, 'wasmBytesHex'), 'deadbeef');
        return 'CLEAN';
      });

      final result = minis.runFilter(
        pluginId: 'filter-v1',
        text: 'hello',
        wasmBytesHex: 'deadbeef',
      );
      expect(result, 'CLEAN');
      expect(minis.lastError, isNull);
    });

    test('rankFeed passes postsJson list, returns ranked list', () {
      final minis = MinisService();
      api.stubListString(
        'crateFfiMinisMinisWasmRankFeed',
        const ['{"id":"b"}', '{"id":"a"}'],
      );

      final ranked = minis.rankFeed(
        pluginId: 'ranker-1',
        postsJson: const ['{"id":"a"}', '{"id":"b"}'],
        wasmBytesHex: 'abcd',
      );
      expect(ranked, ['{"id":"b"}', '{"id":"a"}']);
      final inv = api.callsOf('crateFfiMinisMinisWasmRankFeed').single;
      expect(api.namedArg(inv, 'pluginId'), 'ranker-1');
      expect(api.namedArg(inv, 'postsJson'), ['{"id":"a"}', '{"id":"b"}']);
      expect(api.namedArg(inv, 'wasmBytesHex'), 'abcd');
      expect(minis.lastError, isNull);
    });

    test('errors set lastError; fetchMinis rethrows, plugins degrade', () {
      final minis = MinisService();
      api.stub('crateFfiMinisMinisFetch',
          (_) => throw Exception('registry down'));
      expect(() => minis.fetchMinis(), throwsException);
      expect(minis.lastError, contains('registry down'));

      api.stub('crateFfiMinisMinisWasmExecuteFilter',
          (_) => throw Exception('wasm trap'));
      expect(
        minis.runFilter(pluginId: 'f', text: 't', wasmBytesHex: '00'),
        '',
      );
      expect(minis.lastError, contains('wasm trap'));
      expect(minis.wasmRuntimeUnavailable, isTrue);

      api.stub('crateFfiMinisMinisWasmRankFeed',
          (_) => throw Exception('ranker down'));
      expect(
        minis.rankFeed(
          pluginId: 'f',
          postsJson: const ['{"id":"a"}'],
          wasmBytesHex: '00',
        ),
        isEmpty,
      );
      expect(minis.lastError, contains('ranker down'));
      expect(minis.wasmRuntimeUnavailable, isTrue);
    });

    test('wasmRuntimeUnavailable clears on successful plugin run', () {
      final minis = MinisService();
      api.stub('crateFfiMinisMinisWasmExecuteFilter',
          (_) => throw Exception('trap'));
      minis.runFilter(pluginId: 'f', text: 't', wasmBytesHex: '00');
      expect(minis.wasmRuntimeUnavailable, isTrue);

      api.stubString('crateFfiMinisMinisWasmExecuteFilter', 'CLEAN');
      expect(
        minis.runFilter(pluginId: 'f', text: 't', wasmBytesHex: '00'),
        'CLEAN',
      );
      expect(minis.wasmRuntimeUnavailable, isFalse);
      expect(minis.lastError, isNull);
    });
  });
}