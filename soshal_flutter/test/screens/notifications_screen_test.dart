import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/notifications_screen.dart';
import 'package:soshal_flutter/services/notifications_service.dart';
import 'package:soshal_flutter/services/session_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-notifications');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  const sessionJson =
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,"relay_list":[]}]}';

  const notifJson =
      '[{"id":"n1","notification_type":"like","from_pubkey":"pkx",'
      '"from_name":"Alice","from_avatar":"","content_preview":"liked your post",'
      '"read":false,"created_at":0,"action_url":""}]';

  Future<void> pumpScreen(WidgetTester tester, {String? session}) async {
    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final sessionService = SessionService();
    api.stubString('crateFfiSessionSessionLoad', session ?? sessionJson);
    await sessionService.loadSession();
    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: sessionService),
          ChangeNotifierProvider(create: (_) => NotificationService()),
        ],
        child: const MaterialApp(home: NotificationsScreen()),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('signed out state prompts sign in', (tester) async {
    await pumpScreen(
      tester,
      session: '{"active_pubkey":null,"accounts":[]}',
    );

    expect(find.text('Sign in to see notifications'), findsOneWidget);
  });

  testWidgets('renders tabs, notification row and unread count',
      (tester) async {
    api.stubString('crateFfiNotificationsNotificationsFetch', notifJson);
    api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 3);

    await pumpScreen(tester);

    for (final tab in [
      'All',
      'Mentions',
      'Reactions',
      'Replies',
      'Messages',
      'Follows',
    ]) {
      expect(find.text(tab), findsOneWidget);
    }
    expect(find.text('Alice'), findsOneWidget);
    expect(find.text('liked your post'), findsOneWidget);
    expect(find.text('3 unread'), findsOneWidget);
    expect(find.byIcon(Icons.circle), findsOneWidget); // unread dot
  });

  testWidgets('empty state renders nothing-here message', (tester) async {
    api.stubString('crateFfiNotificationsNotificationsFetch', '[]');
    api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 0);

    await pumpScreen(tester);

    expect(find.text('Nothing here yet'), findsOneWidget);
    expect(find.textContaining('unread'), findsNothing);
  });

  testWidgets('load error does not crash and falls back to empty state',
      (tester) async {
    api.stub('crateFfiNotificationsNotificationsFetch', (_) {
      throw Exception('db busy');
    });

    await pumpScreen(tester);

    expect(find.text('Nothing here yet'), findsOneWidget);
    expect(find.text('Mentions'), findsOneWidget);
  });

  testWidgets('mark all read calls bridge with active pubkey', (tester) async {
    api.stubString('crateFfiNotificationsNotificationsFetch', notifJson);
    api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 3);
    api.stubBool('crateFfiNotificationsNotificationsMarkAllRead', true);

    await pumpScreen(tester);

    await tester.tap(find.byIcon(Icons.done_all));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiNotificationsNotificationsMarkAllRead'), 1);
    final inv =
        api.callsOf('crateFfiNotificationsNotificationsMarkAllRead').single;
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(find.text('3 unread'), findsNothing);
  });

  testWidgets('refresh fetches unread and reports count in snackbar',
      (tester) async {
    api.stubString('crateFfiNotificationsNotificationsFetch', notifJson);
    api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 3);
    api.stubString(
      'crateFfiNotificationsNotificationsFetchUnread',
      '[{"id":"n3","notification_type":"like","from_pubkey":"pkx",'
      '"from_name":"Alice","from_avatar":"","content_preview":"liked your post",'
      '"read":false,"created_at":0,"action_url":""},'
      '{"id":"n2","notification_type":"follow","from_pubkey":"pky",'
      '"from_name":"Bob","from_avatar":"","content_preview":"followed you",'
      '"read":true,"created_at":0,"action_url":""}]',
    );

    await pumpScreen(tester);

    await tester.tap(find.byIcon(Icons.refresh));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiNotificationsNotificationsFetchUnread'), 1);
    expect(find.textContaining('2 unread notification'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('tab switch loads typed notifications', (tester) async {
    api.stubString('crateFfiNotificationsNotificationsFetch', notifJson);
    api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 0);
    api.stubString('crateFfiNotificationsNotificationsFetchByType', notifJson);

    await pumpScreen(tester);

    await tester.tap(find.text('Mentions'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiNotificationsNotificationsFetchByType'), 1);
    final inv =
        api.callsOf('crateFfiNotificationsNotificationsFetchByType').single;
    expect(api.namedArg(inv, 'notificationType'), 'mention');
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(find.text('Alice'), findsOneWidget);
  });

  testWidgets('tapping a notification marks it read', (tester) async {
    api.stubString('crateFfiNotificationsNotificationsFetch', notifJson);
    api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 3);
    api.stubBool('crateFfiNotificationsNotificationsMarkRead', true);

    await pumpScreen(tester);

    await tester.tap(find.text('Alice'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiNotificationsNotificationsMarkRead'), 1);
    final inv = api.callsOf('crateFfiNotificationsNotificationsMarkRead').single;
    expect(api.namedArg(inv, 'notificationId'), 'n1');
    expect(find.text('2 unread'), findsOneWidget);
    expect(find.byIcon(Icons.circle), findsOneWidget);
  });

  testWidgets('swipe to dismiss deletes notification', (tester) async {
    api.stubString('crateFfiNotificationsNotificationsFetch', notifJson);
    api.stubInt('crateFfiNotificationsNotificationsGetUnreadCount', 3);
    api.stubBool('crateFfiNotificationsNotificationsDelete', true);

    await pumpScreen(tester);

    await tester.drag(find.text('Alice'), const Offset(-600, 0));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiNotificationsNotificationsDelete'), 1);
    final inv = api.callsOf('crateFfiNotificationsNotificationsDelete').single;
    expect(api.namedArg(inv, 'notificationId'), 'n1');
    expect(find.text('Nothing here yet'), findsOneWidget);
  });
}