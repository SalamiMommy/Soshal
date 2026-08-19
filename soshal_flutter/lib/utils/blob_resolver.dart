import 'dart:io';

import '../services/media_service.dart';
import '../services/p2p_service.dart';

Future<String?> resolveBlobPath(
  MediaService media,
  P2pService p2p,
  String hash,
) async {
  try {
    final local = await media.fetchBlobQuiet(hash);
    if (local == null) {
      return await media.fetchBlobFromLan(
        hash,
        peers: p2p.peers,
        outPath: '${Directory.systemTemp.path}/$hash',
      );
    }
    return local;
  } catch (_) {
    return null;
  }
}
