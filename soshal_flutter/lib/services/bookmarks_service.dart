import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'feed_service.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Bookmarks Service
/// Local bookmark storage plus post resolution from the local DB cache.
class BookmarksService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify, ServiceGuard {
  List<BookmarkRow> _bookmarks = [];

  List<BookmarkRow> get bookmarks => _bookmarks;

  void resetForAccountSwitch() {
    _bookmarks = [];
    clearLastError();
    notifyListeners();
  }

  /// Save a bookmark for an event. Returns the bookmark id.
  Future<String> save(String pubkey, String eventId) => guard(() {
        return RustLib.instance.api.crateFfiBookmarksBookmarksSave(
          pubkey: pubkey,
          eventId: eventId,
        );
      }, onNotify: notifyDeferred);

  /// List bookmarks for a pubkey, newest first.
  Future<List<BookmarkRow>> list(String pubkey,
          {int limit = 100, int offset = 0}) =>
      guard(() {
        final json = RustLib.instance.api.crateFfiBookmarksBookmarksList(
          pubkey: pubkey,
          limit: limit,
          offset: offset,
        );
        final decoded = jsonDecode(json);
        _bookmarks = decoded is List
            ? List<BookmarkRow>.generate(
                decoded.length,
                (i) => BookmarkRow.fromJson(decoded[i] as Map<String, dynamic>),
                growable: true,
              )
            : <BookmarkRow>[];
        return _bookmarks;
      }, onNotify: notifyDeferred);

  /// Delete a bookmark by id.
  Future<bool> delete(String id) => guard(() {
        return RustLib.instance.api.crateFfiBookmarksBookmarksDelete(
          id: id,
        );
      }, onNotify: notifyDeferred);

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
        if (value is Map<String, dynamic>) {
          out[entry.key] = FeedPost.fromJson(value);
        } else if (value is Map) {
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
