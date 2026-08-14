import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/test/helpers/test_env.dart';

import 'package:soshal_flutter/ffi/raster.dart';

void main() {
  test('raster wrappers call api and return ImpellerFrameBufferInfo', () async {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    final info = ImpellerFrameBufferInfo(width: 10, height: 10, stride: 40, pixelFormat: 'RGBA', bufferPtrAddr: BigInt.from(1));
    api.stub('crateFfiRasterRasterAllocateFrameBuffer', (_) => Future.value(info));
    api.stub('crateFfiRasterRasterSignalImpellerFrameReady', (_) => Future.value(true));

    final got = await rasterAllocateFrameBuffer(width: 10, height: 10);
    expect(got, info);

    final sig = await rasterSignalImpellerFrameReady(textureId: BigInt.from(1), frameTimestampNs: BigInt.from(1));
    expect(sig, true);

    expect(api.callCount('crateFfiRasterRasterAllocateFrameBuffer'), 1);
  });
}
