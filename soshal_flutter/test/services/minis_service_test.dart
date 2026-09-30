// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/minis_service.dart';

import '../helpers/test_env.dart';

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
        () => minis.runFilter(pluginId: 'f', text: 't', wasmBytesHex: '00'),
        throwsStateError,
      );
      expect(minis.lastError, contains('wasm trap'));
      expect(minis.wasmRuntimeUnavailable, isTrue);

      api.stub('crateFfiMinisMinisWasmRankFeed',
          (_) => throw Exception('ranker down'));
      expect(
        () => minis.rankFeed(
          pluginId: 'f',
          postsJson: const ['{"id":"a"}'],
          wasmBytesHex: '00',
        ),
        throwsStateError,
      );
      expect(minis.lastError, contains('ranker down'));
      expect(minis.wasmRuntimeUnavailable, isTrue);
    });

    test('wasmRuntimeUnavailable clears on successful plugin run', () {
      final minis = MinisService();
      api.stub('crateFfiMinisMinisWasmExecuteFilter',
          (_) => throw Exception('trap'));
      expect(
        () => minis.runFilter(pluginId: 'f', text: 't', wasmBytesHex: '00'),
        throwsStateError,
      );
      expect(minis.wasmRuntimeUnavailable, isTrue);

      api.stubString('crateFfiMinisMinisWasmExecuteFilter', 'CLEAN');
      expect(
        minis.runFilter(pluginId: 'f', text: 't', wasmBytesHex: '00'),
        'CLEAN',
      );
      expect(minis.wasmRuntimeUnavailable, isFalse);
      expect(minis.lastError, isNull);
    });

    test('watchMinis and subscribeToMinis stream updates live from bridge', () async {
      final minis = MinisService();
      final controller = StreamController<String>.broadcast();
      addTearDown(controller.close);

      api.stub('crateFfiMinisMinisWatch', (_) => controller.stream);

      final emissions = <List<MiniItem>>[];
      final sub = minis.watchMinis().listen(emissions.add);
      addTearDown(sub.cancel);

      minis.subscribeToMinis();
      expect(minis.minisLoading, isTrue);

      const miniPayload =
          '[{"id":"m-live","pubkey":"pk-1","videoUrl":"blob://live",'
          '"blobHash":"livehash","mediaSize":1024,"textOverlay":"live mini",'
          '"thumbnail":"","audience":"public","createdAt":1700000000}]';

      controller.add(miniPayload);
      await pumpEventQueue();

      expect(emissions.length, 1);
      expect(emissions[0].first.id, 'm-live');
      expect(minis.minis.length, 1);
      expect(minis.minis.first.textOverlay, 'live mini');
      expect(minis.minisLoading, isFalse);

      minis.resetForAccountSwitch();
      controller.add('[]');
      await pumpEventQueue();
      expect(minis.minis.isEmpty, isTrue);
    });

    test('watchMinis forwards source and parse errors to subscribers', () async {
      final minis = MinisService();
      final controller = StreamController<String>.broadcast();
      addTearDown(controller.close);
      api.stub('crateFfiMinisMinisWatch', (_) => controller.stream);

      final errors = <Object>[];
      final sub = minis.watchMinis().listen(
        (_) {},
        onError: errors.add,
      );
      addTearDown(sub.cancel);

      // Source-side error.
      controller.addError(Exception('bridge down'));
      await pumpEventQueue();
      expect(errors, hasLength(1));

      // Mapper-side error: the parse runs behind `asyncMap`, so a malformed
      // payload must surface rather than being swallowed.
      controller.add('not json');
      await pumpEventQueue();
      expect(errors.length, 2);
    });

    test('fetchSavedMinis decodes the saved list into state', () async {
      final minis = MinisService();
      api.stubString(
        'crateFfiMinisMinisSaved',
        '[{"id":"m-saved","pubkey":"pk-1","videoUrl":"blob://saved",'
            '"blobHash":"h","mediaSize":7,"textOverlay":"kept",'
            '"thumbnail":"","audience":"public","createdAt":1700000000}]',
      );

      final out = await minis.fetchSavedMinis();

      expect(out.single.id, 'm-saved');
      expect(out.single.textOverlay, 'kept');
      expect(minis.savedMinis.single.id, 'm-saved');
      expect(minis.lastError, isNull);
    });

    test('fetchSavedMinis failure records the error', () async {
      final minis = MinisService();
      api.stub('crateFfiMinisMinisSaved', (_) => throw Exception('saved down'));

      await expectLater(minis.fetchSavedMinis(), throwsException);
      expect(minis.lastError.toString(), contains('saved down'));
    });

    // ─── the saved-id mirror (6.4) ─────────────────────────────────────────
    //
    // `isSaved` used to be `_saved.any((m) => m.id == id)`, called once per row
    // by the ForYou grid, so every build of the grid cost O(rows x saved). It is
    // now a lookup in a `Set` that has to be kept in step with `_saved` at four
    // separate mutation sites, and these pin each one.

    test('the saved-id mirror tracks every mutation of the saved list',
        () async {
      final minis = MinisService();
      expect(minis.isSaved('m-1'), isFalse);

      api.stubString('crateFfiMinisMinisSaved', _savedJson(['m-1', 'm-2']));
      await minis.fetchSavedMinis();
      expect(minis.isSaved('m-1'), isTrue);
      expect(minis.isSaved('m-2'), isTrue);
      expect(minis.isSaved('m-3'), isFalse);

      // Re-fetching with a different set must replace the mirror, not add to it.
      api.stubString('crateFfiMinisMinisSaved', _savedJson(['m-2', 'm-3']));
      await minis.fetchSavedMinis();
      expect(minis.isSaved('m-1'), isFalse,
          reason: 'm-1 is no longer saved, so the mirror must drop it');
      expect(minis.isSaved('m-3'), isTrue);

      api.stubString('crateFfiMinisMinisSaved', '[]');
      await minis.fetchSavedMinis();
      expect(minis.isSaved('m-2'), isFalse);
      expect(minis.isSaved('m-3'), isFalse);
    });

    test('unsaveMini drops the id from the mirror', () async {
      final minis = MinisService();
      api.stubString('crateFfiMinisMinisSaved', _savedJson(['m-1', 'm-2']));
      await minis.fetchSavedMinis();
      expect(minis.isSaved('m-1'), isTrue);

      api.stub('crateFfiMinisMinisUnsave', (_) => true);
      await minis.unsaveMini('m-1');

      expect(minis.isSaved('m-1'), isFalse);
      expect(minis.savedMinis.map((m) => m.id), isNot(contains('m-1')));
      // The untouched sibling must survive: this is the case a mirror that was
      // cleared rather than pruned on removal would get wrong.
      expect(minis.isSaved('m-2'), isTrue);
      expect(minis.savedMinis.single.id, 'm-2');
    });

    test('unsaving an id that is not saved leaves the mirror alone', () async {
      final minis = MinisService();
      api.stubString('crateFfiMinisMinisSaved', _savedJson(['m-1']));
      await minis.fetchSavedMinis();

      api.stub('crateFfiMinisMinisUnsave', (_) => true);
      await minis.unsaveMini('m-absent');

      expect(minis.isSaved('m-1'), isTrue,
          reason: 'removing something absent must not empty the mirror');
      expect(minis.savedMinis.single.id, 'm-1');
    });

    test('resetForAccountSwitch clears the mirror as well as the list',
        () async {
      final minis = MinisService();
      api.stubString('crateFfiMinisMinisSaved', _savedJson(['m-1', 'm-2']));
      await minis.fetchSavedMinis();
      expect(minis.isSaved('m-1'), isTrue);

      minis.resetForAccountSwitch();

      expect(minis.isSaved('m-1'), isFalse);
      expect(minis.isSaved('m-2'), isFalse);
      expect(minis.savedMinis, isEmpty);
    });

    test('the mirror agrees with the list for every id, by construction',
        () async {
      // The mirror is a cache over a list that is also public, so the invariant
      // that matters is "the two never disagree". Walk a sequence of mutations
      // and check both views after each step rather than spot-checking ids.
      final minis = MinisService();
      api.stubString('crateFfiMinisMinisSaved', _savedJson(['a', 'b', 'c']));
      api.stub('crateFfiMinisMinisUnsave', (_) => true);

      Future<void> expectConsistent(String label) async {
        final fromList = minis.savedMinis.map((m) => m.id).toSet();
        for (final id in ['a', 'b', 'c', 'd', 'absent']) {
          expect(minis.isSaved(id), fromList.contains(id),
              reason: 'isSaved($id) disagrees with the list after $label');
        }
      }

      await minis.fetchSavedMinis();
      await expectConsistent('fetch');
      await minis.unsaveMini('b');
      await expectConsistent('unsave b');
      await minis.unsaveMini('absent');
      await expectConsistent('unsave absent');
      await minis.unsaveMini('a');
      await expectConsistent('unsave a');
      minis.resetForAccountSwitch();
      await expectConsistent('account switch');
    });
  });
}

/// A `crateFfiMinisMinisSaved` payload for the given ids, in the shape
/// `parseMinis` expects. `MiniItem.id` is what the mirror keys on, so the ids
/// are the only part that matters here.
String _savedJson(List<String> ids) {
  return '[${ids.map((id) => '{"id":"$id","pubkey":"pk-1",'
          '"videoUrl":"blob://x","blobHash":"h","mediaSize":1,'
          '"textOverlay":"","thumbnail":"","audience":"public",'
          '"createdAt":1700000000}')
      .join(',')}]';
}