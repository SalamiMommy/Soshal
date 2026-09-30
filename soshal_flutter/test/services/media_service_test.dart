// ignore_for_file: invalid_use_of_internal_member
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/p2p.dart';
import 'package:soshal_flutter/services/media_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-media');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('MediaService', () {
    test('uploadMedia stubs FFI, parses manifest, passes filePath', () async {
      final media = MediaService();
      api.stubString('crateFfiMediaMediaGetMimeType', 'video/mp4');
      api.stubString(
        'crateFfiMediaMediaChunkingForMime',
        '{"min":1048576,"avg":2097152,"max":4194304}',
      );
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
      expect(manifest['chunking'], {
        'min': 1048576,
        'avg': 2097152,
        'max': 4194304,
      });
      expect(media.lastError, isNull);
      final inv = api.callsOf('crateFfiMediaMediaUploadBlobFile').single;
      expect(api.namedArg(inv, 'filePath'), file.path);
      final mimeInv = api.callsOf('crateFfiMediaMediaGetMimeType').single;
      expect(api.namedArg(mimeInv, 'filePath'), file.path);
      final chunkInv = api.callsOf('crateFfiMediaMediaChunkingForMime').single;
      expect(api.namedArg(chunkInv, 'mime'), 'video/mp4');
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

    test('getCachePath is memoized after the first resolved call', () async {
      final media = MediaService();
      api.stubString('crateFfiMediaMediaGetCachePath', '/tmp/cache');

      expect(await media.getCachePath(), '/tmp/cache');
      expect(await media.getCachePath(), '/tmp/cache');
      expect(await media.getCachePath(), '/tmp/cache');

      expect(
        api.callsOf('crateFfiMediaMediaGetCachePath').length,
        1,
        reason: 'the path is pinned once at startup by db_init; every extra '
            'call is a pure FFI round-trip for a constant',
      );
    });

    test('a failed getCachePath is not memoized for later callers', () async {
      final media = MediaService();
      var calls = 0;
      api.stub('crateFfiMediaMediaGetCachePath', (_) {
        calls++;
        if (calls == 1) throw Exception('bridge not ready');
        return '/tmp/cache';
      });

      await expectLater(media.getCachePath(), throwsException);
      expect(await media.getCachePath(), '/tmp/cache');
      expect(calls, 2,
          reason: 'a rejected future must be dropped, not cached for the '
              'rest of the process');
    });

    test('fetchBlobQuiet resolves a hit to the cache path', () async {
      final media = MediaService();
      api.stubString('crateFfiMediaMediaGetCachePath', '/tmp/cache');
      api.stubString(
        'crateFfiMediaMediaFetchBlob',
        '{"success":true,"blob_hash":"h-abc","size":12}',
      );

      expect(await media.fetchBlobQuiet('h-abc'), '/tmp/cache/h-abc');
      final inv = api.callsOf('crateFfiMediaMediaFetchBlob').single;
      expect(api.namedArg(inv, 'outPath'), '/tmp/cache/h-abc');
    });

    test('fetchBlobQuiet returns null and sets no error on a miss', () async {
      final media = MediaService();
      api.stubString('crateFfiMediaMediaGetCachePath', '/tmp/cache');
      api.stub('crateFfiMediaMediaFetchBlob',
          (_) => throw Exception('manifest not found'));

      expect(await media.fetchBlobQuiet('h-missing'), isNull);
      expect(media.lastError, isNull,
          reason: 'a local cache miss is expected for LAN/URL fallback '
              'callers and must not spam the error log');
    });

    test('fetchBlobQuiet deduplicates concurrent fetches of one hash',
        () async {
      final media = MediaService();
      api.stubString('crateFfiMediaMediaGetCachePath', '/tmp/cache');
      api.stubString(
        'crateFfiMediaMediaFetchBlob',
        '{"success":true,"blob_hash":"h-abc","size":12}',
      );

      final paths = await Future.wait([
        media.fetchBlobQuiet('h-abc'),
        media.fetchBlobQuiet('h-abc'),
        media.fetchBlobQuiet('h-abc'),
      ]);

      expect(paths, everyElement('/tmp/cache/h-abc'));
      expect(
        api.callsOf('crateFfiMediaMediaFetchBlob').length,
        1,
        reason: 'each FFI call re-reads and re-writes every chunk of the '
            'blob; N cards showing one image must cost one CAS rebuild',
      );
    });

    test('a quiet fetch seeds the shared cache for a later fetchBlob',
        () async {
      final media = MediaService();
      api.stubString('crateFfiMediaMediaGetCachePath', '/tmp/cache');
      api.stubString(
        'crateFfiMediaMediaFetchBlob',
        '{"success":true,"blob_hash":"h-abc","size":12}',
      );

      await media.fetchBlobQuiet('h-abc');
      // A cache hit returns without re-deriving the manifest from the FFI.
      api.handlers.remove('crateFfiMediaMediaFetchBlob');
      expect(await media.fetchBlob('h-abc'), '/tmp/cache/h-abc');
    });

    test('fetchBlobQuiet respects an explicit outPath without deduplicating',
        () async {
      final media = MediaService();
      api.stubString(
        'crateFfiMediaMediaFetchBlob',
        '{"success":true,"blob_hash":"h-abc","size":12}',
      );

      await media.fetchBlobQuiet('h-abc', outPath: '/tmp/a.bin');
      await media.fetchBlobQuiet('h-abc', outPath: '/tmp/b.bin');

      expect(api.callsOf('crateFfiMediaMediaFetchBlob').length, 2,
          reason: 'distinct explicit destinations must not share a result');
      expect(api.callsOf('crateFfiMediaMediaGetCachePath').isEmpty, isTrue,
          reason: 'an explicit outPath needs no cache-path round-trip');
    });

    test('local server start/stop and URL building', () async {
      final media = MediaService();
      expect(() => media.getLocalUrl('h-abc'), throwsException,
          reason: 'server not started yet');
      expect(media.lastError, isNull,
          reason: 'getLocalUrl throws without setting lastError');

      api.stub('crateFfiMediaMediaStartLocalServer', (_) => BigInt.from(8765));
      final port = await media.startLocalServer();
      expect(port, 8765);
      expect(media.localServerPort, 8765);
      expect(media.getLocalUrl('h-abc'), 'http://127.0.0.1:8765/blob/h-abc');

      api.stub('crateFfiMediaMediaStopLocalServer', (_) => true);
      await media.stopLocalServer();
      expect(media.localServerPort, isNull);
      expect(media.lastError, isNull);
    });

    test('clearCache passes resolved cache dir', () async {
      final media = MediaService();
      api.stubString('crateFfiMediaMediaGetCachePath', '/tmp/cache');
      api.stubString('crateFfiMediaMediaClearCache', '');
      await media.clearCache();
      final inv = api.callsOf('crateFfiMediaMediaClearCache').single;
      expect(api.namedArg(inv, 'cacheDir'), '/tmp/cache');
      expect(media.lastError, isNull);
    });

    test('FFI throw sets lastError and rethrows', () async {
      final media = MediaService();
      api.stubString('crateFfiMediaMediaGetMimeType', 'video/mp4');
      api.stubString('crateFfiMediaMediaChunkingForMime', '{}');
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

    test('fetchBlobFromLan swarms fallback and surfaces all-fail', () async {
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
                pubkey: 'pk-1', ip: '10.0.0.1', port: 4000, quicPort: null),
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
      expect(media.lastError, isNull,
          reason: 'empty-peer check throws before setting lastError');
    });
  });
}
