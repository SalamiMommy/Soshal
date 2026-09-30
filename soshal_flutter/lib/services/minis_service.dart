import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import 'media_service.dart';
import 'p2p_service.dart';
import '../utils/offthread.dart';
import '../utils/service_guard.dart';

/// Minis: mini video registry (kind-31020) plus WASI content-filter /
/// feed-ranker plugin execution. Stateless wrapper — screens own their UI
/// state.
class MinisService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify, ServiceGuard {
  bool _wasmRuntimeUnavailable = false;

  /// True after a plugin call fails: the WASI host is on the roadmap, so
  /// execution is simulated and errors are expected.
  bool get wasmRuntimeUnavailable => _wasmRuntimeUnavailable;

  List<MiniItem> _saved = [];
  /// Mirror of [_saved]'s ids, so [isSaved] is a hash lookup rather than a scan.
  ///
  /// This is a cache, not a source of truth: [_saved] is the list callers read
  /// and every mutation goes through [_setSaved] / [_removeSaved], which keep the
  /// two in step. `MiniItem` is not `==`-comparable, so mirroring by hand is the
  /// only option short of adding equality to it.
  Set<String> _savedIds = {};
  List<MiniItem> _minis = [];
  bool _minisLoading = false;
  StreamSubscription<List<MiniItem>>? _minisSub;

  /// Minis the user saved locally for the Saved tab.
  List<MiniItem> get savedMinis => _saved;
  List<MiniItem> get minis => _minis;
  bool get minisLoading => _minisLoading;

  /// Replace the saved list wholesale, keeping [_savedIds] in step.
  void _setSaved(List<MiniItem> list) {
    _saved = list;
    _savedIds = {for (final m in list) m.id};
  }

  /// Drop one id from [_saved] and the mirror. Returns whether anything was
  /// actually removed, so callers can tell a real change from a no-op.
  bool _removeSaved(String id) {
    final before = _saved.length;
    _saved.removeWhere((m) => m.id == id);
    if (_saved.length == before) return false;
    _savedIds = {..._savedIds}..remove(id);
    return true;
  }

  /// Whether `id` is saved.
  ///
  /// Called once per row by the ForYou grid and the saved list, so this ran
  /// O(rows x saved) on every build of either.
  bool isSaved(String id) => _savedIds.contains(id);

  @override
  void dispose() {
    _minisSub?.cancel();
    super.dispose();
  }

  /// Clears in-memory saved minis and transient state on account switch.
  void resetForAccountSwitch() {
    _minisSub?.cancel();
    _minisSub = null;
    _setSaved(const []);
    _minis = [];
    _minisLoading = false;
    clearLastError();
    notifyListeners();
  }

  /// Reactive stream of known minis, updating live when posts or reactions change.
  ///
  /// The bridge re-emits the *whole* registry on every change, so the decode
  /// runs on each emission — off the UI isolate via [runOffThreadCompute].
  Stream<List<MiniItem>> watchMinis({String audience = 'public'}) {
    return RustLib.instance.api
        .crateFfiMinisMinisWatch(audience: audience)
        .asyncMap((json) => runOffThreadCompute(_parseMinis, json));
  }

  /// Subscribe to live minis updates.
  void subscribeToMinis({String audience = 'public'}) {
    _minisSub?.cancel();
    _minisLoading = true;
    notifyDeferred();
    _minisSub = watchMinis(audience: audience).listen(
      (updated) {
        _minis = updated;
        _minisLoading = false;
        clearLastError();
        notifyDeferred();
      },
      onError: (e, st) {
        _minisLoading = false;
        setLastError(e, st);
        notifyDeferred();
      },
    );
  }

  /// Fetch known minis from the local registry, newest first.
  List<MiniItem> fetchMinis({String audience = 'public'}) => guardSync(() {
        final json =
            RustLib.instance.api.crateFfiMinisMinisFetch(audience: audience);
        final decoded = jsonDecode(json) as List<dynamic>;
        return List<MiniItem>.generate(
          decoded.length,
          (i) => MiniItem.fromJson(decoded[i] as Map<String, dynamic>),
          growable: true,
        );
      }, notifyOnSuccess: false, notifyOnError: false);

  /// Saved minis from the local `saved_content` store, newest saved first.
  Future<List<MiniItem>> fetchSavedMinis() => guard(() async {
        final list = await runOffThreadCompute(
          _parseMinis,
          RustLib.instance.api.crateFfiMinisMinisSaved(),
        );
        _setSaved(list);
        return list;
      });

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
      // Re-saving an already-saved mini moves it to the front without changing
      // the set, so the mirror is only rebuilt when the id was new.
      if (!_savedIds.contains(mini.id)) {
        _savedIds = {..._savedIds, mini.id};
      }
      _saved.removeWhere((m) => m.id == mini.id);
      _saved.insert(0, mini);
      clearLastError();
      notifyDeferred();
      return hosted;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return false;
    }
  }

  /// Remove a mini from the Saved tab.
  Future<void> unsaveMini(String id) async {
    try {
      RustLib.instance.api.crateFfiMinisMinisUnsave(eventId: id);
      _removeSaved(id);
      clearLastError();
      notifyDeferred();
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
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
  }) =>
      guard(() async {
        return await RustLib.instance.api.crateFfiMinisMinisPublish(
          mediaSource: mediaSource,
          textOverlay: textOverlay,
          thumbnail: thumbnail,
          audience: audience,
        );
      }, notifyOnSuccess: false, notifyOnError: false);

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
        notifyDeferred();
      }
      return result;
    } catch (e, st) {
      setLastError(e, st);
      if (!_wasmRuntimeUnavailable) {
        _wasmRuntimeUnavailable = true;
        notifyDeferred();
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
        notifyDeferred();
      }
      return ranked;
    } catch (e, st) {
      setLastError(e, st);
      if (!_wasmRuntimeUnavailable) {
        _wasmRuntimeUnavailable = true;
        notifyDeferred();
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

/// JSON → [MiniItem] list, top-level so [runOffThreadCompute] can decode on a
/// background isolate. Each mini carries caption, media URLs and tags, and the
/// registry re-emits in full on every change, so this is not a small payload.
List<MiniItem> _parseMinis(String json) {
  final decoded = jsonDecode(json) as List<dynamic>;
  return List<MiniItem>.generate(
    decoded.length,
    (i) => MiniItem.fromJson(decoded[i] as Map<String, dynamic>),
    growable: true,
  );
}
