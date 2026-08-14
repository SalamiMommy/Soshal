// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'feed_service.dart';
import 'error_log.dart';

/// Bookmarks Service
/// Local bookmark storage plus post resolution from the local DB cache.
class BookmarksService extends ChangeNotifier with LastErrorMixin {
  List<BookmarkRow> _bookmarks = [];

  List<BookmarkRow> get bookmarks => _bookmarks;

  /// Save a bookmark for an event. Returns the bookmark id.
  Future<String> save(String pubkey, String eventId) async {
    try {
      final id = RustLib.instance.api.crateFfiBookmarksBookmarksSave(
        pubkey: pubkey,
        eventId: eventId,
      );
      _lastError = null;
      notifyListeners();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// List bookmarks for a pubkey, newest first.
  Future<List<BookmarkRow>> list(String pubkey,
      {int limit = 100, int offset = 0}) async {
    try {
      final json = RustLib.instance.api.crateFfiBookmarksBookmarksList(
        pubkey: pubkey,
        limit: limit,
        offset: offset,
      );
      final decoded = jsonDecode(json);
      _bookmarks = decoded is List
          ? decoded
              .map((e) => BookmarkRow.fromJson(e as Map<String, dynamic>))
              .toList()
          : <BookmarkRow>[];
      _lastError = null;
      notifyListeners();
      return _bookmarks;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Delete a bookmark by id.
  Future<bool> delete(String id) async {
    try {
      final removed = RustLib.instance.api.crateFfiBookmarksBookmarksDelete(
        id: id,
      );
      _lastError = null;
      notifyListeners();
      return removed;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Resolve a bookmarked event from the local DB cache.
  /// Returns null when the post is not cached locally.
  Future<FeedPost?> resolvePost(String eventId) async {
    try {
      final json = RustLib.instance.api.crateFfiBookmarksBookmarksResolvePost(
        eventId: eventId,
      );
      if (json.isEmpty) return null;
      final decoded = jsonDecode(json);
      if (decoded is! Map<String, dynamic>) return null;
      return FeedPost.fromJson(decoded);
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return null;
    }
  }
}

/// A saved bookmark row.
class BookmarkRow {
  final String id;
  final String pubkey;
  final String eventId;
  final int createdAt;

  BookmarkRow({
    required this.id,
    required this.pubkey,
    required this.eventId,
    required this.createdAt,
  });

  factory BookmarkRow.fromJson(Map<String, dynamic> json) {
    return BookmarkRow(
      id: json['id'] as String? ?? '',
      pubkey: json['pubkey'] as String? ?? '',
      eventId: json['event_id'] as String? ?? '',
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}
