import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/groups_screen.dart';
import 'package:soshal_flutter/services/groups_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-groups');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
    api.stubString('crateFfiDbDbGetSetting', '');
    api.stubBool('crateFfiDbDbSetSetting', true);
  });

  const sessionJson =
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,"relay_list":[]}]}';

  const groupJson =
      '{"id":"g1","name":"Soshal Devs","description":"build stuff",'
      '"picture":"","owner":"pk123","members":42,"is_member":false,'
      '"role":"","created_at":0}';

  Future<void> pumpScreen(WidgetTester tester) async {
    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    await session.loadSession();

    final router = GoRouter(
      initialLocation: '/groups',
      routes: [
        GoRoute(path: '/groups', builder: (_, __) => const GroupsScreen()),
        GoRoute(
          path: '/groups/:groupId',
          builder: (_, __) =>
              const Scaffold(body: Text('group placeholder')),
        ),
      ],
    );

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: session),
          ChangeNotifierProvider(create: (_) => GroupsService()),
        ],
        child: MaterialApp.router(routerConfig: router),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('empty state shows no groups yet', (tester) async {
    api.stubString('crateFfiGroupsGroupsFetchGroups', '[]');

    await pumpScreen(tester);

    expect(find.text('No groups yet'), findsOneWidget);
    expect(find.byType(FloatingActionButton), findsOneWidget);
  });

  testWidgets('renders group rows with member count and join button',
      (tester) async {
    api.stubString('crateFfiGroupsGroupsFetchGroups', '[$groupJson]');

    await pumpScreen(tester);

    expect(find.text('Soshal Devs'), findsOneWidget);
    expect(find.text('42 members'), findsOneWidget);
    expect(find.widgetWithText(TextButton, 'Join'), findsOneWidget);
  });

  testWidgets('join calls bridge with group and pubkey then reloads',
      (tester) async {
    api.stubString('crateFfiGroupsGroupsFetchGroups', '[$groupJson]');
    api.stubBool('crateFfiGroupsGroupsJoin', true);

    await pumpScreen(tester);

    await tester.tap(find.widgetWithText(TextButton, 'Join'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiGroupsGroupsJoin'), 1);
    final inv = api.callsOf('crateFfiGroupsGroupsJoin').single;
    expect(api.namedArg(inv, 'groupId'), 'g1');
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(api.callCount('crateFfiGroupsGroupsFetchGroups'), 2);
  });

  testWidgets('leave calls bridge for member groups', (tester) async {
    final memberJson =
        groupJson.replaceFirst('"is_member":false', '"is_member":true');
    api.stubString('crateFfiGroupsGroupsFetchGroups', '[$memberJson]');
    api.stubBool('crateFfiGroupsGroupsLeave', true);

    await pumpScreen(tester);

    expect(find.widgetWithText(TextButton, 'Leave'), findsOneWidget);

    await tester.tap(find.widgetWithText(TextButton, 'Leave'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiGroupsGroupsLeave'), 1);
    final inv = api.callsOf('crateFfiGroupsGroupsLeave').single;
    expect(api.namedArg(inv, 'groupId'), 'g1');
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
  });

  testWidgets('join failure surfaces error snackbar without crash',
      (tester) async {
    api.stubString('crateFfiGroupsGroupsFetchGroups', '[$groupJson]');
    api.stub('crateFfiGroupsGroupsJoin', (_) {
      throw Exception('relay unreachable');
    });

    await pumpScreen(tester);

    await tester.tap(find.widgetWithText(TextButton, 'Join'));
    await tester.pumpAndSettle();

    expect(find.textContaining('relay unreachable'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('create dialog creates group with entered name', (tester) async {
    api.stubString('crateFfiGroupsGroupsFetchGroups', '[]');
    api.stubString('crateFfiGroupsGroupsCreate', 'g2');

    await pumpScreen(tester);

    await tester.tap(find.byType(FloatingActionButton));
    await tester.pumpAndSettle();
    expect(find.text('Create group'), findsOneWidget);

    await tester.enterText(find.byType(TextField).at(0), 'New Group');
    await tester.tap(find.widgetWithText(FilledButton, 'Create'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiGroupsGroupsCreate'), 1);
    final inv = api.callsOf('crateFfiGroupsGroupsCreate').single;
    expect(api.namedArg(inv, 'name'), 'New Group');
    expect(api.namedArg(inv, 'creatorPubkey'), 'pk123');
    expect((api.namedArg(inv, 'groupId') as String).startsWith('grp'), true);
    expect(api.callCount('crateFfiGroupsGroupsFetchGroups'), 2);
  });

  testWidgets('create failure surfaces snackbar', (tester) async {
    api.stubString('crateFfiGroupsGroupsFetchGroups', '[]');
    api.stub('crateFfiGroupsGroupsCreate', (_) {
      throw Exception('create failed');
    });

    await pumpScreen(tester);

    await tester.tap(find.byType(FloatingActionButton));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField).at(0), 'Broken');
    await tester.tap(find.widgetWithText(FilledButton, 'Create'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Create failed'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('detail renders group info and posts chat messages',
      (tester) async {
    const memberJson =
        '{"id":"g1","name":"Soshal Devs","description":"build stuff",'
        '"picture":"","owner":"pk123","members":42,"is_member":true,'
        '"role":"owner","created_at":0}';
    const msgJson =
        '[{"id":"m1","group_id":"g1","sender_pubkey":"pk123456789012",'
        '"content":"welcome","created_at":0}]';

    final session = SessionService();
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    await session.loadSession();
    api.stubString('crateFfiDbDbGetSetting', '300');
    api.stubString('crateFfiGroupsGroupsGetGroupInfo', memberJson);
    api.stubListString('crateFfiGroupsGroupsGetMembers', ['pkA']);
    api.stubString('crateFfiGroupsGroupsMembersWithRoles', '[]');
    api.stubString('crateFfiGroupsGroupsRolesList', '[]');
    api.stubString('crateFfiGroupsGroupsRoomsList', '[]');
    api.stubString('crateFfiGroupsGroupsThreadsList', '[]');
    api.stubString('crateFfiGroupsGroupsVoiceChannelsList', '[]');
    api.stubString('crateFfiGroupsGroupsFetchMessages', msgJson);
    api.stubString('crateFfiGroupsGroupsPostMessage', '{"id":"m2"}');

    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: session),
          ChangeNotifierProvider(create: (_) => GroupsService()),
          ChangeNotifierProvider(create: (_) => SettingsService()),
        ],
        child: const MaterialApp(
          home: GroupDetailScreen(groupId: 'g1'),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Soshal Devs'), findsOneWidget);

    // Switch to Rooms tab to view and post messages
    await tester.tap(find.text('Rooms'));
    await tester.pumpAndSettle();

    expect(find.text('welcome'), findsOneWidget);

    await tester.enterText(find.byType(TextField), 'hello group');
    await tester.tap(find.byIcon(Icons.send));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiGroupsGroupsPostMessage'), 1);
    final inv = api.callsOf('crateFfiGroupsGroupsPostMessage').single;
    expect(api.namedArg(inv, 'groupId'), 'g1');
    expect(api.namedArg(inv, 'content'), 'hello group');
    expect(api.callCount('crateFfiGroupsGroupsFetchMessages'), 2);
  });

  testWidgets('private group renders lock icon and prompts for password on join',
      (tester) async {
    const privateGroupJson =
        '{"id":"g-priv","name":"Secret Club","description":"shh",'
        '"picture":"","owner":"pk-owner","members":5,"is_member":false,'
        '"role":"","created_at":0,"is_private":true}';

    api.stubString('crateFfiGroupsGroupsFetchGroups', '[$privateGroupJson]');
    api.stubBool('crateFfiGroupsGroupsJoin', true);

    await pumpScreen(tester);

    expect(find.text('Secret Club'), findsOneWidget);
    expect(find.byIcon(Icons.lock_outline), findsOneWidget);

    await tester.tap(find.widgetWithText(TextButton, 'Join'));
    await tester.pumpAndSettle();

    // Dialog appears
    expect(find.text('Private Community'), findsOneWidget);
    expect(find.textContaining('Enter the password to join "Secret Club"'),
        findsOneWidget);

    // Enter password and submit
    await tester.enterText(
        find.widgetWithText(TextField, 'Password'), 'superSecret42');
    await tester.tap(find.widgetWithText(FilledButton, 'Join'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiGroupsGroupsJoin'), 1);
    final inv = api.callsOf('crateFfiGroupsGroupsJoin').single;
    expect(api.namedArg(inv, 'groupId'), 'g-priv');
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(api.namedArg(inv, 'password'), 'superSecret42');
  });

  testWidgets('create private group sends isPrivate and password', (tester) async {
    api.stubString('crateFfiGroupsGroupsFetchGroups', '[]');
    api.stubString('crateFfiGroupsGroupsCreate', 'g-created-priv');

    await pumpScreen(tester);

    await tester.tap(find.byType(FloatingActionButton));
    await tester.pumpAndSettle();

    await tester.enterText(
        find.widgetWithText(TextField, 'Name *'), 'Private Lounge');

    // Toggle private switch
    await tester.tap(find.byType(SwitchListTile));
    await tester.pumpAndSettle();

    // Enter password
    await tester.enterText(
        find.widgetWithText(TextField, 'Community Password *'), 'myClubPass');

    await tester.tap(find.widgetWithText(FilledButton, 'Create'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiGroupsGroupsCreate'), 1);
    final inv = api.callsOf('crateFfiGroupsGroupsCreate').single;
    expect(api.namedArg(inv, 'name'), 'Private Lounge');
    expect(api.namedArg(inv, 'isPrivate'), isTrue);
    expect(api.namedArg(inv, 'password'), 'myClubPass');
  });
}