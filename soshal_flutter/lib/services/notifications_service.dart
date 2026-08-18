// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';

/// Notification Service
/// Fetches, marks, and counts notifications through the Rust bridge.
class NotificationService extends ChangeNotifier with LastErrorMixin {
  List<AppNotification> _notifications = [];
  List<AppNotification> _unread = [];
  final Map<String, List<AppNotification>> _byType = {};
  final Map<String, DateTime> _byTypeFetchedAt = {};
  int _unreadCount = 0;

  /// Category tab results are considered fresh for this long; switching tabs
  /// within the window skips the refetch (DB query) entirely.
  static const _typeCacheTtl = Duration(seconds: 30);

  List<AppNotification> get notifications => _notifications;
  List<AppNotification> get unread => _unread;
  int get unreadCount => _unreadCount;

  /// Notifications for a category tab (mention/like/reply/message/follow).
  List<AppNotification> byType(String type) => _byType[type] ?? const [];

  /// Fetch recent notifications (all types).
  Future<List<AppNotification>> fetchNotifications(String pubkey,
      {int limit = 50}) async {
    try {
      final json = RustLib.instance.api.crateFfiNotificationsNotificationsFetch(
        userPubkey: pubkey,
        limit: limit,
        offset: 0,
      );
      final parsed = await runOffThread(() => parseNotifications(json));
      final capped = parsed.length > 100 ? parsed.sublist(0, 100) : parsed;
      if (_sameNotifications(_notifications, capped)) return _notifications;
      _notifications = capped;
      clearLastError();
      notifyListeners();
      return _notifications;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Fetch only unread notifications.
  Future<List<AppNotification>> fetchUnread(String pubkey,
      {int limit = 50}) async {
    try {
      final json =
          RustLib.instance.api.crateFfiNotificationsNotificationsFetchUnread(
        userPubkey: pubkey,
        limit: limit,
      );
      final parsed = await runOffThread(() => parseNotifications(json));
      if (_sameNotifications(_unread, parsed)) return _unread;
      _unread = parsed;
      clearLastError();
      notifyListeners();
      return _unread;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Fetch notifications of a specific type (mentions/reactions/replies/follows).
  Future<List<AppNotification>> fetchByType(
      String pubkey, String type, int limit) async {
    return _fetchCategory(
        pubkey,
        type,
        () =>
            RustLib.instance.api.crateFfiNotificationsNotificationsFetchByType(
              userPubkey: pubkey,
              notificationType: type,
              limit: limit,
            ));
  }

  /// Fetch mentions.
  Future<List<AppNotification>> fetchMentions(String pubkey,
      {int limit = 50}) async {
    return _fetchCategory(
      pubkey,
      'mention',
      () =>
          RustLib.instance.api.crateFfiNotificationsNotificationsFetchMentions(
        userPubkey: pubkey,
        limit: limit,
      ),
    );
  }

  /// Fetch likes/reactions.
  Future<List<AppNotification>> fetchReactions(String pubkey,
      {int limit = 50}) async {
    return _fetchCategory(
      pubkey,
      'like',
      () =>
          RustLib.instance.api.crateFfiNotificationsNotificationsFetchReactions(
        userPubkey: pubkey,
        limit: limit,
      ),
    );
  }

  /// Fetch replies.
  Future<List<AppNotification>> fetchReplies(String pubkey,
      {int limit = 50}) async {
    return _fetchCategory(
      pubkey,
      'reply',
      () => RustLib.instance.api.crateFfiNotificationsNotificationsFetchReplies(
        userPubkey: pubkey,
        limit: limit,
      ),
    );
  }

  /// Fetch messages (new DMs).
  Future<List<AppNotification>> fetchMessages(String pubkey,
      {int limit = 50}) async {
    return _fetchCategory(
      pubkey,
      'message',
      () =>
          RustLib.instance.api.crateFfiNotificationsNotificationsFetchMessages(
        userPubkey: pubkey,
        limit: limit,
      ),
    );
  }

  /// Fetch follows.
  Future<List<AppNotification>> fetchFollows(String pubkey,
      {int limit = 50}) async {
    return _fetchCategory(
      pubkey,
      'follow',
      () => RustLib.instance.api.crateFfiNotificationsNotificationsFetchFollows(
        userPubkey: pubkey,
        limit: limit,
      ),
    );
  }

  /// Register the platform push token for the active account.
  Future<bool> registerPush(String userPubkey, String token) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiNotificationsNotificationsRegisterPush(
        userPubkey: userPubkey,
        token: token,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Unregister from push notifications.
  Future<bool> unregisterPush(String userPubkey) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiNotificationsNotificationsUnregisterPush(
        userPubkey: userPubkey,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Mark a single notification as read.
  Future<bool> markRead(String notificationId) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiNotificationsNotificationsMarkRead(
        notificationId: notificationId,
      );
      if (ok) {
        _unreadCount = _unreadCount > 0 ? _unreadCount - 1 : 0;
        _unread.removeWhere((n) => n.id == notificationId);
      }
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Mark every notification read for the active account.
  Future<bool> markAllRead(String pubkey) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiNotificationsNotificationsMarkAllRead(
        userPubkey: pubkey,
      );
      if (ok) {
        _unreadCount = 0;
        // Read-state changed everywhere; next category fetch must not be
        // served from the TTL cache.
        _byTypeFetchedAt.clear();
      }
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Get the unread notification count.
  Future<int> refreshUnreadCount(String pubkey) async {
    try {
      _unreadCount =
          RustLib.instance.api.crateFfiNotificationsNotificationsGetUnreadCount(
        userPubkey: pubkey,
      );
      clearLastError();
      notifyListeners();
      return _unreadCount;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Delete a notification from the local store.
  Future<bool> deleteNotification(String notificationId) async {
    try {
      final ok = RustLib.instance.api.crateFfiNotificationsNotificationsDelete(
        notificationId: notificationId,
      );
      if (ok) {
        _notifications.removeWhere((n) => n.id == notificationId);
        _unread.removeWhere((n) => n.id == notificationId);
        for (final list in _byType.values) {
          list.removeWhere((n) => n.id == notificationId);
        }
      }
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<List<AppNotification>> _fetchCategory(
    String pubkey,
    String type,
    String Function() call,
  ) async {
    // Per-type TTL: skip the DB query when this tab was loaded recently and
    // nothing invalidated it (markAllRead clears the cache).
    final fetchedAt = _byTypeFetchedAt[type];
    if (fetchedAt != null &&
        DateTime.now().difference(fetchedAt) < _typeCacheTtl &&
        (_byType[type]?.isNotEmpty ?? false)) {
      return _byType[type]!;
    }
    try {
      final json = call();
      final list = await runOffThread(() => parseNotifications(json));
      if (_sameNotifications(_byType[type] ?? const [], list)) {
        _byTypeFetchedAt[type] = DateTime.now();
        return _byType[type]!;
      }
      _byType[type] = list;
      _byTypeFetchedAt[type] = DateTime.now();
      clearLastError();
      notifyListeners();
      return list;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Whether two notification lists are identical for UI purposes (same
  /// ids in the same order with the same read state). Gates `notifyListeners`
  /// so a no-change refetch doesn't rebuild every subscriber.
  bool _sameNotifications(List<AppNotification> a, List<AppNotification> b) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (a[i].id != b[i].id || a[i].read != b[i].read) return false;
    }
    return true;
  }
}

List<AppNotification> parseNotifications(String json) {
  final decoded = jsonDecode(json) as List<dynamic>;
  return decoded
      .map((e) => AppNotification.fromJson(e as Map<String, dynamic>))
      .toList();
}

/// A notification item from the local store.
class AppNotification {
  final String id;
  final String notificationType;
  final String fromPubkey;
  final String fromName;
  final String fromAvatar;
  final String contentPreview;
  final String? eventId;
  final int createdAt;
  final bool read;
  final String actionUrl;

  AppNotification({
    required this.id,
    required this.notificationType,
    required this.fromPubkey,
    required this.fromName,
    required this.fromAvatar,
    required this.contentPreview,
    this.eventId,
    required this.createdAt,
    required this.read,
    required this.actionUrl,
  });

  factory AppNotification.fromJson(Map<String, dynamic> json) {
    return AppNotification(
      id: json.strOf('id'),
      notificationType: json.strOf('notification_type'),
      fromPubkey: json.strOf('from_pubkey'),
      fromName: json.strOf('from_name'),
      fromAvatar: json.strOf('from_avatar'),
      contentPreview: json.strOf('content_preview'),
      eventId: json.strOrNull('event_id'),
      createdAt: json.intOf('created_at'),
      read: json.boolOf('read'),
      actionUrl: json.strOf('action_url'),
    );
  }
}
