// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/p2p.dart';
import 'package:soshal_flutter/services/media_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-media');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('MediaService', () {
    test('uploadMedia stubs FFI, parses manifest, passes filePath',
        () async {
      final media = MediaService();
      api.stubString(
        'crateFfiMediaMediaUploadBlobFile',
        '{"hash":"h-abc","size":42,"chunks":["c1","c2"]}',
      );
      final dir = await Directory.systemTemp.createTemp('media_upload');
      addTearDown(() => dir.delete(recursive: true));
      final file = File('${dir.path}/clip.mp4')..writeAsStringSync('data');

      final manifest = await media.uploadMedia(file.path);
      expect(manifest['hash'], 'h-abc');
      expect(manifest['size'], 42);
      expect((manifest['chunks'] as List).length, 2);
      expect(media.lastError, isNull);
      final inv =
          api.callsOf('crateFfiMediaMediaUploadBlobFile').single;
      expect(api.namedArg(inv, 'filePath'), file.path);
    });

    test('uploadMedia rejects missing file before FFI', () async {
      final media = MediaService();
      await expectLater(
        media.uploadMedia('/nonexistent/missing.mp4'),
        throwsException,
      );
      expect(media.lastError, contains('File not found'));
      expect(api.callCount('crateFfiMediaMediaUploadBlobFile'), 0);
    });

    test('fetchBlob resolves outPath and passes hash', () async {
      final media = MediaService();
      api.stubString(
        'crateFfiMediaMediaFetchBlob',
        '{"success":true,"error":null}',
      );
      final path = await media.fetchBlob('h-abc', outPath: '/tmp/out.bin');
      expect(path, '/tmp/out.bin');
      expect(media.lastError, isNull);
      final inv = api.callsOf('crateFfiMediaMediaFetchBlob').single;
      expect(api.namedArg(inv, 'blobHash'), 'h-abc');
      expect(api.namedArg(inv, 'outPath'), '/tmp/out.bin');
    });

    test('fetchBlob defaults outPath from cache path', () async {
      final media = MediaService();
      api.stubString('crateFfiMediaMediaGetCachePath', '/tmp/cache');
      api.stubString(
        'crateFfiMediaMediaFetchBlob',
        '{"success":true,"error":null}',
      );
      final path = await media.fetchBlob('h-abc');
      expect(path, '/tmp/cache/h-abc');
      final inv = api.callsOf('crateFfiMediaMediaFetchBlob').single;
      expect(api.namedArg(inv, 'outPath'), '/tmp/cache/h-abc');
    });

    test('fetchBlob surfaces failure result as error', () async {
      final media = MediaService();
      api.stubString(
        'crateFfiMediaMediaFetchBlob',
        '{"success":false,"error":"chunk missing"}',
      );
      await expectLater(
        media.fetchBlob('h-abc', outPath: '/tmp/out.bin'),
        throwsException,
      );
      expect(media.lastError, contains('chunk missing'));
    });

    test('local server start/stop and URL building', () async {
      final media = MediaService();
      expect(() => media.getLocalUrl('h-abc'), throwsException,
          reason: 'server not started yet');
      expect(media.lastError, contains('Local server not started'));

      api.stubInt('crateFfiMediaMediaStartLocalServer', 8765);
      final port = await media.startLocalServer();
      expect(port, 8765);
      expect(media.localServerPort, 8765);
      expect(media.getLocalUrl('h-abc'),
          'http://127.0.0.1:8765/blob/h-abc');

      api.stub('crateFfiMediaMediaStopLocalServer', (_) => null);
      await media.stopLocalServer();
      expect(media.localServerPort, isNull);
      expect(media.lastError, isNull);
    });

    test('clearCache passes resolved cache dir', () async {
      final media = MediaService();
      api.stubString('crateFfiMediaMediaGetCachePath', '/tmp/cache');
      api.stub('crateFfiMediaMediaClearCache', (_) => null);
      await media.clearCache();
      final inv = api.callsOf('crateFfiMediaMediaClearCache').single;
      expect(api.namedArg(inv, 'cacheDir'), '/tmp/cache');
      expect(media.lastError, isNull);
    });

    test('FFI throw sets lastError and rethrows', () async {
      final media = MediaService();
      api.stub('crateFfiMediaMediaUploadBlobFile',
          (_) => throw Exception('store full'));
      final dir = await Directory.systemTemp.createTemp('media_err');
      addTearDown(() => dir.delete(recursive: true));
      final file = File('${dir.path}/clip.mp4')..writeAsStringSync('data');

      await expectLater(
        media.uploadMedia(file.path),
        throwsException,
      );
      expect(media.lastError, contains('store full'));
    });

    test('fetchBlobFromLan swarms fallback and surfaces all-fail',
        () async {
      final media = MediaService();
      api.stubStringBuilder(
        'crateFfiP2PP2PFetchBlobFromPeer',
        (inv) => inv.namedArguments[Symbol('ip')] == '10.0.0.2'
            ? '{"success":true,"error":null}'
            : '{"success":false,"error":"peer refused"}',
      );

      final path = await media.fetchBlobFromLan(
        'h-abc',
        peers: [
          P2pPeerDto(
            pubkey: 'pk-1',
            ip: '10.0.0.1',
            port: 4000,
            quicPort: null,
          ),
          P2pPeerDto(
            pubkey: 'pk-2',
            ip: '10.0.0.2',
            port: 4000,
            quicPort: 4433,
          ),
        ],
        outPath: '/tmp/out.bin',
      );
      expect(path, '/tmp/out.bin');
      expect(media.lastError, isNull);
      expect(api.callCount('crateFfiP2PP2PFetchBlobFromPeer'), 2);

      // All peers fail -> lastError + rethrow.
      api.handlers.clear();
      api.stubStringBuilder('crateFfiP2PP2PFetchBlobFromPeer',
          (_) => '{"success":false,"error":"peer refused"}');
      await expectLater(
        media.fetchBlobFromLan(
          'h-abc',
          peers: [
            P2pPeerDto(
                pubkey: 'pk-1',
                ip: '10.0.0.1',
                port: 4000,
                quicPort: null),
          ],
          outPath: '/tmp/out.bin',
        ),
        throwsException,
      );
      expect(media.lastError, contains('peer refused'));
    });

    test('fetchBlobFromLan rejects empty peer list', () async {
      final media = MediaService();
      await expectLater(
        media.fetchBlobFromLan(
          'h-abc',
          peers: const [],
          outPath: '/tmp/out.bin',
        ),
        throwsException,
      );
      expect(media.lastError, contains('No LAN peers'));
    });
  });
}