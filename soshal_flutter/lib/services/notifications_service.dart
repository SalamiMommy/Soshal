// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Notification Service
/// Fetches, marks, and counts notifications through the Rust bridge.
class NotificationService extends ChangeNotifier with LastErrorMixin {
  List<AppNotification> _notifications = [];
  List<AppNotification> _unread = [];
  final Map<String, List<AppNotification>> _byType = {};
  int _unreadCount = 0;

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
      final parsed = parseNotifications(json);
      _notifications = parsed.length > 100 ? parsed.sublist(0, 100) : parsed;
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
      _unread = parseNotifications(json);
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
    try {
      final json =
          RustLib.instance.api.crateFfiNotificationsNotificationsFetchByType(
        userPubkey: pubkey,
        notificationType: type,
        limit: limit,
      );
      final parsed = parseNotifications(json);
      _notifications = parsed;
      _byType[type] = parsed;
      clearLastError();
      notifyListeners();
      return _notifications;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
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
    try {
      final list = parseNotifications(call());
      _byType[type] = list;
      clearLastError();
      notifyListeners();
      return list;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
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
      id: json['id'] as String? ?? '',
      notificationType: json['notification_type'] as String? ?? '',
      fromPubkey: json['from_pubkey'] as String? ?? '',
      fromName: json['from_name'] as String? ?? '',
      fromAvatar: json['from_avatar'] as String? ?? '',
      contentPreview: json['content_preview'] as String? ?? '',
      eventId: json['event_id'] as String?,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
      read: json['read'] as bool? ?? false,
      actionUrl: json['action_url'] as String? ?? '',
    );
  }
}
