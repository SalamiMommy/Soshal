// Manual ffi tests for turso
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/turso.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-turso-manual');
  final api = env.$1;

  test('dbTursoConfigure calls crateFfiTursoDbTursoConfigure', () {
    api.stubString('crateFfiTursoDbTursoConfigure', 'ok');
    final res = dbTursoConfigure(url: 'https://db', authToken: 'tok');
    expect(res, 'ok');
    expect(api.callCount('crateFfiTursoDbTursoConfigure'), 1);
  });

  test('dbTursoSync calls crateFfiTursoDbTursoSync', () {
    api.stubString('crateFfiTursoDbTursoSync', 'sync');
    final res = dbTursoSync();
    expect(res, 'sync');
    expect(api.callCount('crateFfiTursoDbTursoSync'), 1);
  });

  test('dbTursoStatus calls crateFfiTursoDbTursoStatus', () {
    api.stubString('crateFfiTursoDbTursoStatus', 'status');
    final res = dbTursoStatus();
    expect(res, 'status');
    expect(api.callCount('crateFfiTursoDbTursoStatus'), 1);
  });
}
