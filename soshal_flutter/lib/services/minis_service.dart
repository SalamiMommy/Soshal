import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import 'media_service.dart';
import 'p2p_service.dart';

/// Minis: mini video registry (kind-31020) plus WASI content-filter /
/// feed-ranker plugin execution. Stateless wrapper — screens own their UI
/// state.
class MinisService extends ChangeNotifier with LastErrorMixin {
  bool _wasmRuntimeUnavailable = false;

  /// True after a plugin call fails: the WASI host is on the roadmap, so
  /// execution is simulated and errors are expected.
  bool get wasmRuntimeUnavailable => _wasmRuntimeUnavailable;

  /// Fetch known minis from the local registry, newest first.
  List<MiniItem> fetchMinis() {
    try {
      final json =
          RustLib.instance.api.crateFfiMinisMinisFetch(audience: 'public');
      final list = (jsonDecode(json) as List<dynamic>)
          .map((e) => MiniItem.fromJson(e as Map<String, dynamic>))
          .toList();
      clearLastError();
      return list;
    } catch (e, st) {
      setLastError(e, st);
      rethrow;
    }
  }

  /// Publish a mini video (kind 31020). `mediaSource` is a local video file
  /// path or an https media URL; bytes are chunked into the local CAS and
  /// the event carries a blob tag so peers can fetch from caches. Returns
  /// the event id.
  Future<String> publishMini({
    required String mediaSource,
    String? textOverlay,
    String? thumbnail,
    String? audience,
  }) async {
    try {
      final id = await RustLib.instance.api.crateFfiMinisMinisPublish(
        mediaSource: mediaSource,
        textOverlay: textOverlay,
        thumbnail: thumbnail,
        audience: audience,
      );
      clearLastError();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      rethrow;
    }
  }

  /// Run a WASI content-filter plugin against text.
  String runFilter({
    required String pluginId,
    required String text,
    required String wasmBytesHex,
  }) {
    try {
      final result = RustLib.instance.api.crateFfiMinisMinisWasmExecuteFilter(
        pluginId: pluginId,
        text: text,
        wasmBytesHex: wasmBytesHex,
      );
      clearLastError();
      if (_wasmRuntimeUnavailable) {
        _wasmRuntimeUnavailable = false;
        notifyListeners();
      }
      return result;
    } catch (e, st) {
      setLastError(e, st);
      if (!_wasmRuntimeUnavailable) {
        _wasmRuntimeUnavailable = true;
        notifyListeners();
      }
      // Honest-err: empty string looked like success. Throw so callers
      // show "unavailable (roadmap)" instead of empty result.
      throw StateError('WASI filter unavailable (roadmap): $e');
    }
  }

  /// Run a WASI feed-ranker plugin over candidate posts (JSON strings).
  List<String> rankFeed({
    required String pluginId,
    required List<String> postsJson,
    required String wasmBytesHex,
  }) {
    try {
      final ranked = RustLib.instance.api.crateFfiMinisMinisWasmRankFeed(
        pluginId: pluginId,
        postsJson: postsJson,
        wasmBytesHex: wasmBytesHex,
      );
      clearLastError();
      if (_wasmRuntimeUnavailable) {
        _wasmRuntimeUnavailable = false;
        notifyListeners();
      }
      return ranked;
    } catch (e, st) {
      setLastError(e, st);
      if (!_wasmRuntimeUnavailable) {
        _wasmRuntimeUnavailable = true;
        notifyListeners();
      }
      throw StateError('WASI ranker unavailable (roadmap): $e');
    }
  }
}

/// A mini (kind 31020), serialized via `mini_from_event`.
class MiniItem {
  final String id;
  final String pubkey;
  final String videoUrl;
  final String blobHash;
  final int mediaSize;
  final String textOverlay;
  final String thumbnail;
  final String audience;
  final int createdAt;

  MiniItem({
    required this.id,
    required this.pubkey,
    required this.videoUrl,
    required this.blobHash,
    required this.mediaSize,
    required this.textOverlay,
    required this.thumbnail,
    required this.audience,
    required this.createdAt,
  });

  factory MiniItem.fromJson(Map<String, dynamic> json) => MiniItem(
        id: json.strOf('id'),
        pubkey: json.strOf('pubkey'),
        videoUrl: json.strOf('videoUrl'),
        blobHash: json.strOf('blobHash'),
        mediaSize: json.intOf('mediaSize'),
        textOverlay: json.strOf('textOverlay'),
        thumbnail: json.strOf('thumbnail'),
        audience: json.strOrNull('audience') ?? 'public',
        createdAt: json.intOf('createdAt'),
      );
}

/// Resolves a mini's video to a playable URL: local CAS blob first, then
/// LAN peer fetch, then the original URL as fallback (blob-first, URL
/// fallback). Returns null when nothing is reachable.
Future<String?> resolveMiniPlaybackUrl(
  MiniItem mini,
  MediaService media,
  P2pService p2p,
) async {
  if (mini.blobHash.isNotEmpty) {
    final local = await media.fetchBlobQuiet(mini.blobHash);
    if (local != null) {
      await media.startLocalServer();
      return media.getLocalUrl(mini.blobHash);
    }
    try {
      await media.fetchBlobFromLan(
        mini.blobHash,
        peers: p2p.peers,
        outPath: '${Directory.systemTemp.path}/${mini.blobHash}',
      );
      await media.startLocalServer();
      return media.getLocalUrl(mini.blobHash);
    } catch (_) {
      // Fall through to the URL fallback.
    }
  }
  return mini.videoUrl.isNotEmpty ? mini.videoUrl : null;
}
