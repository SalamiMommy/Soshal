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

  List<MiniItem> _saved = [];

  /// Minis the user saved locally for the Saved tab.
  List<MiniItem> get savedMinis => _saved;

  bool isSaved(String id) => _saved.any((m) => m.id == id);

  /// Fetch known minis from the local registry, newest first.
  List<MiniItem> fetchMinis({String audience = 'public'}) {
    try {
      final json =
          RustLib.instance.api.crateFfiMinisMinisFetch(audience: audience);
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

  /// Saved minis from the local `saved_content` store, newest saved first.
  Future<List<MiniItem>> fetchSavedMinis() async {
    try {
      final json = RustLib.instance.api.crateFfiMinisMinisSaved();
      final list = (jsonDecode(json) as List<dynamic>)
          .map((e) => MiniItem.fromJson(e as Map<String, dynamic>))
          .toList();
      _saved = list;
      clearLastError();
      notifyListeners();
      return list;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Save a mini: materialize its video blob into the local chunk store (so
  /// this device can serve it to peers, "hosting"), then persist the entry.
  /// Returns true when the blob was hosted on this device; false means the
  /// entry was saved but the blob is not yet available locally.
  Future<bool> saveMini(
    MiniItem mini, {
    required MediaService media,
    required P2pService p2p,
  }) async {
    try {
      final hosted = await hostMiniBlob(mini, media, p2p);
      RustLib.instance.api.crateFfiMinisMinisSave(eventId: mini.id);
      final json = RustLib.instance.api.crateFfiMinisMinisSaved();
      final list = (jsonDecode(json) as List<dynamic>)
          .map((e) => MiniItem.fromJson(e as Map<String, dynamic>))
          .toList();
      _saved = list;
      clearLastError();
      notifyListeners();
      return hosted;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  /// Remove a mini from the Saved tab.
  Future<void> unsaveMini(String id) async {
    try {
      RustLib.instance.api.crateFfiMinisMinisUnsave(eventId: id);
      final json = RustLib.instance.api.crateFfiMinisMinisSaved();
      _saved = (jsonDecode(json) as List<dynamic>)
          .map((e) => MiniItem.fromJson(e as Map<String, dynamic>))
          .toList();
      clearLastError();
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
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
  final int reactions;
  final bool liked;

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
    required this.reactions,
    required this.liked,
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
        reactions: json.intOf('reactions'),
        liked: json.boolOf('liked'),
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

/// Hosts a mini's video on this device: the blob must live in the local
/// chunk store (CAS) so peers can fetch it from this device. Uses the local
/// CAS copy when present, otherwise pulls it from LAN peers (which absorbs
/// it into the CAS), otherwise downloads the http(s) URL fallback and
/// re-uploads it. Returns whether the blob is now hosted locally.
Future<bool> hostMiniBlob(
  MiniItem mini,
  MediaService media,
  P2pService p2p,
) async {
  if (mini.blobHash.isEmpty) return false;
  final local = await media.fetchBlobQuiet(mini.blobHash);
  if (local != null) return true;
  try {
    await media.fetchBlobFromLan(
      mini.blobHash,
      peers: p2p.peers,
      outPath: '${Directory.systemTemp.path}/${mini.blobHash}',
    );
    return true;
  } catch (_) {
    // Fall through to the URL fallback.
  }
  final url = mini.videoUrl;
  if (url.startsWith('http')) {
    try {
      final file = await media.fetch(url, cacheDir: await media.getCachePath());
      await media.uploadMedia(file);
      return true;
    } catch (e) {
      debugPrint('hostMiniBlob: $e');
      logRuntimeError(e);
    }
  }
  return false;
}
