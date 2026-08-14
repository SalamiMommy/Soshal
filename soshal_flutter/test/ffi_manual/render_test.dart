import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import '../helpers/test_env.dart';

import 'package:soshal_flutter/ffi/render.dart';

void main() {
  test('render wrappers call api and return expected shapes', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1;

    api.stubString('crateFfiRenderRenderCreateSession', '42');
    api.stub('crateFfiRenderRenderComputeMeshFrame', (_) => Uint8List.fromList([0, 1, 2]));

    final sid = renderCreateSession(width: 100, height: 200);
    expect(sid, '42');

    final frame = renderComputeMeshFrame(sessionId: 1, nodesJson: '[]', deltaTime: 0.016);
    expect(frame, isA<Uint8List>());

    expect(api.callCount('crateFfiRenderRenderCreateSession'), 1);
  });
}
