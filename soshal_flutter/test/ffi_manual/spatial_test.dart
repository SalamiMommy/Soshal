import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/test/helpers/test_env.dart';

import 'package:soshal_flutter/ffi/spatial.dart';

void main() {
  test('spatialEncodeGeohash calls api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiSpatialSpatialEncodeGeohash', 'u4pruydqqvj');

    final gh = spatialEncodeGeohash(lat: 10.0, lon: 20.0);
    expect(gh, 'u4pruydqqvj');

    expect(api.callCount('crateFfiSpatialSpatialEncodeGeohash'), 1);
  });
}
