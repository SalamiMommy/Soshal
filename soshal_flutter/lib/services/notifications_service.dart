// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Notification Service
/// Fetches, marks, and counts notifications through the Rust bridge.
class NotificationService extends ChangeNotifier
    with LastErrorMixin, ServiceGuard {
  List<AppNotification> _notifications = [];
  List<AppNotification> _unread = [];
  final Map<String, List<AppNotification>> _byType = {};
  final Map<String, DateTime> _byTypeFetchedAt = {};
  int _unreadCount = 0;
  bool _isLoading = false;

  /// Category tab results are considered fresh for this long; switching tabs
  /// within the window skips the refetch (DB query) entirely.
  static const _typeCacheTtl = Duration(seconds: 30);

  List<AppNotification> get notifications => _notifications;
  List<AppNotification> get unread => _unread;
  int get unreadCount => _unreadCount;
  bool get isLoading => _isLoading;

  /// Notifications for a category tab (mention/like/reply/message/follow).
  List<AppNotification> byType(String type) => _byType[type] ?? const [];

  /// Clear all account-scoped state on account switch so Account B never sees
  /// Account A's cached notifications.
  void resetForAccountSwitch() {
    _notifications = [];
    _unread = [];
    _byType.clear();
    _byTypeFetchedAt.clear();
    _unreadCount = 0;
    _isLoading = false;
    clearLastError();
    notifyListeners();
  }

  /// Fetch recent notifications (all types).
  Future<List<AppNotification>> fetchNotifications(String pubkey,
      {int limit = 50}) async {
    _isLoading = true;
    notifyListeners();
    try {
      final json = RustLib.instance.api.crateFfiNotificationsNotificationsFetch(
        userPubkey: pubkey,
        limit: limit,
        offset: 0,
      );
      final parsed = await runOffThreadCompute(parseNotifications, json);
      final capped = parsed.length > 100 ? parsed.sublist(0, 100) : parsed;
      _isLoading = false;
      if (_sameNotifications(_notifications, capped)) {
        notifyListeners();
        return _notifications;
      }
      _notifications = capped;
      clearLastError();
      notifyListeners();
      return _notifications;
    } catch (e, st) {
      _isLoading = false;
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
      final parsed = await runOffThreadCompute(parseNotifications, json);
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

  /// Register the platform push token for the active account.
  Future<bool> registerPush(String userPubkey, String token) => guard(() {
        return RustLib.instance.api
            .crateFfiNotificationsNotificationsRegisterPush(
          userPubkey: userPubkey,
          token: token,
        );
      });

  /// Unregister from push notifications.
  Future<bool> unregisterPush(String userPubkey) => guard(() {
        return RustLib.instance.api
            .crateFfiNotificationsNotificationsUnregisterPush(
          userPubkey: userPubkey,
        );
      });

  /// Mark a single notification as read.
  Future<bool> markRead(String notificationId) => guard(() {
        final ok =
            RustLib.instance.api.crateFfiNotificationsNotificationsMarkRead(
          notificationId: notificationId,
        );
        if (ok) {
          final listed = _notifications.any((n) => n.id == notificationId) ||
              _unread.any((n) => n.id == notificationId);
          _unread.removeWhere((n) => n.id == notificationId);
          for (var i = 0; i < _notifications.length; i++) {
            final n = _notifications[i];
            if (n.id == notificationId && !n.read) {
              _notifications[i] = _asRead(n);
            }
          }
          if (listed) {
            _recomputeUnreadCount();
          } else if (_unreadCount > 0) {
            _unreadCount -= 1;
          }
        }
        return ok;
      });

  /// Mark every notification read for the active account.
  Future<bool> markAllRead(String pubkey) => guard(() {
        final ok =
            RustLib.instance.api.crateFfiNotificationsNotificationsMarkAllRead(
          userPubkey: pubkey,
        );
        if (ok) {
          if (_notifications.any((n) => !n.read)) {
            _notifications = [
              for (final n in _notifications)
                if (!n.read) _asRead(n) else n,
            ];
          }
          _unread = [];
          // Read-state changed everywhere; next category fetch must not be
          // served from the TTL cache.
          _byTypeFetchedAt.clear();
          _recomputeUnreadCount();
        }
        return ok;
      });

  /// Get the unread notification count.
  Future<int> refreshUnreadCount(String pubkey) => guard(() {
        final count = RustLib.instance.api
            .crateFfiNotificationsNotificationsGetUnreadCount(
          userPubkey: pubkey,
        );
        if (_unreadCount != count) {
          _unreadCount = count;
          notifyListeners();
        }
        return _unreadCount;
      }, notifyOnSuccess: false);

  /// Delete a notification from the local store.
  Future<bool> deleteNotification(String notificationId) => guard(() {
        final ok =
            RustLib.instance.api.crateFfiNotificationsNotificationsDelete(
          notificationId: notificationId,
        );
        if (ok) {
          _notifications.removeWhere((n) => n.id == notificationId);
          _unread.removeWhere((n) => n.id == notificationId);
          for (final list in _byType.values) {
            list.removeWhere((n) => n.id == notificationId);
          }
          _recomputeUnreadCount();
        }
        return ok;
      });

  /// Ignore/dismiss a single notification.
  Future<void> ignoreNotification(String notificationId) async {
    await deleteNotification(notificationId);
  }

  /// Ignore all notifications from a user.
  Future<bool> ignoreUser(String targetPubkey, {String? userPubkey}) async {
    return guard(() {
      var ok = userPubkey == null || userPubkey.isEmpty;
      if (!ok) {
        ok = RustLib.instance.api.crateFfiNotificationsNotificationsIgnoreUser(
          userPubkey: userPubkey,
          fromPubkey: targetPubkey,
        );
      }
      if (ok) {
        _notifications.removeWhere((n) => n.fromPubkey == targetPubkey);
        _unread.removeWhere((n) => n.fromPubkey == targetPubkey);
        for (final list in _byType.values) {
          list.removeWhere((n) => n.fromPubkey == targetPubkey);
        }
        _recomputeUnreadCount();
      }
      return ok;
    });
  }

  /// Turn off notifications for a thread/post.
  Future<bool> ignoreThread(String eventId, {String? userPubkey}) async {
    return guard(() {
      var ok = userPubkey == null || userPubkey.isEmpty;
      if (!ok) {
        ok =
            RustLib.instance.api.crateFfiNotificationsNotificationsIgnoreThread(
          userPubkey: userPubkey,
          eventId: eventId,
        );
      }
      if (ok) {
        _notifications.removeWhere((n) => n.eventId == eventId);
        _unread.removeWhere((n) => n.eventId == eventId);
        for (final list in _byType.values) {
          list.removeWhere((n) => n.eventId == eventId);
        }
        _recomputeUnreadCount();
      }
      return ok;
    });
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
      final list = await runOffThreadCompute(parseNotifications, json);
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

  AppNotification _asRead(AppNotification n) => AppNotification(
        id: n.id,
        notificationType: n.notificationType,
        fromPubkey: n.fromPubkey,
        fromName: n.fromName,
        fromAvatar: n.fromAvatar,
        contentPreview: n.contentPreview,
        eventId: n.eventId,
        createdAt: n.createdAt,
        read: true,
        actionUrl: n.actionUrl,
      );

  void _recomputeUnreadCount() {
    var count = _notifications.where((n) => !n.read).length;
    if (_unread.isNotEmpty) {
      final listed = _notifications.map((n) => n.id).toSet();
      count += _unread.where((n) => !listed.contains(n.id)).length;
    }
    _unreadCount = count;
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
  return List<AppNotification>.generate(
    decoded.length,
    (i) => AppNotification.fromJson(decoded[i] as Map<String, dynamic>),
    growable: true,
  );
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
