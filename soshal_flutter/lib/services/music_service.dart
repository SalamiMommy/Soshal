// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/blob_resolver.dart';
import '../utils/safe_url.dart';
import '../utils/media_upload.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'audio_service.dart';
import 'error_log.dart';
import 'media_service.dart';
import 'p2p_service.dart';
import 'social_entry.dart';
import '../utils/service_guard.dart';

/// Musicloud: kind-31022 track publishing, fetching, sharing to feed and
/// comment threads. All FFI calls are async — always awaited.
class MusicService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify, ServiceGuard {
  List<MusicTrack> _tracks = [];

  List<MusicTrack> get tracks => _tracks;

  List<MusicTrack> _savedTracks = [];
  List<MusicPlaylist> _playlists = [];

  List<MusicTrack> get savedTracks => _savedTracks;
  List<MusicPlaylist> get playlists => _playlists;

  bool isTrackSaved(String id) => _savedTracks.any((t) => t.id == id);

  /// Clears in-memory saved tracks, playlists, and transient state on account switch.
  void resetForAccountSwitch() {
    _savedTracks = [];
    _playlists = [];
    // `_tracks` is scoped to the active account too — leaving the previous
    // account's fetched stream behind leaks their lib into the new account.
    _tracks = [];
    clearLastError();
    notifyListeners();
  }

  /// Fetch tracks (kind 31022), optionally filtered by author pubkey.
  Future<List<MusicTrack>> fetchTracks({String? author, int limit = 50}) =>
      guard(() async {
        final json = await RustLib.instance.api.crateFfiMusicMusicFetch(
          limit: BigInt.from(limit),
          author: author,
          audience: 'public',
        );
        _tracks = await runOffThreadCompute(_parseTracks, json);
        return _tracks;
      });

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
  }) =>
      guard(() async {
        return await RustLib.instance.api.crateFfiMusicMusicPublish(
          mediaSource: mediaSource,
          title: title,
          thumbnail: thumbnail,
          hashtags: hashtags,
          audience: audience,
        );
      }, notifyOnSuccess: false);

  /// Share a track to the feed as a kind-1 text note. Returns the event id.
  Future<String> shareToFeed({
    required String trackId,
    required String trackPubkey,
    required String trackD,
    required String message,
    List<String> hashtags = const [],
  }) =>
      guard(() async {
        return await RustLib.instance.api.crateFfiMusicMusicShareToFeed(
          trackId: trackId,
          trackPubkey: trackPubkey,
          trackD: trackD,
          message: message,
          hashtags: hashtags,
        );
      }, notifyOnSuccess: false);

  /// Publish a comment on a track. Returns the comment event id.
  Future<String> comment({
    int trackKind = 31022,
    required String trackPubkey,
    required String trackD,
    required String content,
  }) =>
      guard(() async {
        return await RustLib.instance.api.crateFfiMusicMusicComment(
          trackKind: trackKind,
          trackPubkey: trackPubkey,
          trackD: trackD,
          content: content,
        );
      }, notifyOnSuccess: false);

  /// Fetch comments for a track. Returns JSON array of mini event outputs.
  Future<List<TrackComment>> fetchComments({
    int trackKind = 31022,
    required String trackPubkey,
    required String trackD,
  }) =>
      guard(() async {
        final json = await RustLib.instance.api.crateFfiMusicMusicComments(
          trackKind: trackKind,
          trackPubkey: trackPubkey,
          trackD: trackD,
        );
        return await runOffThreadCompute(_parseComments, json);
      }, notifyOnSuccess: false);

  /// Saved tracks from the local `saved_content` store, newest saved first.
  Future<List<MusicTrack>> fetchSavedTracks() => guard(() async {
        final json = RustLib.instance.api.crateFfiMusicMusicSaved();
        _savedTracks = await runOffThreadCompute(_parseTracks, json);
        return _savedTracks;
      });

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
      notifyDeferred();
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
      notifyDeferred();
    }
  }

  /// The user's playlists, newest first.
  Future<List<MusicPlaylist>> fetchPlaylists() => guard(() async {
        final json = RustLib.instance.api.crateFfiMusicMusicPlaylistList();
        _playlists = await runOffThreadCompute(_parsePlaylists, json);
        return _playlists;
      });

  /// Create a playlist. Returns the new playlist id.
  Future<String> createPlaylist({
    required String title,
    required bool isPrivate,
  }) =>
      guard(() async {
        final id = RustLib.instance.api.crateFfiMusicMusicPlaylistCreate(
          title: title,
          isPrivate: isPrivate,
        );
        await fetchPlaylists();
        return id;
      }, notifyOnSuccess: false);

  /// Rename a playlist.
  Future<void> renamePlaylist(String playlistId, String title) async {
    await guard(() async {
      RustLib.instance.api.crateFfiMusicMusicPlaylistRename(
          playlistId: playlistId, title: title);
      await fetchPlaylists();
    }, notifyOnSuccess: false);
  }

  /// Delete a playlist and its tracks.
  Future<void> deletePlaylist(String playlistId) async {
    await guard(() async {
      RustLib.instance.api
          .crateFfiMusicMusicPlaylistDelete(playlistId: playlistId);
      await fetchPlaylists();
    }, notifyOnSuccess: false);
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
      notifyDeferred();
      return false;
    }
  }

  /// Remove a track from a playlist.
  Future<void> removeFromPlaylist(String playlistId, String trackId) async {
    await guard(() {
      RustLib.instance.api.crateFfiMusicMusicPlaylistRemoveTrack(
        playlistId: playlistId,
        trackId: trackId,
      );
    }, notifyOnSuccess: false);
  }

  /// A playlist's tracks in insertion order.
  Future<List<MusicTrack>> fetchPlaylistTracks(String playlistId) =>
      guard(() async {
        final json = RustLib.instance.api
            .crateFfiMusicMusicPlaylistTracks(playlistId: playlistId);
        return await runOffThreadCompute(_parseTracks, json);
      }, notifyOnSuccess: false);
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
        hashtags: json.stringsOf('hashtags'),
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
/// fallback). Blob refs (`blob://`, `n<hash>`, bare hex) are never returned —
/// they are only meaningful to the chunk store, not to the media player.
/// Returns null when nothing reachable is available.
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
      mediaBlobHash(url) == null &&
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
    } catch (e) {
      debugPrint('hostTrackBlob: $e');
      logRuntimeError(e);
    }
  }
  return false;
}

