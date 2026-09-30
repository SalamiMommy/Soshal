// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert' show jsonDecode;
import 'package:flutter/foundation.dart' show listEquals;
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

    // ─── value equality (6.3) ─────────────────────────────────────────────
    //
    // `notifications_screen.dart` gates its rebuild on
    // `Selector.shouldRebuild: (a, b) => !listEquals(a, b)`, so these pin what
    // that comparison is actually able to see.

    test('two notifications with identical content are equal', () {
      final a = AppNotification.fromJson(
          jsonDecode(notifJson('n-1', 'like')) as Map<String, dynamic>);
      final b = AppNotification.fromJson(
          jsonDecode(notifJson('n-1', 'like')) as Map<String, dynamic>);

      // Two separate parse passes -- exactly what the watch stream produces on
      // every tick.
      expect(identical(a, b), isFalse);
      expect(a, equals(b));
      expect(a.hashCode, equals(b.hashCode));
      expect(listEquals([a], [b]), isTrue,
          reason: 'this is the comparison the Selector makes');
    });

    test('every field participates in equality', () {
      // A field missing from `operator ==` is a change the Selector would
      // silently refuse to rebuild for, and the row count does not change, so no
      // list-length assertion would ever catch it. Hence one case per field.
      final base = AppNotification.fromJson(
          jsonDecode(notifJson('n-1', 'like')) as Map<String, dynamic>);

      final variants = <String, AppNotification>{
        'id': AppNotification(
            id: 'other', notificationType: base.notificationType,
            fromPubkey: base.fromPubkey, fromName: base.fromName,
            fromAvatar: base.fromAvatar,
            contentPreview: base.contentPreview, eventId: base.eventId,
            createdAt: base.createdAt, read: base.read, actionUrl: base.actionUrl),
        'notificationType': AppNotification(
            id: base.id, notificationType: 'other',
            fromPubkey: base.fromPubkey, fromName: base.fromName,
            fromAvatar: base.fromAvatar,
            contentPreview: base.contentPreview, eventId: base.eventId,
            createdAt: base.createdAt, read: base.read, actionUrl: base.actionUrl),
        'fromPubkey': AppNotification(
            id: base.id, notificationType: base.notificationType,
            fromPubkey: 'other', fromName: base.fromName,
            fromAvatar: base.fromAvatar,
            contentPreview: base.contentPreview, eventId: base.eventId,
            createdAt: base.createdAt, read: base.read, actionUrl: base.actionUrl),
        'fromName': AppNotification(
            id: base.id, notificationType: base.notificationType,
            fromPubkey: base.fromPubkey, fromName: 'other',
            fromAvatar: base.fromAvatar,
            contentPreview: base.contentPreview, eventId: base.eventId,
            createdAt: base.createdAt, read: base.read, actionUrl: base.actionUrl),
        'fromAvatar': AppNotification(
            id: base.id, notificationType: base.notificationType,
            fromPubkey: base.fromPubkey, fromName: base.fromName,
            fromAvatar: 'other', contentPreview: base.contentPreview,
            eventId: base.eventId, createdAt: base.createdAt, read: base.read,
            actionUrl: base.actionUrl),
        'contentPreview': AppNotification(
            id: base.id, notificationType: base.notificationType,
            fromPubkey: base.fromPubkey, fromName: base.fromName,
            fromAvatar: base.fromAvatar, contentPreview: 'other',
            eventId: base.eventId, createdAt: base.createdAt, read: base.read,
            actionUrl: base.actionUrl),
        'eventId': AppNotification(
            id: base.id, notificationType: base.notificationType,
            fromPubkey: base.fromPubkey, fromName: base.fromName,
            fromAvatar: base.fromAvatar,
            contentPreview: base.contentPreview, eventId: 'other',
            createdAt: base.createdAt, read: base.read, actionUrl: base.actionUrl),
        'createdAt': AppNotification(
            id: base.id, notificationType: base.notificationType,
            fromPubkey: base.fromPubkey, fromName: base.fromName,
            fromAvatar: base.fromAvatar,
            contentPreview: base.contentPreview, eventId: base.eventId,
            createdAt: 1, read: base.read, actionUrl: base.actionUrl),
        'read': AppNotification(
            id: base.id, notificationType: base.notificationType,
            fromPubkey: base.fromPubkey, fromName: base.fromName,
            fromAvatar: base.fromAvatar,
            contentPreview: base.contentPreview, eventId: base.eventId,
            createdAt: base.createdAt, read: !base.read,
            actionUrl: base.actionUrl),
        'actionUrl': AppNotification(
            id: base.id, notificationType: base.notificationType,
            fromPubkey: base.fromPubkey, fromName: base.fromName,
            fromAvatar: base.fromAvatar,
            contentPreview: base.contentPreview, eventId: base.eventId,
            createdAt: base.createdAt, read: base.read, actionUrl: 'other'),
      };

      for (final entry in variants.entries) {
        expect(entry.value, isNot(equals(base)),
            reason: 'a change to ${entry.key} must be visible to the Selector');
        expect(listEquals([base], [entry.value]), isFalse,
            reason: 'the Selector must rebuild when ${entry.key} changes');
      }
    });

    test('a watch tick with unchanged content notifies nobody', () async {
      final notif = NotificationService();
      final controller = StreamController<String>();
      addTearDown(controller.close);
      api.stub('crateFfiNotificationsNotificationsWatch',
          (_) => controller.stream);

      var notified = 0;
      notif.addListener(() => notified++);
      notif.subscribeToNotifications('me', limit: 50);

      final payload = '[${notifJson('live-1', 'like')}]';
      controller.add(payload);
      await pumpEventQueue();
      expect(notified, 1, reason: 'the first tick is real content');
      final first = notif.notifications;

      // Same payload, re-sent. The stream re-parses into fresh instances, so
      // without the guard the Selector compared identity and rebuilt a list
      // whose contents had not moved at all.
      controller.add(payload);
      await pumpEventQueue();
      controller.add(payload);
      await pumpEventQueue();

      expect(notified, 1,
          reason: 'identical content on the watch stream is not a change');
      expect(identical(notif.notifications, first), isTrue,
          reason: 'and the list itself is left alone');
      expect(notif.unreadCount, 1);
    });

    test('a watch tick that changes one field still notifies', () async {
      final notif = NotificationService();
      final controller = StreamController<String>();
      addTearDown(controller.close);
      api.stub('crateFfiNotificationsNotificationsWatch',
          (_) => controller.stream);

      var notified = 0;
      notif.addListener(() => notified++);
      notif.subscribeToNotifications('me', limit: 50);

      controller.add('[${notifJson('live-1', 'like')}]');
      await pumpEventQueue();
      expect(notified, 1);

      // Same id, same read state, edited body. An equality check that compared
      // only id + read -- which is what `_sameNotifications` used to do -- would
      // swallow this and leave the row showing stale text.
      controller.add(
        '[{"id":"live-1","notification_type":"like",'
        '"from_pubkey":"pk-live-1","from_name":"Renamed",'
        '"from_avatar":"av-live-1","content_preview":"edited",'
        '"event_id":"ev-live-1","created_at":1700000000,"read":false,'
        '"action_url":"soshal://n/live-1"}]',
      );
      await pumpEventQueue();

      expect(notified, 2,
          reason: 'a content edit is a change, id and read state or not');
      expect(notif.lastError, isNull,
          reason: 'the payload was a well-formed list, so this is a real '
              'content change and not the parse-error path');
      expect(notif.notifications.single.fromName, 'Renamed');
      expect(notif.notifications.single.contentPreview, 'edited');
    });

    test('a watch tick that only reorders is a change, and a shrink too',
        () async {
      final notif = NotificationService();
      final controller = StreamController<String>();
      addTearDown(controller.close);
      api.stub('crateFfiNotificationsNotificationsWatch',
          (_) => controller.stream);

      var notified = 0;
      notif.addListener(() => notified++);
      notif.subscribeToNotifications('me', limit: 50);

      controller
        ..add('[${notifJson('a', 'like')}, ${notifJson('b', 'reply')}]')
        ..add('[${notifJson('b', 'reply')}, ${notifJson('a', 'like')}]');
      await pumpEventQueue();
      expect(notified, 2,
          reason: 'position is content here -- the list is ordered newest-first');

      controller.add('[${notifJson('a', 'like')}]');
      await pumpEventQueue();
      expect(notified, 3, reason: 'a shorter list is a change');
    });
  });
}
