// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:soshal_flutter/ffi/media.dart';
import 'package:soshal_flutter/ffi/p2p.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Media Service
/// Handles media upload/download, blob storage, and local file serving via
/// the Rust media-core chunk store. Provides streaming HTTP range server
/// (sendfile) for video playback. LAN peers can be crawled for blobs by
/// hash alone (`fetchBlobFromLan`) — one TCP/QUIC round trip for the
/// manifest, then parallel-verified chunk pulls.
class MediaService extends ChangeNotifier with LastErrorMixin, ServiceGuard {
  int? _localServerPort;
  Future<int>? _localServerStart;
  AppLifecycleListener? _lifecycle;
  static const int _blobCacheCap = 64;
  final Map<String, String> _blobCache = {};
  final Map<String, Future<String>> _blobInFlight = {};

  /// Separate in-flight map for [fetchBlobQuiet] because its contract differs
  /// from [fetchBlob] — it resolves to null on a miss instead of throwing — so
  /// the two cannot share one map without changing a caller's error handling.
  final Map<String, Future<String?>> _blobQuietInFlight = {};
  Future<String>? _cachePathFuture;

  int? get localServerPort => _localServerPort;

  MediaService() {
    _lifecycle = AppLifecycleListener(
      // Stop the blob server on real backgrounding only. `onPause` fires for
      // transient interruptions (dialogs, notification shade, permission
      // prompts) too, but keeping `onHide` here killed the server during
      // any such overlay and nothing restarted it on resume.
      onPause: _stopLocalServerIfRunning,
      onResume: _restartLocalServerIfNeeded,
    );
  }

  bool _wasServingOnPause = false;

  void _stopLocalServerIfRunning() {
    _wasServingOnPause = _localServerPort != null;
    if (_localServerPort != null) stopLocalServer();
  }

