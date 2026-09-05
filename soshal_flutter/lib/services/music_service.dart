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

  List<MusicTrack> _savedTracks = [];
  List<MusicPlaylist> _playlists = [];

  List<MusicTrack> get savedTracks => _savedTracks;
  List<MusicPlaylist> get playlists => _playlists;

  bool isTrackSaved(String id) => _savedTracks.any((t) => t.id == id);

  /// Fetch tracks (kind 31022), optionally filtered by author pubkey.
  Future<List<MusicTrack>> fetchTracks({String? author, int limit = 50}) async {
    try {
      final json = await RustLib.instance.api.crateFfiMusicMusicFetch(
        limit: BigInt.from(limit),
        author: author,
        audience: 'public',
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
    required String trackD,
    required String message,
    List<String> hashtags = const [],
  }) async {
    try {
      final id = await RustLib.instance.api.crateFfiMusicMusicShareToFeed(
        trackId: trackId,
        trackPubkey: trackPubkey,
        trackD: trackD,
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

  /// Saved tracks from the local `saved_content` store, newest saved first.
  Future<List<MusicTrack>> fetchSavedTracks() async {
    try {
      final json = RustLib.instance.api.crateFfiMusicMusicSaved();
      _savedTracks = await runOffThread(() => _parseTracks(json));
      clearLastError();
      notifyListeners();
      return _savedTracks;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Save a track: materialize its audio blob into the local chunk store (so
  /// this device can serve it to peers), then persist the entry. Returns true
  /// when the blob was hosted locally.
  Future<bool> saveTrack(
    MusicTrack track, {
    required MediaService media,
    required P2pService p2p,
  }) async {
    try {
      final hosted = await hostTrackBlob(track, media, p2p);
      RustLib.instance.api
          .crateFfiMusicMusicSave(trackJson: jsonEncode(trackToJson(track)));
      await fetchSavedTracks();
      clearLastError();
      return hosted;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  /// Remove a track from the Saved tab.
  Future<void> unsaveTrack(String id) async {
    try {
      RustLib.instance.api.crateFfiMusicMusicUnsave(trackId: id);
      await fetchSavedTracks();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
    }
  }

  /// The user's playlists, newest first.
  Future<List<MusicPlaylist>> fetchPlaylists() async {
    try {
      final json = RustLib.instance.api.crateFfiMusicMusicPlaylistList();
      _playlists = await runOffThread(
        () => (jsonDecode(json) as List<dynamic>)
            .map((e) => MusicPlaylist.fromJson(e as Map<String, dynamic>))
            .toList(),
      );
      clearLastError();
      notifyListeners();
      return _playlists;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Create a playlist. Returns the new playlist id.
  Future<String> createPlaylist({
    required String title,
    required bool isPrivate,
  }) async {
    try {
      final id = RustLib.instance.api.crateFfiMusicMusicPlaylistCreate(
        title: title,
        isPrivate: isPrivate,
      );
      await fetchPlaylists();
      clearLastError();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Rename a playlist.
  Future<void> renamePlaylist(String playlistId, String title) async {
    try {
      RustLib.instance.api.crateFfiMusicMusicPlaylistRename(
          playlistId: playlistId, title: title);
      await fetchPlaylists();
      clearLastError();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Delete a playlist and its tracks.
  Future<void> deletePlaylist(String playlistId) async {
    try {
      RustLib.instance.api
          .crateFfiMusicMusicPlaylistDelete(playlistId: playlistId);
      await fetchPlaylists();
      clearLastError();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Add a track to a playlist (de-duplicated by track id).
  Future<bool> addToPlaylist(String playlistId, MusicTrack track) async {
    try {
      final ok = RustLib.instance.api.crateFfiMusicMusicPlaylistAddTrack(
        playlistId: playlistId,
        trackJson: jsonEncode(trackToJson(track)),
      );
      clearLastError();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  /// Remove a track from a playlist.
  Future<void> removeFromPlaylist(String playlistId, String trackId) async {
    try {
      RustLib.instance.api.crateFfiMusicMusicPlaylistRemoveTrack(
        playlistId: playlistId,
        trackId: trackId,
      );
      clearLastError();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// A playlist's tracks in insertion order.
  Future<List<MusicTrack>> fetchPlaylistTracks(String playlistId) async {
    try {
      final json = RustLib.instance.api
          .crateFfiMusicMusicPlaylistTracks(playlistId: playlistId);
      final list = await runOffThread(() => _parseTracks(json));
      clearLastError();
      return list;
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

/// A user playlist; `trackCount` is the number of tracks in it.
class MusicPlaylist {
  final String id;
  final String title;
  final bool isPrivate;
  final int createdAt;
  final int trackCount;

  MusicPlaylist({
    required this.id,
    required this.title,
    required this.isPrivate,
    required this.createdAt,
    required this.trackCount,
  });

  factory MusicPlaylist.fromJson(Map<String, dynamic> json) => MusicPlaylist(
        id: json.strOf('id'),
        title: json.strOf('title'),
        isPrivate: json.boolOf('isPrivate'),
        createdAt: json.intOf('createdAt'),
        trackCount: json.intOf('trackCount'),
      );
}

/// Serializes a track back to the musicloud-shaped JSON the save/playlist
/// FFI expects (the same shape `MusicTrack.fromJson` reads).
Map<String, dynamic> trackToJson(MusicTrack t) => {
      'id': t.id,
      'pubkey': t.pubkey,
      'audioUrl': t.audioUrl,
      'blobHash': t.blobHash,
      'mediaSize': t.mediaSize,
      'title': t.title,
      'thumbnail': t.thumbnail,
      'hashtags': t.hashtags,
      'd': t.d,
      'audience': t.audience,
      'createdAt': t.createdAt,
    };

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

/// Hosts a track's audio on this device so peers can fetch it: the blob must
/// live in the local chunk store. Uses the cached/local copy or a LAN peer
/// fetch (which absorbs bytes into the CAS); falls back to downloading the
/// http(s) URL and re-uploading. Returns whether the blob is hosted locally.
Future<bool> hostTrackBlob(
  MusicTrack track,
  MediaService media,
  P2pService p2p,
) async {
  if (track.blobHash.isEmpty) return false;
  final path = await resolveBlobPath(media, p2p, track.blobHash);
  if (path != null) return true;
  final url = track.audioUrl;
  if (url.startsWith('http')) {
    try {
      final file = await media.fetch(url, cacheDir: await media.getCachePath());
      await media.uploadMedia(file);
      return true;
    } catch (_) {}
  }
  return false;
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
