import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'feed_service.dart';
import 'error_log.dart';

/// Bookmarks Service
/// Local bookmark storage plus post resolution from the local DB cache.
class BookmarksService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify {
  List<BookmarkRow> _bookmarks = [];

  List<BookmarkRow> get bookmarks => _bookmarks;

  /// Clear all account-scoped state on account switch so Account B never
  /// sees Account A's cached bookmarks.
  void resetForAccountSwitch() {
    _bookmarks = [];
    clearLastError();
    notifyListeners();
  }

  /// Save a bookmark for an event. Returns the bookmark id.
  Future<String> save(String pubkey, String eventId) async {
    try {
      final id = RustLib.instance.api.crateFfiBookmarksBookmarksSave(
        pubkey: pubkey,
        eventId: eventId,
      );
      clearLastError();
      notifyDeferred();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
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
      clearLastError();
      notifyDeferred();
      return _bookmarks;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Delete a bookmark by id.
  Future<bool> delete(String id) async {
    try {
      final removed = RustLib.instance.api.crateFfiBookmarksBookmarksDelete(
        id: id,
      );
      clearLastError();
      notifyDeferred();
      return removed;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Resolve many bookmarked events in one FFI call. Returns a map of
  /// eventId → post for the rows cached locally.
  Future<Map<String, FeedPost>> resolvePosts(List<String> eventIds) async {
    final out = <String, FeedPost>{};
    if (eventIds.isEmpty) return out;
    try {
      final json = RustLib.instance.api.crateFfiBookmarksBookmarksResolvePosts(
        idsJson: jsonEncode(eventIds),
      );
      final decoded = jsonDecode(json) as Map<String, dynamic>;
      for (final entry in decoded.entries) {
        final value = entry.value;
        if (value is Map) {
          out[entry.key] = FeedPost.fromJson(Map<String, dynamic>.from(value));
        }
      }
      clearLastError();
      return out;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return out;
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
      id: json.strOf('id'),
      pubkey: json.strOf('pubkey'),
      eventId: json.strOf('event_id'),
      createdAt: json.intOf('created_at'),
    );
  }
}
