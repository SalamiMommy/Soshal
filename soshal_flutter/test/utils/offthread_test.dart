import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/utils/offthread.dart';

/// Wave 6 item 6.8: the payload-size threshold in `runOffThreadCompute`.
///
/// The wiring itself is not observable in a `flutter test` run — `_isTest`
/// short-circuits to inline *before* the threshold is consulted, so any test of
/// `runOffThreadCompute` would see the inline path either way. Deleting the
/// `shouldParseInline(payload)` call from `runOffThreadCompute` entirely is a
/// mutant these tests cannot catch, and that is not an oversight to be patched
/// over with a test that asserts nothing.
///
/// The reason is structural: inline and off-thread are *not distinguishable in
/// the return value*. Every parser passed to `runOffThreadCompute` is a
/// top-level pure function, so the two paths differ only in timing — and timing
/// is exactly what a unit test must not assert. Catching it would need a real
/// isolate benchmark, which is flaky in CI by construction. So the threshold
/// decision is treated as the whole behavioral surface and is tested directly;
/// the single call that consults it is a review obligation.
void main() {
  group('estimatePayloadSize', () {
    test('measures a string in code units', () {
      expect(estimatePayloadSize(''), 0);
      expect(estimatePayloadSize('abc'), 3);
      expect(estimatePayloadSize('x' * 5000), 5000);
    });

    test('measures a list by element count', () {
      expect(estimatePayloadSize(<Object?>[]), 0);
      expect(estimatePayloadSize([1, 2, 3]), 3);
    });

    test('returns null for anything it cannot cheaply measure', () {
      // A map's byte size would need a full walk, so it is reported as unknown
      // and keeps the off-thread path. `sync_service.dart` passes one to encode.
      expect(estimatePayloadSize({'a': 1}), isNull);
      expect(estimatePayloadSize(42), isNull);
      expect(estimatePayloadSize(null), isNull);
    });
  });

  group('shouldParseInline', () {
    test('the mention path parses inline', () {
      // The case 6.8 exists for: `SearchService.mentions(query, limit: 8)` on
      // every debounce tick, carrying at most 8 `{pubkey, name}` rows. Spawning
      // an isolate to decode a few hundred bytes costs far more than the parse.
      final json =
          '[${List.generate(8, (i) => '{"pubkey":"pk$i","name":"User $i"}').join(',')}]';
      expect(json.length, lessThan(kInlineParseBytes));
      expect(shouldParseInline(json), isTrue);
    });

    test('an empty string is inline, not a special case', () {
      expect(shouldParseInline(''), isTrue);
    });

    test('the boundary is exclusive at the cutoff', () {
      // Below the cutoff runs inline; exactly at it does not. A one-character
      // difference here is the difference between the two paths.
      final atCutoff = 'x' * kInlineParseBytes;
      expect(estimatePayloadSize(atCutoff), kInlineParseBytes);
      expect(shouldParseInline(atCutoff), isFalse);
      expect(shouldParseInline('x' * (kInlineParseBytes - 1)), isTrue);
    });

    test('a large payload stays off-thread', () {
      expect(shouldParseInline('x' * (kInlineParseBytes * 4)), isFalse);
    });

    test('a large list stays off-thread and a small one does not', () {
      expect(shouldParseInline(List<Object?>.filled(kInlineParseBytes, 0)),
          isFalse);
      expect(shouldParseInline(List<Object?>.filled(kInlineParseBytes - 1, 0)),
          isTrue);
    });

    test('an unmeasurable payload stays off-thread', () {
      // "unknown" must never resolve to "cheap" — guessing small here would
      // silently move a map encode back onto the UI isolate.
      expect(shouldParseInline({'a': 1}), isFalse);
      expect(shouldParseInline(42), isFalse);
      expect(shouldParseInline(null), isFalse);
    });

    test('the cutoff is 8 KiB', () {
      // Pinned because the whole optimization is a trade against the isolate
      // spawn cost. Moving it is a deliberate retune, not an incidental edit.
      expect(kInlineParseBytes, 8 * 1024);
    });
  });
}
