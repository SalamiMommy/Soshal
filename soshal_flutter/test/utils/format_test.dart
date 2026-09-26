import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/utils/format.dart';

void main() {
  group('Format Utils - intl extensions', () {
    test('formatSats formats various amounts cleanly', () {
      expect(formatSats(0), '0 sats');
      expect(formatSats(-10), '0 sats');
      expect(formatSats(500), '500 sats');
      expect(formatSats(1500), '1,500 sats');
      expect(formatSats(99999), '99,999 sats');
      expect(formatSats(100000), '100K sats');
      expect(formatSats(1500000), '1.5M sats');
    });

    test('formatRelativeTime returns accurate relative buckets', () {
      final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;

      expect(formatRelativeTime(0), '');
      expect(formatRelativeTime(-10), '');
      expect(formatRelativeTime(now), 'just now');
      expect(formatRelativeTime(now - 30), 'just now');
      expect(formatRelativeTime(now - 300), '5m ago');
      expect(formatRelativeTime(now - 7200), '2h ago');
      expect(formatRelativeTime(now - 86400), 'yesterday');
      expect(formatRelativeTime(now - 86400 * 3), '3d ago');
    });
  });
}
