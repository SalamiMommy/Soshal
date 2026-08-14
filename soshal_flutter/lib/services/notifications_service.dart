// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Notification Service
/// Fetches, marks, and counts notifications through the Rust bridge.
class NotificationService extends ChangeNotifier {
  List<AppNotification> _notifications = [];
  int _unreadCount = 0;
  String? _lastError;

  List<AppNotification> get notifications => _notifications;
  int get unreadCount => _unreadCount;
  String? get lastError => _lastError;

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
      _lastError = null;
      notifyListeners();
      return _notifications;
    } catch (e) {
      _lastError = e.toString();
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
      _notifications = parseNotifications(json);
      _lastError = null;
      notifyListeners();
      return _notifications;
    } catch (e) {
      _lastError = e.toString();
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
      _notifications = parseNotifications(json);
      _lastError = null;
      notifyListeners();
      return _notifications;
    } catch (e) {
      _lastError = e.toString();
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
      }
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
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
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
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
      _lastError = null;
      notifyListeners();
      return _unreadCount;
    } catch (e) {
      _lastError = e.toString();
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
      }
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
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
