// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/search_service.dart';

import './helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-search');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('SearchService', () {
    test('searchPosts parses post rows, updates results and notifies',
        () async {
      final search = SearchService();
      var notified = 0;
      search.addListener(() => notified++);
      api.stubString(
        'crateFfiSearchSearchPosts',
        '[{"event_id":"ev-1","pubkey":"pk-1","content":"hello world",'
        '"created_at":1700000001}]',
      );

      final results = await search.searchPosts('hello', limit: 25);
      expect(results.single.id, 'ev-1');
      expect(results.single.title, 'hello world');
      expect(results.single.kind, 'post');
      expect(results.single.pubkey, 'pk-1');
      expect(results.single.createdAt, 1700000001);
      expect(search.results.single.id, 'ev-1');
      expect(search.lastError, isNull);
      expect(notified, 1);

      final inv = api.callsOf('crateFfiSearchSearchPosts').single;
      expect(api.namedArg(inv, 'query'), 'hello');
      expect(api.namedArg(inv, 'limit'), 25);
    });

    test('searchProfiles parses profile rows with derived kind', () async {
      final search = SearchService();
      api.stubString(
        'crateFfiSearchSearchProfiles',
        '[{"pubkey":"pk-9","name":"alice","about":"builder",'
        '"created_at":1700000002}]',
      );

      final profiles = await search.searchProfiles('alice');
      expect(profiles.single.id, 'pk-9');
      expect(profiles.single.title, 'alice');
      expect(profiles.single.description, 'builder');
      expect(profiles.single.kind, 'profile');
      expect(search.results.single.kind, 'profile');

      final inv = api.callsOf('crateFfiSearchSearchProfiles').single;
      expect(api.namedArg(inv, 'query'), 'alice');
    });

    test('searchGlobal parses mixed rows and non-list resets results',
        () async {
      final search = SearchService();
      api.stubString(
        'crateFfiSearchSearchGlobal',
        '[{"id":"h-1","tag":"#rust","created_at":3}]',
      );

      final results = await search.searchGlobal('#rust');
      expect(results.single.id, 'h-1');
      expect(results.single.kind, 'hashtag');
      expect(search.results.single.kind, 'hashtag');
      expect(api.namedArg(
          api.callsOf('crateFfiSearchSearchGlobal').single, 'query'), '#rust');

      api.stubString('crateFfiSearchSearchGlobal', '{"not":"a list"}');
      await search.searchGlobal('#rust');
      expect(search.results, isEmpty);
    });

    test('hashtags and trending state update and notify', () async {
      final search = SearchService();
      var notified = 0;
      search.addListener(() => notified++);
      api.stubListString(
          'crateFfiSearchSearchHashtags', const ['#rust', '#flutter']);
      api.stubListString(
          'crateFfiSearchSearchTrendingHashtags', const ['#soshal']);
      api.stubString(
        'crateFfiSearchSearchTrendingProfiles',
        '[{"pubkey":"pk-2","name":"bob","about":"","created_at":4}]',
      );

      final tags = await search.searchHashtags('rust');
      expect(tags, ['#rust', '#flutter']);
      expect(search.hashtags, ['#rust', '#flutter']);
      expect(search.trendingHashtagsList, isEmpty);
      final tagInv = api.callsOf('crateFfiSearchSearchHashtags').single;
      expect(api.namedArg(tagInv, 'query'), 'rust');

      final trending = await search.trendingHashtags();
      expect(trending, ['#soshal']);
      expect(search.trendingHashtagsList, ['#soshal']);

      final profiles = await search.trendingProfiles();
      expect(profiles.single.title, 'bob');
      expect(search.trendingProfilesList.single.kind, 'profile');
      expect(notified, 3);
    });

    test('error sets lastError and rethrows', () async {
      final search = SearchService();
      api.stub('crateFfiSearchSearchPosts',
          (_) => throw Exception('fts down'));
      await expectLater(search.searchPosts('x'), throwsException);
      expect(search.lastError, contains('fts down'));

      api.stub('crateFfiSearchSearchHashtags',
          (_) => throw Exception('tags down'));
      await expectLater(search.searchHashtags('x'), throwsException);
      expect(search.lastError, contains('tags down'));
    });
  });
}
