// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/notifications_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

String notifJson(String id, String type, {bool read = false}) {
  return '{"id":"$id","notification_type":"$type",'
      '"from_pubkey":"pk-$id","from_name":"Name $id",'
      '"from_avatar":"av-$id","content_preview":"hello $id",'
      '"event_id":"ev-$id","created_at":1700000000,"read":$read,'
      '"action_url":"soshal://n/$id"}';
}

void main() {
  final env = bootstrapTestEnv('test-notifications');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('NotificationService', () {
    test('fetchNotifications parses list and caps at 100', () async {
      final notif = NotificationService();
      var notified = 0;
      notif.addListener(() => notified++);
      final items =
          List.generate(120, (i) => notifJson('n-$i', 'reaction'));
      api.stubString(
        'crateFfiNotificationsNotificationsFetch',
        '[${items.join(',')}]',
      );

      final list = await notif.fetchNotifications('me', limit: 80);
      expect(list.length, 100, reason: 'capped at 100');
      expect(notif.notifications.length, 100);
      expect(notif.notifications.first.id, 'n-0');
      expect(notif.notifications.last.id, 'n-99');
      expect(notif.notifications.first.notificationType, 'reaction');
      expect(notif.notifications.first.fromPubkey, 'pk-n-0');
      expect(notif.notifications.first.eventId, 'ev-n-0');
      expect(notif.notifications.first.actionUrl, 'soshal://n/n-0');
      expect(notified, 2);

      final inv = api
          .callsOf('crateFfiNotificationsNotificationsFetch')
          .single;
      expect(api.namedArg(inv, 'userPubkey'), 'me');
      expect(api.namedArg(inv, 'limit'), 80);
      expect(api.namedArg(inv, 'offset'), 0);
    });

    test('fetchUnread and fetchByType parse and pass type', () async {
      final notif = NotificationService();
      api.stubString(
        'crateFfiNotificationsNotificationsFetchUnread',
        '[${notifJson('u-1', 'mention', read: false)}]',
      );
      api.stubString(
        'crateFfiNotificationsNotificationsFetchByType',
        '[${notifJson('t-1', 'follow')}]',
      );

      final unread = await notif.fetchUnread('me');
      expect(unread.single.id, 'u-1');
      expect(unread.single.read, isFalse);

      final typed = await notif.fetchByType('me', 'follow', 10);
      expect(typed.single.notificationType, 'follow');

      var inv = api
          .callsOf('crateFfiNotificationsNotificationsFetchUnread')
          .single;
      expect(api.namedArg(inv, 'limit'), 50);
      inv = api
          .callsOf('crateFfiNotificationsNotificationsFetchByType')
          .single;
      expect(api.namedArg(inv, 'notificationType'), 'follow');
      expect(api.namedArg(inv, 'limit'), 10);
    });

    test('refreshUnreadCount stores count from FFI', () async {
      final notif = NotificationService();
      api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 7);

      expect(await notif.refreshUnreadCount('me'), 7);
      expect(notif.unreadCount, 7);
      final inv = api
          .callsOf('crateFfiNotificationsNotificationsGetUnreadCount')
          .single;
      expect(api.namedArg(inv, 'userPubkey'), 'me');
    });

    test('markRead decrements unread count and passes id', () async {
      final notif = NotificationService();
      api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 5);
      await notif.refreshUnreadCount('me');
      api.stubBool('crateFfiNotificationsNotificationsMarkRead', true);

      expect(await notif.markRead('n-1'), isTrue);
      expect(notif.unreadCount, 4);
      final inv =
          api.callsOf('crateFfiNotificationsNotificationsMarkRead').single;
      expect(api.namedArg(inv, 'notificationId'), 'n-1');
    });

    test('markAllRead resets count to zero', () async {
      final notif = NotificationService();
      api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 9);
      await notif.refreshUnreadCount('me');
      api.stubBool('crateFfiNotificationsNotificationsMarkAllRead', true);

      expect(await notif.markAllRead('me'), isTrue);
      expect(notif.unreadCount, 0);
      final inv = api
          .callsOf('crateFfiNotificationsNotificationsMarkAllRead')
          .single;
      expect(api.namedArg(inv, 'userPubkey'), 'me');
    });

    test('deleteNotification removes item from list', () async {
      final notif = NotificationService();
      api.stubString(
        'crateFfiNotificationsNotificationsFetch',
        '[${notifJson('n-1', 'reaction')},${notifJson('n-2', 'reply')}]',
      );
      await notif.fetchNotifications('me');
      api.stubBool('crateFfiNotificationsNotificationsDelete', true);

      expect(await notif.deleteNotification('n-1'), isTrue);
      expect(notif.notifications.single.id, 'n-2');
      final inv = api
          .callsOf('crateFfiNotificationsNotificationsDelete')
          .single;
      expect(api.namedArg(inv, 'notificationId'), 'n-1');
    });

    test('fetch error sets lastError and rethrows', () async {
      final notif = NotificationService();
      api.stub('crateFfiNotificationsNotificationsFetch',
          (_) => throw Exception('notif down'));

      await expectLater(notif.fetchNotifications('me'), throwsException);
      expect(notif.lastError, contains('notif down'));
    });

    test('markRead failure sets lastError and keeps count', () async {
      final notif = NotificationService();
      api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 3);
      await notif.refreshUnreadCount('me');
      api.stub('crateFfiNotificationsNotificationsMarkRead',
          (_) => throw Exception('db locked'));

      await expectLater(notif.markRead('n-1'), throwsException);
      expect(notif.lastError, contains('db locked'));
      expect(notif.unreadCount, 3);
    });

    test('watchNotifications parses streamed notifications', () async {
      final notif = NotificationService();
      final controller = StreamController<String>();
      addTearDown(controller.close);

      api.stub(
        'crateFfiNotificationsNotificationsWatch',
        (_) => controller.stream,
      );

      final stream = notif.watchNotifications('me', limit: 20);
      final emissions = <List<AppNotification>>[];
      final sub = stream.listen(emissions.add);
      addTearDown(sub.cancel);

      controller.add('[${notifJson('s-1', 'mention', read: false)}]');
      await pumpEventQueue();

      expect(emissions.length, 1);
      expect(emissions[0].single.id, 's-1');
      expect(emissions[0].single.notificationType, 'mention');
    });

    test('subscribeToNotifications updates notifications and unreadCount live', () async {
      final notif = NotificationService();
      final controller = StreamController<String>();
      addTearDown(controller.close);

      api.stub(
        'crateFfiNotificationsNotificationsWatch',
        (_) => controller.stream,
      );

      var notified = 0;
      notif.addListener(() => notified++);

      notif.subscribeToNotifications('me', limit: 50);

      controller.add(
        '[${notifJson('live-1', 'like', read: false)}, ${notifJson('live-2', 'reply', read: true)}]',
      );
      await pumpEventQueue();

      expect(notif.notifications.length, 2);
      expect(notif.unread.length, 1);
      expect(notif.unreadCount, 1);
      expect(notified, greaterThanOrEqualTo(1));

      // Account switch should clean up subscription and clear lists
      notif.resetForAccountSwitch();
      expect(notif.notifications.isEmpty, true);
      expect(notif.unreadCount, 0);

      controller.add('[${notifJson('live-3', 'like', read: false)}]');
      await pumpEventQueue();
      // Should remain empty because subscription was cancelled
      expect(notif.notifications.isEmpty, true);
    });
  });
}
