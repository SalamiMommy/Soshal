// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/turso_service.dart';

import 'package:soshal_flutter/test/helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-turso');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('TursoService', () {
    test('initial state is correct', () {
      final turso = TursoService();

      expect(turso.isConfigured, false);
      expect(turso.status, 'idle');
      expect(turso.lastSyncedAt, isNull);
      expect(turso.url, '');
      expect(turso.isSyncing, false);
      expect(turso.lastError, isNull);
    });

    test('checkStatus silently handles parse errors', () async {
      api.stub('crateFfiTursoDbTursoStatus', (_) {
        throw Exception('Status error');
      });

      final turso = TursoService();
      final notifySpy = <void>[];
      turso.addListener(() => notifySpy.add(null));

      // Should not throw
      await turso.checkStatus();
      expect(notifySpy.isEmpty, true);
    });
  });
}
