import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/groups_screen.dart';
import 'package:soshal_flutter/services/groups_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';
import 'package:soshal_flutter/widgets/group_sidebar.dart';
import 'package:soshal_flutter/widgets/group_tabs.dart';

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

  const sessionJson = '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
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
          builder: (_, __) => const Scaffold(body: Text('group placeholder')),
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
    // Group info, members, member roles, custom roles, rooms and voice
    // channels all arrive in one call now; threads and messages stay separate.
    api.stubString(
      'crateFfiGroupsGroupsGetDetailBundle',
      '{"group":$memberJson,"members":["pkA"],"memberRoles":[],'
          '"roles":[],"rooms":[],"voiceChannels":[]}',
    );
    api.stubString('crateFfiGroupsGroupsThreadsList', '[]');
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

  testWidgets('rooms tab renders reactions and toggles emoji', (tester) async {
    const memberJson =
        '{"id":"g1","name":"Soshal Devs","description":"build stuff",'
        '"picture":"","owner":"pk123","members":42,"is_member":true,'
        '"role":"owner","created_at":0}';
    const msgJson =
        '[{"id":"m1","group_id":"g1","sender_pubkey":"pk123456789012",'
        '"content":"welcome","created_at":0}]';
    const reactionJson =
        '[{"message_id":"m1","emoji":"👍","count":2,"reacted":false},'
        '{"message_id":"m1","emoji":"🔥","count":1,"reacted":true}]';

    final session = SessionService();
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    await session.loadSession();
    api.stubString('crateFfiDbDbGetSetting', '300');
    // Group info, members, member roles, custom roles, rooms and voice
    // channels all arrive in one call now; threads and messages stay separate.
    api.stubString(
      'crateFfiGroupsGroupsGetDetailBundle',
      '{"group":$memberJson,"members":["pkA"],"memberRoles":[],'
          '"roles":[],"rooms":[],"voiceChannels":[]}',
    );
    api.stubString('crateFfiGroupsGroupsThreadsList', '[]');
    api.stubString('crateFfiGroupsGroupsFetchMessages', msgJson);
    api.stubString('crateFfiGroupsGroupsRoomsReactions', reactionJson);
    api.stubBool('crateFfiGroupsGroupsRoomsReact', true);

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

    // Switch to Rooms tab
    await tester.tap(find.text('Rooms'));
    await tester.pumpAndSettle();

    // Verify reactions rendered
    expect(find.text('welcome'), findsOneWidget);
    expect(find.text('2'), findsOneWidget); // 👍 count: 2
    expect(find.text('1'), findsOneWidget); // 🔥 count: 1

    // Tap on 👍 reaction chip to toggle
    await tester.tap(find.widgetWithText(FilterChip, '2'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiGroupsGroupsRoomsReact'), 1);
    final inv = api.callsOf('crateFfiGroupsGroupsRoomsReact').single;
    expect(api.namedArg(inv, 'groupId'), 'g1');
    expect(api.namedArg(inv, 'roomId'), '');
    expect(api.namedArg(inv, 'messageId'), 'm1');
    expect(api.namedArg(inv, 'emoji'), '👍');
    expect(api.namedArg(inv, 'pubkey'), 'pk123');

    // Tap more reactions icon
    await tester.tap(find.byIcon(Icons.add_reaction_outlined));
    await tester.pumpAndSettle();

    // Bottom sheet with emoji picker should open
    expect(find.text('🎉'), findsOneWidget);
    await tester.tap(find.text('🎉'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiGroupsGroupsRoomsReact'), 2);
    final inv2 = api.callsOf('crateFfiGroupsGroupsRoomsReact').last;
    expect(api.namedArg(inv2, 'emoji'), '🎉');
  });

  // --- 6.7: the sidebar resize must not rebuild the screen ------------------
  //
  // The drag handle fires once per pointer-move frame. With the width in
  // `State` that rebuilt all of `GroupDetailScreen.build()` at display refresh
  // rate -- the `Consumer<GroupsService>`, the group header, and all three tab
  // subtrees -- for a change that only moves the sidebar's edge.

  const memberGroupJson =
      '{"id":"g1","name":"Soshal Devs","description":"build stuff",'
      '"picture":"","owner":"pk123","members":42,"is_member":true,'
      '"role":"owner","created_at":0}';

  /// `GroupDetailScreen` on a 800px viewport, which is past the 700px
  /// breakpoint, so the sidebar renders beside the tabs (`_desktop`).
  ///
  /// [owner] defaults to the signed-in account, which makes the sidebar render
  /// its `Privacy & Access` section. At the 240px minimum width that section
  /// overflows under the widget-test font -- every glyph is a full em square, so
  /// a 16-character title is roughly twice its real width. That is a test-font
  /// artifact, not a layout bug, so the clamp tests pin a non-owner group to
  /// keep the width assertions clean.
  Future<void> pumpDetail(WidgetTester tester,
      {String? savedWidth,
      String owner = 'pk123',
      Size size = const Size(800, 2400)}) async {
    final group =
        memberGroupJson.replaceFirst('"owner":"pk123"', '"owner":"$owner"');
    tester.view.physicalSize = size;
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    await session.loadSession();
    api.stubString('crateFfiDbDbGetSetting', savedWidth ?? '300');
    api.stubString(
      'crateFfiGroupsGroupsGetDetailBundle',
      '{"group":$group,"members":["pkA"],"memberRoles":[],'
          '"roles":[],"rooms":[],"voiceChannels":[]}',
    );
    api.stubString('crateFfiGroupsGroupsThreadsList', '[]');
    api.stubString('crateFfiGroupsGroupsFetchMessages', '[]');

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: session),
          ChangeNotifierProvider(create: (_) => GroupsService()),
          ChangeNotifierProvider(create: (_) => SettingsService()),
        ],
        child: const MaterialApp(home: GroupDetailScreen(groupId: 'g1')),
      ),
    );
    await tester.pumpAndSettle();
  }

  /// The handle is the 8px strip immediately left of the sidebar.
  Offset handleCenter(WidgetTester tester) =>
      Offset(tester.getTopLeft(find.byType(GroupSidebar)).dx - 4, 400);

  double sidebarWidth(WidgetTester tester) =>
      tester.getSize(find.byType(GroupSidebar)).width;

  /// The tab widgets a full `build()` re-creates. If the screen rebuilds these
  /// are new instances; if only the sidebar's notifier fires, they are the very
  /// same objects.
  Map<Type, Widget> tabWidgets(WidgetTester tester) => {
        for (final t in [GroupVoiceTab, GroupRoomsTab, GroupThreadsTab])
          if (find.byType(t).evaluate().isNotEmpty)
            t: tester.widget(find.byType(t)),
      };

  testWidgets('dragging the handle resizes the sidebar and persists the width',
      (tester) async {
    await pumpDetail(tester);
    expect(sidebarWidth(tester), 300);

    // The handle subtracts the drag delta, so a leftward drag widens.
    final gesture = await tester.startGesture(handleCenter(tester));
    await gesture.moveBy(const Offset(-60, 0));
    await tester.pump();
    expect(sidebarWidth(tester), 360);

    // Not written on every frame -- only when the drag ends.
    expect(api.callCount('crateFfiDbDbSetSetting'), 0);

    await gesture.up();
    await tester.pumpAndSettle();
    final inv = api.callsOf('crateFfiDbDbSetSetting').single;
    expect(api.namedArg(inv, 'key'), 'group_sidebar_width');
    expect(api.namedArg(inv, 'value'), '360');
  });

  testWidgets('the sidebar clamps at its min and max and settles exactly there',
      (tester) async {
    await pumpDetail(tester, owner: 'someone-else');

    // Far past the 420px max.
    final grow = await tester.startGesture(handleCenter(tester));
    await grow.moveBy(const Offset(-900, 0));
    await tester.pump();
    expect(sidebarWidth(tester), 420);
    await grow.up();
    await tester.pumpAndSettle();
    expect(api.namedArg(api.callsOf('crateFfiDbDbSetSetting').last, 'value'),
        '420');

    // Far past the 240px min.
    final shrink = await tester.startGesture(handleCenter(tester));
    await shrink.moveBy(const Offset(900, 0));
    await tester.pump();
    expect(sidebarWidth(tester), 240);
    await shrink.up();
    await tester.pumpAndSettle();
    expect(api.namedArg(api.callsOf('crateFfiDbDbSetSetting').last, 'value'),
        '240');
  });

  testWidgets('a restored width is applied before the first frame',
      (tester) async {
    // The notifier is written in initState, so this must be visible in the very
    // first frame -- there is no setState to wait for.
    await pumpDetail(tester, savedWidth: '355');
    expect(sidebarWidth(tester), 355);
  });

  testWidgets('an out-of-range stored width is clamped, not trusted',
      (tester) async {
    await pumpDetail(tester, savedWidth: '5000', owner: 'someone-else');
    expect(sidebarWidth(tester), 420);
  });

  testWidgets('resizing does not rebuild the tab subtrees', (tester) async {
    await pumpDetail(tester);
    final before = tabWidgets(tester);
    expect(before, isNotEmpty,
        reason: 'no tab is built, so "not rebuilt" would pass vacuously');

    final gesture = await tester.startGesture(handleCenter(tester));
    for (var i = 0; i < 5; i++) {
      await gesture.moveBy(const Offset(-10, 0));
      await tester.pump();
    }
    await gesture.up();
    await tester.pumpAndSettle();

    expect(sidebarWidth(tester), 350, reason: 'the drag must still resize');
    final after = tabWidgets(tester);
    for (final entry in before.entries) {
      expect(identical(after[entry.key], entry.value), isTrue,
          reason: '${entry.key} was rebuilt by a sidebar resize');
    }
  });

  testWidgets('the mobile overlay follows the same notifier and its own cap',
      (tester) async {
    // Below the 700px breakpoint the sidebar is an overlay, built by a second
    // width consumer. It has to read the notifier too, and it caps at 90% of
    // the viewport, which is what makes it distinguishable from the desktop
    // path: at 400px wide that cap is 360, below the 420 hard maximum, so a
    // grow drag settles at 360 rather than 420.
    await pumpDetail(tester, owner: 'someone-else', size: const Size(400, 900));
    await tester.tap(find.byTooltip('Members & roles'));
    await tester.pumpAndSettle();

    // The overlay is a Row of [8px handle][sidebar], so the sidebar itself is
    // always 8px narrower than the width the notifier holds.
    const handle = 8.0;
    expect(sidebarWidth(tester), 300 - handle);

    final grow = await tester.startGesture(handleCenter(tester));
    await grow.moveBy(const Offset(-900, 0));
    await tester.pump();
    expect(sidebarWidth(tester), 360 - handle);
    await grow.up();
    await tester.pumpAndSettle();

    final shrink = await tester.startGesture(handleCenter(tester));
    await shrink.moveBy(const Offset(900, 0));
    await tester.pump();
    expect(sidebarWidth(tester), 240 - handle);
    await shrink.up();
    await tester.pumpAndSettle();
  });

  testWidgets('a real state change still rebuilds the tab subtrees',
      (tester) async {
    // The control for the test above. If this stopped rebuilding the tabs, then
    // "resizing does not rebuild" would be passing for the wrong reason --
    // a screen that never rebuilds at all.
    await pumpDetail(tester);
    final before = tabWidgets(tester);
    expect(before, isNotEmpty);

    // The app bar's Members button is a plain setState, so this *does* re-run
    // build() and re-create the tab widgets.
    await tester.tap(find.byTooltip('Members & roles'));
    await tester.pumpAndSettle();

    final after = tabWidgets(tester);
    for (final entry in before.entries) {
      expect(identical(after[entry.key], entry.value), isFalse,
          reason: '${entry.key} should have been rebuilt by a setState');
    }
  });

  testWidgets(
      'private group renders lock icon and prompts for password on join',
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

  testWidgets('create private group sends isPrivate and password',
      (tester) async {
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
