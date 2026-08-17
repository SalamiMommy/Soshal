// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/offthread.dart';
import 'error_log.dart';
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

  /// Publish a track (kind 31022). Returns the event id.
  Future<String> publishTrack({
    required String audioUrl,
    String? title,
    String? thumbnail,
    List<String> hashtags = const [],
    String? audience,
  }) async {
    try {
      final id = await RustLib.instance.api.crateFfiMusicMusicPublish(
        audioUrl: audioUrl,
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
    required this.title,
    required this.thumbnail,
    required this.hashtags,
    required this.d,
    required this.audience,
    required this.createdAt,
  });

  factory MusicTrack.fromJson(Map<String, dynamic> json) => MusicTrack(
        id: json['id'] as String? ?? '',
        pubkey: json['pubkey'] as String? ?? '',
        audioUrl: json['audioUrl'] as String? ?? '',
        title: json['title'] as String? ?? '',
        thumbnail: json['thumbnail'] as String? ?? '',
        hashtags: (json['hashtags'] as List<dynamic>? ?? const [])
            .map((e) => e as String? ?? '')
            .where((e) => e.isNotEmpty)
            .toList(),
        d: json['d'] as String? ?? '',
        audience: json['audience'] as String? ?? 'public',
        createdAt: (json['createdAt'] as num?)?.toInt() ?? 0,
      );
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
        id: json['id'] as String? ?? '',
        pubkey: json['pubkey'] as String? ?? '',
        content: json['textOverlay'] as String? ?? '',
        createdAt: (json['createdAt'] as num?)?.toInt() ?? 0,
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
