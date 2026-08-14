// Generated callable ffi tests for turso
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/turso.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-calls-turso');
  final api = env.$1;

  test('dbTursoSync calls crateFfiTursoDbTursoSync', () {
    api.stubString('crateFfiTursoDbTursoSync', 'stub');
    final res = dbTursoSync();
    expect(res, 'stub');
    expect(api.callCount('crateFfiTursoDbTursoSync'), 1);
  });

  test('dbTursoStatus calls crateFfiTursoDbTursoStatus', () {
    api.stubString('crateFfiTursoDbTursoStatus', 'stub');
    final res = dbTursoStatus();
    expect(res, 'stub');
    expect(api.callCount('crateFfiTursoDbTursoStatus'), 1);
  });

}
