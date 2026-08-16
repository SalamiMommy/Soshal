import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/social_entry.dart';

void main() {
  group('SocialEntry', () {
    test('constructor stores all fields', () {
      final entry = SocialEntry(
        id: 'ev-1',
        pubkey: 'pk-1',
        content: 'hello',
        createdAt: 1700000001,
      );

      expect(entry.id, 'ev-1');
      expect(entry.pubkey, 'pk-1');
      expect(entry.content, 'hello');
      expect(entry.createdAt, 1700000001);
    });

    test('fromJson parses full map', () {
      final entry = SocialEntry.fromJson({
        'id': 'ev-2',
        'pubkey': 'pk-2',
        'content': 'world',
        'created_at': 1700000002,
      });

      expect(entry.id, 'ev-2');
      expect(entry.pubkey, 'pk-2');
      expect(entry.content, 'world');
      expect(entry.createdAt, 1700000002);
    });

    test('fromJson fills defaults for empty map', () {
      final entry = SocialEntry.fromJson(const {});

      expect(entry.id, '');
      expect(entry.pubkey, '');
      expect(entry.content, '');
      expect(entry.createdAt, 0);
    });

    test('fromJson treats null values as defaults', () {
      final entry = SocialEntry.fromJson({
        'id': null,
        'pubkey': null,
        'content': null,
        'created_at': null,
      });

      expect(entry.id, '');
      expect(entry.pubkey, '');
      expect(entry.content, '');
      expect(entry.createdAt, 0);
    });

    test('fromJson truncates fractional created_at to int', () {
      final entry =
          SocialEntry.fromJson({'created_at': 1700000002.75});

      expect(entry.createdAt, 1700000002);
      expect(entry.id, '');
    });

    test('fromJson ignores unknown keys', () {
      final entry = SocialEntry.fromJson({
        'id': 'ev-3',
        'extra': 'ignored',
        'likes': 99,
      });

      expect(entry.id, 'ev-3');
      expect(entry.pubkey, '');
      expect(entry.content, '');
      expect(entry.createdAt, 0);
    });
  });
}