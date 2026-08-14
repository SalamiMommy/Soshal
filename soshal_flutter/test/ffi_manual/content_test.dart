// Manual ffi tests for content
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/content.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-content-manual');
  final api = env.$1;

  test('contentExtractHashtags calls crateFfiContentContentExtractHashtags', () {
    api.stubListString('crateFfiContentContentExtractHashtags', ['tag']);
    final res = contentExtractHashtags(text: 'hello #tag');
    expect(res, ['tag']);
    expect(api.callCount('crateFfiContentContentExtractHashtags'), 1);
  });

  test('contentCompressJsonDict calls crateFfiContentContentCompressJsonDict', () {
    api.stubString('crateFfiContentContentCompressJsonDict', 'compressed');
    final res = contentCompressJsonDict(data: '{"a":1}');
    expect(res, 'compressed');
    expect(api.callCount('crateFfiContentContentCompressJsonDict'), 1);
  });

  test('contentDecompressJsonDict calls crateFfiContentContentDecompressJsonDict', () {
    api.stubString('crateFfiContentContentDecompressJsonDict', 'decompressed');
    final res = contentDecompressJsonDict(encoded: 'zzz');
    expect(res, 'decompressed');
    expect(api.callCount('crateFfiContentContentDecompressJsonDict'), 1);
  });
}
