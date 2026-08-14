// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/ffi/media.dart';
import 'package:soshal_flutter/ffi/p2p.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Media Service
/// Handles media upload/download, blob storage, and local file serving via
/// the Rust media-core chunk store. Provides streaming HTTP range server
/// (sendfile) for video playback. LAN peers can be crawled for blobs by
/// hash alone (`fetchBlobFromLan`) — one TCP/QUIC round trip for the
/// manifest, then parallel-verified chunk pulls.
class MediaService extends ChangeNotifier with LastErrorMixin {
  int? _localServerPort;

  int? get localServerPort => _localServerPort;

  /// Fetch a blob by hash from LAN peers (crawl-then-swarm), falling back
  /// through every discovered peer until one succeeds. `peers` maps to
  /// `P2pPeerDto` from P2pService; QUIC ports are preferred when advertised.
  /// Returns the local file path; skips to next peer on transport errors.
  Future<String> fetchBlobFromLan(
    String blobHash, {
    required List<P2pPeerDto> peers,
    required String outPath,
  }) async {
    if (peers.isEmpty) throw Exception('No LAN peers to fetch from');
    final dir = File(outPath).parent;
    if (!await dir.exists()) await dir.create(recursive: true);
    Object? last;
    for (final peer in peers) {
      try {
        final json = p2PFetchBlobFromPeer(
          blobHash: blobHash,
          ip: peer.ip,
          tcpPort: peer.port,
          quicPort: peer.quicPort,
          outPath: outPath,
        );
        final res = Map<String, dynamic>.from(jsonDecode(json) as Map);
        if (res['success'] != true) {
          throw Exception(res['error'] ?? 'Peer fetch failed');
        }
        clearLastError();
        notifyListeners();
        return outPath;
      } catch (e) {
        last = e;
        debugPrint('media: peer ${peer.ip} failed: $e');
      }
    }
    setLastError(last ?? 'null');
    notifyListeners();
    throw Exception('All LAN peers failed: $last');
  }

  /// Decode an image into uncompressed RGBA pixels on Rust worker threads
  /// (off the UI isolate). Returns pixels + dimensions.
  Future<DecodedImageRgbaDto> decodeImageRgba(
    String filePathOrUrl, {
    int? maxWidth,
    int? maxHeight,
  }) async {
    return RustLib.instance.api.crateFfiMediaMediaDecodeImageRgba(
      filePathOrUrl: filePathOrUrl,
      maxWidth: maxWidth,
      maxHeight: maxHeight,
    );
  }

  /// Upload media to the chunk store and return the blob manifest.
  /// The blob is chunked, deduplicated, and stored in the local CAS.
  Future<Map<String, dynamic>> uploadMedia(String filePath) async {
    try {
      final file = File(filePath);
      if (!await file.exists()) {
        throw Exception('File not found: $filePath');
      }

      final manifestJson =
          RustLib.instance.api.crateFfiMediaMediaUploadBlobFile(
        filePath: filePath,
      );

      final manifest = Map<String, dynamic>.from(
        jsonDecode(manifestJson) as Map,
      );

      clearLastError();
      notifyListeners();
      return manifest;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Fetch a blob by hash from the chunk store (local or swarm).
  /// Returns the local file path after download.
  Future<String> fetchBlob(String blobHash, {String? outPath}) async {
    try {
      if (outPath == null) {
        // Use default temp location if not specified
        final tempDir = RustLib.instance.api.crateFfiMediaMediaGetCachePath();
        outPath = '$tempDir/$blobHash';
      }

      final manifestJson = RustLib.instance.api.crateFfiMediaMediaFetchBlob(
        blobHash: blobHash,
        outPath: outPath,
      );

      final result = Map<String, dynamic>.from(
        jsonDecode(manifestJson) as Map,
      );

      if (result['success'] != true) {
        throw Exception(result['error'] ?? 'Fetch failed');
      }

      clearLastError();
      notifyListeners();
      return outPath;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Start a local HTTP range server for media playback (sendfile zero-copy).
  /// Returns the bound port.
  Future<int> startLocalServer() async {
    try {
      _localServerPort =
          RustLib.instance.api.crateFfiMediaMediaStartLocalServer().toInt();
      clearLastError();
      notifyListeners();
      return _localServerPort!;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Stop the local HTTP range server.
  Future<void> stopLocalServer() async {
    try {
      RustLib.instance.api.crateFfiMediaMediaStopLocalServer();
      _localServerPort = null;
      clearLastError();
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
    }
  }

  /// Get the local HTTP URL for a blob hash (for video_player).
  /// The local server must be started first.
  String getLocalUrl(String blobHash) {
    if (_localServerPort == null) {
      throw Exception('Local server not started');
    }
    return 'http://127.0.0.1:$_localServerPort/blob/$blobHash';
  }

  /// Get the cache path for the chunk store.
  Future<String> getCachePath() async {
    try {
      final path = RustLib.instance.api.crateFfiMediaMediaGetCachePath();
      clearLastError();
      return path;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Clear the chunk cache (evicts all stored chunks).
  Future<void> clearCache() async {
    try {
      final cachePath = await getCachePath();
      RustLib.instance.api.crateFfiMediaMediaClearCache(cacheDir: cachePath);
      clearLastError();
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
    }
  }
}
