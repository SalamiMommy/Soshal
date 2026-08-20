// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/blob_resolver.dart';
import '../utils/safe_url.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';
import 'media_service.dart';
import 'p2p_service.dart';
import 'social_entry.dart';

/// Musicloud: kind-31022 track publishing, fetching, sharing to feed and
/// comment threads. All FFI calls are async — always awaited.
class MusicService extends ChangeNotifier with LastErrorMixin {
  List<MusicTrack> _tracks = [];

  List<MusicTrack> get tracks => _tracks;

  /// Fetch tracks (kind 31022), optionally filtered by author pubkey.
  Future<List<MusicTrack>> fetchTracks({String? author, int limit = 50}) async {
    try {
      final json = await RustLib.instance.api.crateFfiMusicMusicFetch(
        limit: BigInt.from(limit),
        author: author,
      );
      _tracks = await runOffThread(() => _parseTracks(json));
      clearLastError();
      notifyListeners();
      return _tracks;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Publish a track (kind 31022). `mediaSource` is a local audio file path
  /// or an https media URL; the bytes are chunked into the local CAS and the
  /// event carries a blob tag so peers can fetch from caches. Returns the
  /// event id.
  Future<String> publishTrack({
    required String mediaSource,
    String? title,
    String? thumbnail,
    List<String> hashtags = const [],
    String? audience,
  }) async {
    try {
      final id = await RustLib.instance.api.crateFfiMusicMusicPublish(
        mediaSource: mediaSource,
        title: title,
        thumbnail: thumbnail,
        hashtags: hashtags,
        audience: audience,
      );
      clearLastError();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Share a track to the feed as a kind-1 text note. Returns the event id.
  Future<String> shareToFeed({
    required String trackId,
    required String trackPubkey,
    required String message,
    List<String> hashtags = const [],
  }) async {
    try {
      final id = await RustLib.instance.api.crateFfiMusicMusicShareToFeed(
        trackId: trackId,
        trackPubkey: trackPubkey,
        message: message,
        hashtags: hashtags,
      );
      clearLastError();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Publish a comment on a track. Returns the comment event id.
  Future<String> comment({
    int trackKind = 31022,
    required String trackPubkey,
    required String trackD,
    required String content,
  }) async {
    try {
      final id = await RustLib.instance.api.crateFfiMusicMusicComment(
        trackKind: trackKind,
        trackPubkey: trackPubkey,
        trackD: trackD,
        content: content,
      );
      clearLastError();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Fetch comments for a track. Returns JSON array of mini event outputs.
  Future<List<TrackComment>> fetchComments({
    int trackKind = 31022,
    required String trackPubkey,
    required String trackD,
  }) async {
    try {
      final json = await RustLib.instance.api.crateFfiMusicMusicComments(
        trackKind: trackKind,
        trackPubkey: trackPubkey,
        trackD: trackD,
      );
      final comments = await runOffThread(() => _parseComments(json));
      clearLastError();
      return comments;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }
}

/// A Musicloud track (kind 31022), serialized via `musicloud_from_event`.
class MusicTrack {
  final String id;
  final String pubkey;
  final String audioUrl;
  final String blobHash;
  final int mediaSize;
  final String title;
  final String thumbnail;
  final List<String> hashtags;
  final String d;
  final String audience;
  final int createdAt;

  MusicTrack({
    required this.id,
    required this.pubkey,
    required this.audioUrl,
    required this.blobHash,
    required this.mediaSize,
    required this.title,
    required this.thumbnail,
    required this.hashtags,
    required this.d,
    required this.audience,
    required this.createdAt,
  });

  factory MusicTrack.fromJson(Map<String, dynamic> json) => MusicTrack(
        id: json.strOf('id'),
        pubkey: json.strOf('pubkey'),
        audioUrl: json.strOf('audioUrl'),
        blobHash: json.strOf('blobHash'),
        mediaSize: json.intOf('mediaSize'),
        title: json.strOf('title'),
        thumbnail: json.strOf('thumbnail'),
        hashtags: (json['hashtags'] as List<dynamic>? ?? const [])
            .map((e) => e as String? ?? '')
            .where((e) => e.isNotEmpty)
            .toList(),
        d: json.strOf('d'),
        audience: json.strOrNull('audience') ?? 'public',
        createdAt: json.intOf('createdAt'),
      );
}

/// Resolves a track's audio to a playable URL: local CAS blob first, then
/// LAN peer fetch, then an http(s) URL as fallback (blob-first, URL
/// fallback). `blob://` references are never returned — they are only
/// meaningful to the chunk store, not to the media player. Returns null
/// when nothing reachable is available.
Future<String?> resolveTrackPlaybackUrl(
  MusicTrack track,
  MediaService media,
  P2pService p2p,
) async {
  if (track.blobHash.isNotEmpty) {
    final path = await resolveBlobPath(media, p2p, track.blobHash);
    if (path != null) {
      try {
        await media.startLocalServer();
        return media.getLocalUrl(track.blobHash);
      } catch (_) {
        // Fall through to the URL fallback.
      }
    }
  }
  final url = track.audioUrl;
  if (url.isNotEmpty &&
      !url.startsWith('blob://') &&
      SafeUrl.isSafeMediaUrl(url)) {
    return url;
  }
  return null;
}

/// A comment on a track (kind 1 with `E` tag), serialized via `mini_event_out`.
class TrackComment extends SocialEntry {
  TrackComment({
    required super.id,
    required super.pubkey,
    required super.content,
    required super.createdAt,
  });

  factory TrackComment.fromJson(Map<String, dynamic> json) => TrackComment(
        id: json.strOf('id'),
        pubkey: json.strOf('pubkey'),
        content: json.strOf('textOverlay'),
        createdAt: json.intOf('createdAt'),
      );
}

/// JSON → [MusicTrack] list, top-level so [runOffThread] can decode on a
/// background isolate.
List<MusicTrack> _parseTracks(String json) =>
    (jsonDecode(json) as List<dynamic>)
        .map((e) => MusicTrack.fromJson(e as Map<String, dynamic>))
        .toList();

/// JSON → [TrackComment] list, top-level so [runOffThread] can decode on a
/// background isolate.
List<TrackComment> _parseComments(String json) =>
    (jsonDecode(json) as List<dynamic>)
        .map((e) => TrackComment.fromJson(e as Map<String, dynamic>))
        .toList();
