// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/audit_service.dart';

import './helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-audit');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('AuditService', () {
    test('list parses rows, forwards filters, notifies', () async {
      final audit = AuditService();
      var notifications = 0;
      audit.addListener(() => notifications++);
      api.stubString(
        'crateFfiAuditAuditList',
        '[{"id":"a1","group_id":"g1","actor_pubkey":"pk-1",'
        '"action":"group.kick","target_pubkey":"pk-2","details":"bye",'
        '"created_at":1700000001},'
        '{"id":"a2","actor_pubkey":"pk-1","action":"login",'
        '"created_at":1700000000}]',
      );

      final rows = await audit.list(limit: 50, actor: 'pk-1');

      expect(rows.length, 2);
      expect(rows.first.id, 'a1');
      expect(rows.first.groupId, 'g1');
      expect(rows.first.actorPubkey, 'pk-1');
      expect(rows.first.targetPubkey, 'pk-2');
      expect(rows.first.details, 'bye');
      expect(rows.first.createdAt, 1700000001);
      expect(rows.last.groupId, isNull);
      expect(rows.last.createdAt, 1700000000);
      expect(audit.rows, same(rows));
      expect(audit.lastError, isNull);
      expect(notifications, 1);

      final inv = api.callsOf('crateFfiAuditAuditList').single;
      expect(api.namedArg(inv, 'limit'), 50);
      expect(api.namedArg(inv, 'actorPubkey'), 'pk-1');
    });

    test('empty list yields empty rows, default limit sent', () async {
      final audit = AuditService();
      api.stubString('crateFfiAuditAuditList', '[]');
      expect(await audit.list(), isEmpty);
      expect(audit.rows, isEmpty);
      expect(audit.lastError, isNull);

      final inv = api.callsOf('crateFfiAuditAuditList').single;
      expect(api.namedArg(inv, 'limit'), 100);
      expect(api.namedArg(inv, 'actorPubkey'), isNull);
    });

    test('non-list payload yields empty rows without error', () async {
      final audit = AuditService();
      api.stubString('crateFfiAuditAuditList', '{"error":"x"}');
      expect(await audit.list(), isEmpty);
      expect(audit.rows, isEmpty);
      expect(audit.lastError, isNull);
    });

    test('ffi error rethrows and records lastError', () async {
      final audit = AuditService();
      api.stub('crateFfiAuditAuditList', (_) => throw Exception('db locked'));
      await expectLater(audit.list(), throwsA(anything));
      expect(audit.lastError, contains('db locked'));
    });

    test('prev rows kept when a later list fails', () async {
      final audit = AuditService();
      api.stubString('crateFfiAuditAuditList', '[{"id":"a1"}]');
      await audit.list();
      api.stub('crateFfiAuditAuditList', (_) => throw Exception('boom'));
      await expectLater(audit.list(), throwsA(anything));
      expect(audit.rows.single.id, 'a1');
    });

    test('row parses cleanly and clears lastError on success', () async {
      final audit = AuditService();
      api.stub('crateFfiAuditAuditList', (_) => throw Exception('x'));
      await expectLater(audit.list(), throwsA(anything));
      api.stubString('crateFfiAuditAuditList', '[]');
      await audit.list();
      expect(audit.lastError, isNull);
    });

    test('fromJson fills defaults for sparse payload', () {
      final row = AuditRow.fromJson(const {});
      expect(row.id, '');
      expect(row.groupId, isNull);
      expect(row.actorPubkey, '');
      expect(row.action, '');
      expect(row.targetPubkey, isNull);
      expect(row.details, isNull);
      expect(row.createdAt, 0);

      final full = AuditRow.fromJson({
        'id': 'z',
        'group_id': 'g',
        'actor_pubkey': 'pk',
        'action': 'act',
        'target_pubkey': 'tp',
        'details': 'd',
        'created_at': 42,
      });
      expect(full.id, 'z');
      expect(full.groupId, 'g');
      expect(full.createdAt, 42);
    });
  });
}