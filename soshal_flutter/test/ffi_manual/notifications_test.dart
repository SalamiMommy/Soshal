// Manual ffi tests for notifications
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/ffi/notifications.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-ffi-notifications-manual');
  final api = env.$1;

  test('notificationsFetchUnread and notificationsFetch', () {
    api.stubString('crateFfiNotificationsNotificationsFetchUnread', '[]');
    api.stubString('crateFfiNotificationsNotificationsFetch', '[]');
    final u = notificationsFetchUnread(userPubkey: 'u', limit: 5);
    final all = notificationsFetch(userPubkey: 'u', limit: 5, offset: 0);
    expect(u, '[]');
    expect(all, '[]');
    expect(api.callCount('crateFfiNotificationsNotificationsFetchUnread'), 1);
    expect(api.callCount('crateFfiNotificationsNotificationsFetch'), 1);
  });

  test('mark read and unread count and register push', () {
    api.stubBool('crateFfiNotificationsNotificationsMarkRead', true);
    api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 3);
    api.stubBool('crateFfiNotificationsNotificationsRegisterPush', true);
    final ok = notificationsMarkRead(notificationId: 'n');
    final cnt = notificationsGetUnreadCount(userPubkey: 'u');
    final reg = notificationsRegisterPush(userPubkey: 'u', token: 't');
    expect(ok, true);
    expect(cnt, 3);
    expect(reg, true);
    expect(api.callCount('crateFfiNotificationsNotificationsMarkRead'), 1);
    expect(api.callCount('crateFfiNotificationsNotificationsGetUnreadCount'), 1);
    expect(api.callCount('crateFfiNotificationsNotificationsRegisterPush'), 1);
  });
}
