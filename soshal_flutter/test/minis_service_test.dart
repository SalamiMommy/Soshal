// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/minis_service.dart';

import 'package:soshal_flutter/test/helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-minis');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('MinisService', () {
    test('fetchMinis returns stub registry list verbatim', () {
      final minis = MinisService();
      api.stubListString(
          'crateFfiMinisMinisFetch', const ['/minis/feed-ranker.wasm']);

      final result = minis.fetchMinis();
      expect(result, ['/minis/feed-ranker.wasm']);
      expect(minis.lastError, isNull);
      expect(api.callCount('crateFfiMinisMinisFetch'), 1);
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

    test('errors set lastError and rethrow', () {
      final minis = MinisService();
      api.stub('crateFfiMinisMinisFetch',
          (_) => throw Exception('registry down'));
      expect(() => minis.fetchMinis(), throwsException);
      expect(minis.lastError, contains('registry down'));

      api.stub('crateFfiMinisMinisWasmExecuteFilter',
          (_) => throw Exception('wasm trap'));
      expect(
        () => minis.runFilter(
            pluginId: 'f', text: 't', wasmBytesHex: '00'),
        throwsException,
      );
      expect(minis.lastError, contains('wasm trap'));
    });
  });
}