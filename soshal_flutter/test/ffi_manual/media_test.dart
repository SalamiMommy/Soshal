// Manual ffi tests for media
import 'dart:typed_data';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/media.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-media-manual');
  final api = env.$1;

  test('mediaDecodeImageRgba and upload/fetch/load/mime/clear/cache', () async {
    api.stub('crateFfiMediaMediaDecodeImageRgba', (_) => Future.value(DecodedImageRgbaDto(width: 1, height: 1, pixels: Uint8List.fromList([1]))));
    api.stub('crateFfiMediaMediaUpload', (_) => Future.value('id'));
    api.stub('crateFfiMediaMediaFetch', (_) => Future.value('/tmp/x'));
    api.stub('crateFfiMediaMediaLoadLocal', (_) => Future.value(Uint8List.fromList([1,2,3])));
    api.stub('crateFfiMediaMediaGetMimeType', (_) => Future.value('image/png'));
    api.stub('crateFfiMediaMediaClearCache', (_) => Future.value('cleared'));
    api.stubString('crateFfiMediaMediaUploadBlob', 'blobid');

    final dec = await mediaDecodeImageRgba(filePathOrUrl: 'x');
    final up = await mediaUpload(filePath: '/tmp/f', blossomServer: 'b');
    final fetched = await mediaFetch(url: 'u', cacheDir: '/tmp');
    final loaded = await mediaLoadLocal(filePath: '/tmp/f');
    final mime = await mediaGetMimeType(filePath: '/tmp/f');
    final cleared = await mediaClearCache(cacheDir: '/tmp');
    final blob = mediaUploadBlob(data: [1,2,3]);

    expect(dec.width, 1);
    expect(up, 'id');
    expect(fetched, '/tmp/x');
    expect(loaded, isA<Uint8List>());
    expect(mime, 'image/png');
    expect(cleared, 'cleared');
    expect(blob, 'blobid');
    expect(api.callCount('crateFfiMediaMediaDecodeImageRgba'), 1);
  });
