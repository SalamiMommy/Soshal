import 'dart:io';

import '../services/media_service.dart';
import '../services/p2p_service.dart';

final Map<String, String> _resolvedBlobPathCache = <String, String>{};
const int _maxBlobCacheEntries = 256;

Future<String?> resolveBlobPath(
  MediaService media,
  P2pService p2p,
  String hash,
) async {
  final cached = _resolvedBlobPathCache[hash];
  if (cached != null) {
    if (File(cached).existsSync()) {
      return cached;
    } else {
      _resolvedBlobPathCache.remove(hash);
    }
  }

  try {
    final local = await media.fetchBlobQuiet(hash);
    if (local == null) {
      final lanPath = await media.fetchBlobFromLan(
        hash,
        peers: p2p.peers,
        outPath: '${Directory.systemTemp.path}/$hash',
      );
      if (_resolvedBlobPathCache.length >= _maxBlobCacheEntries) {
        _resolvedBlobPathCache.remove(_resolvedBlobPathCache.keys.first);
      }
      _resolvedBlobPathCache[hash] = lanPath;
      return lanPath;
    }
    if (_resolvedBlobPathCache.length >= _maxBlobCacheEntries) {
      _resolvedBlobPathCache.remove(_resolvedBlobPathCache.keys.first);
    }
    _resolvedBlobPathCache[hash] = local;
    return local;
  } catch (_) {
    return null;
  }
}