/// Extracts waveform peaks (0..1 RMS bins, 64 default) for a track's audio so
/// the playing bar can draw a scrubber. Local CAS blob first (resolveBlobPath
/// gives a file path; sync-server streams the same file), then a downloaded
/// http(s) copy. Returns an empty list when the audio is unreachable — the
/// bar then renders progress-only instead of a fabricated waveform.
Future<List<double>> extractTrackPeaks(
  MusicTrack track, {
  required MediaService media,
  required P2pService p2p,
}) async {
  final audio = AudioService();
  try {
    if (track.blobHash.isNotEmpty) {
      final path = await resolveBlobPath(media, p2p, track.blobHash);
      if (path != null && File(path).existsSync()) {
        final peaks = await audio.peaksFor(path);
        if (peaks.isNotEmpty) return peaks;
      }
    }
    final url = track.audioUrl;
    if (url.isNotEmpty && SafeUrl.isSafeMediaUrl(url)) {
      final cached =
          await media.fetch(url, cacheDir: await media.getCachePath());
      final peaks = await audio.peaksFor(cached);
      if (peaks.isNotEmpty) return peaks;
    }
  } catch (e) {
    debugPrint('extractTrackPeaks: $e');
    logRuntimeError(e);
  }
  return const [];
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
List<MusicTrack> _parseTracks(String json) {
  final decoded = jsonDecode(json) as List<dynamic>;
  return List<MusicTrack>.generate(
    decoded.length,
    (i) => MusicTrack.fromJson(decoded[i] as Map<String, dynamic>),
    growable: true,
  );
}

/// JSON → [TrackComment] list, top-level so [runOffThread] can decode on a
/// background isolate.
List<TrackComment> _parseComments(String json) {
  final decoded = jsonDecode(json) as List<dynamic>;
  return List<TrackComment>.generate(
    decoded.length,
    (i) => TrackComment.fromJson(decoded[i] as Map<String, dynamic>),
    growable: true,
  );
}

/// JSON → [MusicPlaylist] list, top-level so [runOffThreadCompute] can decode on a
/// background isolate.
List<MusicPlaylist> _parsePlaylists(String json) {
  return (jsonDecode(json) as List<dynamic>)
      .map((e) => MusicPlaylist.fromJson(e as Map<String, dynamic>))
      .toList();
}