  void _restartLocalServerIfNeeded() {
    if (_wasServingOnPause) {
      _wasServingOnPause = false;
      unawaited(startLocalServer());
    }
  }

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
        final json = await p2PFetchBlobFromPeer(
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
  /// `filePath` may be a local file path or an http(s):// URL (SSRF-guarded
  /// fetch on the Rust side). The blob is chunked, deduplicated, and stored
  /// in the local CAS.
  Future<Map<String, dynamic>> uploadMedia(String filePath) => guard(() async {
        final file = File(filePath);
        if (!await file.exists()) {
          throw Exception('File not found: $filePath');
        }

        final mime = await mimeType(filePath);
        final window = await chunkingForMime(mime);

        final manifestJson =
            await RustLib.instance.api.crateFfiMediaMediaUploadBlobFile(
          filePath: filePath,
        );

        final manifest = Map<String, dynamic>.from(
          jsonDecode(manifestJson) as Map,
        );
        manifest['chunking'] = window;
        return manifest;
      });

  /// Local-only blob fetch that never sets lastError: returns null when the
  /// blob is absent from the chunk store. For callers with a LAN/URL
  /// fallback chain (feed, minis, blob_resolver) — a cache miss is expected
  /// there and must not spam the error log.
  ///
  /// Deduplicated against [_blobCache] / [_blobQuietInFlight] exactly like
  /// [fetchBlob]. The underlying `media_fetch_blob` re-reads and re-writes every
  /// chunk of the blob, so without this the same hash appearing in N feed cards
  /// paid N full CAS reconstructions.
  Future<String?> fetchBlobQuiet(String blobHash, {String? outPath}) {
    if (outPath == null) {
      final cached = _blobCache[blobHash];
      if (cached != null) return Future.value(cached);
      final inFlight = _blobQuietInFlight[blobHash];
      if (inFlight != null) return inFlight;
    }
    final future = _fetchBlobQuiet(blobHash, outPath);
    if (outPath == null) {
      _blobQuietInFlight[blobHash] = future;
      future.then((path) {
        _blobQuietInFlight.remove(blobHash);
        // A hit left the blob on disk at exactly the path `fetchBlob` would
        // have returned, so seed the shared cache and let a later `fetchBlob`
        // for the same hash skip the FFI entirely.
        if (path != null) {
          _blobCache[blobHash] = path;
          if (_blobCache.length > _blobCacheCap) {
            _blobCache.remove(_blobCache.keys.first);
          }
        }
      }, onError: (_) {
        _blobQuietInFlight.remove(blobHash);
      });
    }
    return future;
  }

  Future<String?> _fetchBlobQuiet(String blobHash, String? outPath) async {
    try {
      var target = outPath;
      if (target == null) {
        final tempDir = await getCachePath();
        target = '$tempDir/$blobHash';
      }
      await RustLib.instance.api.crateFfiMediaMediaFetchBlob(
        blobHash: blobHash,
        outPath: target,
      );
      return target;
    } catch (_) {
      return null;
    }
  }

  /// Fetch a blob by hash from the chunk store (local or swarm).
  /// Returns the local file path after download.
  Future<String> fetchBlob(String blobHash, {String? outPath}) {
    if (outPath == null) {
      final cached = _blobCache[blobHash];
      if (cached != null) return Future.value(cached);
      final inFlight = _blobInFlight[blobHash];
      if (inFlight != null) return inFlight;
    }
    final future = _fetchBlob(blobHash, outPath);
    if (outPath == null) {
      _blobInFlight[blobHash] = future;
      future.then((path) {
        _blobInFlight.remove(blobHash);
        _blobCache[blobHash] = path;
        if (_blobCache.length > _blobCacheCap) {
          _blobCache.remove(_blobCache.keys.first);
        }
      }, onError: (_) {
        _blobInFlight.remove(blobHash);
      });
    }
    return future;
  }

  Future<String> _fetchBlob(String blobHash, String? outPath) =>
      guard(() async {
        var target = outPath;
        if (target == null) {
          // Use default temp location if not specified
          final tempDir = RustLib.instance.api.crateFfiMediaMediaGetCachePath();
          target = '$tempDir/$blobHash';
        }

        final manifestJson =
            await RustLib.instance.api.crateFfiMediaMediaFetchBlob(
          blobHash: blobHash,
          outPath: target,
        );

        final result = Map<String, dynamic>.from(
          jsonDecode(manifestJson) as Map,
        );

        if (result['success'] != true) {
          throw Exception(result['error'] ?? 'Fetch failed');
        }

        return target;
      });

  /// Start a local HTTP range server for media playback (sendfile zero-copy).
  /// Returns the bound port.
  Future<int> startLocalServer() {
    if (_localServerPort != null) return Future.value(_localServerPort!);
    final inFlight = _localServerStart;
    if (inFlight != null) return inFlight;
    final future = _startLocalServer();
    _localServerStart = future;
    future.then((_) {
      _localServerStart = null;
    }, onError: (_) {
      _localServerStart = null;
    });
    return future;
  }

  Future<int> _startLocalServer() async {
    final port = await guard(() async {
      return RustLib.instance.api.crateFfiMediaMediaStartLocalServer().toInt();
    });
    _localServerPort = port;
    return port;
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

  /// Get the local HTTP URL for a blob hash (for media_kit playback).
  /// The local server must be started first.
  String getLocalUrl(String blobHash) {
    if (_localServerPort == null) {
      throw Exception('Local server not started');
    }
    return 'http://127.0.0.1:$_localServerPort/blob/$blobHash';
  }

  /// Get the cache path for the chunk store.
  ///
  /// Memoized. The Rust side resolves this from the `OnceLock` that `db_init`
  /// pins once at startup (`ChunkStore::set_default_root`), and `db_init` is
  /// itself memoized process-wide in `FfiBridge.ensureDatabaseInitialized`, so
  /// the value is fixed for the process lifetime. Five call sites — including
  /// every `fetch` — were each paying an FFI round-trip for that constant.
  Future<String> getCachePath() {
    final existing = _cachePathFuture;
    if (existing != null) return existing;
    final future = guard(() async {
      return RustLib.instance.api.crateFfiMediaMediaGetCachePath();
    }, notifyOnSuccess: false);
    _cachePathFuture = future;
    // Never memoize a rejected future: one transient FFI failure would
    // otherwise poison the path for the rest of the process.
    future.then((_) {}, onError: (_) {
      if (identical(_cachePathFuture, future)) _cachePathFuture = null;
    });
    return future;
  }

  /// Clear the chunk cache (evicts all stored chunks).
  Future<void> clearCache() async {
    try {
      final cachePath = await getCachePath();
      RustLib.instance.api.crateFfiMediaMediaClearCache(cacheDir: cachePath);
      _blobCache.clear();
      _blobInFlight.clear();
      _blobQuietInFlight.clear();
      clearLastError();
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
    }
  }

  /// Upload a local file (or URL, SSRF-guarded) to a Blossom server.
  /// Returns the server-side hash/url.
  Future<String> upload(String filePath, {required String blossomServer}) =>
      guard(() {
        return RustLib.instance.api.crateFfiMediaMediaUpload(
          filePath: filePath,
          blossomServer: blossomServer,
        );
      });

  /// Fetch media from a URL and cache it locally; returns the cache path.
  Future<String> fetch(String url, {String? cacheDir}) => guard(() async {
        final dir = cacheDir ?? await getCachePath();
        return await RustLib.instance.api.crateFfiMediaMediaFetch(
          url: url,
          cacheDir: dir,
        );
      });

  /// Load a local media file's bytes off the UI isolate.
  Future<Uint8List> loadLocal(String path) async {
    return await guard(() async {
      return RustLib.instance.api.crateFfiMediaMediaLoadLocal(
        filePath: path,
      );
    }, notifyOnSuccess: false);
  }

  /// MIME type of a local file (inferred on the Rust side).
  Future<String> mimeType(String path) async {
    return await guard(() async {
      return RustLib.instance.api.crateFfiMediaMediaGetMimeType(
        filePath: path,
      );
    }, notifyOnSuccess: false);
  }

  /// Upload raw bytes to the local chunk store; returns the blob manifest
  /// (`blob_hash`, `total_size`, `chunks`).
  Future<Map<String, dynamic>> uploadBlob(List<int> bytes) => guard(() async {
        final manifestJson = await RustLib.instance.api
            .crateFfiMediaMediaUploadBlob(data: bytes);
        return Map<String, dynamic>.from(
          jsonDecode(manifestJson) as Map,
        );
      });

  /// Content-aware chunking window (min/avg/max) for a MIME type.
  Future<Map<String, dynamic>> chunkingForMime(String mime) async {
    return await guard(() async {
      final json =
          RustLib.instance.api.crateFfiMediaMediaChunkingForMime(mime: mime);
      return Map<String, dynamic>.from(jsonDecode(json) as Map);
    }, notifyOnSuccess: false);
  }

  /// Feed scroll telemetry into the global prefetcher.
  Future<bool> updateScrollTelemetry({
    required double velocity,
    required int topIndex,
    required int bottomIndex,
  }) async {
    return await guard(() async {
      return RustLib.instance.api.crateFfiMediaMediaUpdateScrollTelemetry(
        velocity: velocity,
        topIndex: topIndex,
        bottomIndex: bottomIndex,
      );
    }, notifyOnSuccess: false);
  }

  @override
  void dispose() {
    _lifecycle?.dispose();
    super.dispose();
  }
}
