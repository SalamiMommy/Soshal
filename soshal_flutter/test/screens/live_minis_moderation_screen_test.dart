import 'package:camera_platform_interface/camera_platform_interface.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:plugin_platform_interface/plugin_platform_interface.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/live_broadcast_screen.dart';
import 'package:soshal_flutter/screens/live_screen.dart';
import 'package:soshal_flutter/screens/minis_screen.dart';
import 'package:soshal_flutter/screens/minis_user_screen.dart';
import 'package:soshal_flutter/screens/moderation_screen.dart';
import 'package:soshal_flutter/services/messaging_service.dart';
import 'package:soshal_flutter/services/minis_service.dart';
import 'package:soshal_flutter/services/moderation_service.dart';
import 'package:soshal_flutter/services/p2p_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/streaming_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

class FakeModerationService extends ModerationService {
  @override
  Future<void> load(String pubkey) async {
    await Future<void>.delayed(Duration.zero);
    notifyListeners();
  }
}

class NoCamerasPlatform extends CameraPlatform with MockPlatformInterfaceMixin {
  @override
  Future<List<CameraDescription>> availableCameras() async => [];
}

void main() {
  final env = bootstrapTestEnv('test-live-minis-screens');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
    CameraPlatform.instance = NoCamerasPlatform();
  });

  const sessionJson = '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,'
      '"relay_list":["wss://relay.example.com"]}]}';

  Future<SessionService> pump(
    WidgetTester tester,
    Widget child, {
    bool withSession = false,
    bool settle = true,
  }) async {
    tester.view.physicalSize = const Size(900, 2600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    if (withSession) {
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      await session.loadSession();
    }
    await tester.pumpWidget(MultiProvider(
      providers: [
        ChangeNotifierProvider<SessionService>.value(value: session),
        ChangeNotifierProvider(create: (_) => StreamingService()),
        ChangeNotifierProvider(create: (_) => P2pService()),
        ChangeNotifierProvider<ModerationService>.value(
          value: FakeModerationService(),
        ),
        // A ChangeNotifier, so it needs the listening provider: a plain
        // `Provider` makes every `context.watch<MinisService>()` throw
        // "Tried to use Provider with a subtype of Listenable/Stream", which
        // `_loadRecent` swallows into a debugPrint. That is why every minis
        // test here only ever saw the empty state.
        ChangeNotifierProvider<MinisService>(create: (_) => MinisService()),
        ChangeNotifierProvider(create: (_) => IdentityService()),
      ],
      child: MaterialApp(home: child),
    ));
    if (settle) {
      await tester.pumpAndSettle();
    } else {
      await tester.pump(const Duration(milliseconds: 300));
    }
    return session;
  }

  testWidgets('live broadcast screen handles camera unavailable',
      (tester) async {
    await pump(
      tester,
      const LiveBroadcastScreen(streamId: 'stream-1', title: 'Test stream'),
      settle: false,
    );

    expect(find.text('Test stream'), findsOneWidget);
    expect(find.text('Stop'), findsOneWidget);
    expect(find.textContaining('Camera unavailable'), findsOneWidget);
  });

  testWidgets('live screen empty state', (tester) async {
    api.stubString('crateFfiStreamingStreamingFetchLive', '[]');
    await pump(tester, const LiveScreen());

    expect(find.text('Live'), findsOneWidget);
    expect(find.text('No streams right now'), findsOneWidget);
    expect(find.byIcon(Icons.videocam), findsOneWidget);
    expect(api.callCount('crateFfiStreamingStreamingFetchLive'), 1);
  });

  testWidgets('minis screen empty registry', (tester) async {
    api.stubString('crateFfiMinisMinisFetch', '[]');
    await pump(tester, const MinisScreen());

    expect(find.text('Minis'), findsWidgets);
    expect(find.text('Content filter plugin'), findsOneWidget);
    expect(find.text('Run filter'), findsOneWidget);
    expect(find.text('Rank feed with mini plugin'), findsOneWidget);
    expect(find.text('No minis yet'), findsOneWidget);
  });

  /// `MiniItem.fromJson` with every field, so a seeded row renders exactly like a
  /// fetched one. [n] rows with distinct ids and URLs.
  String minisJson(int n) {
    final rows = [
      for (var i = 0; i < n; i++)
        '{"id":"mini$i","pubkey":"pk${i.toString().padLeft(4, '0')}",'
            '"videoUrl":"https://v/$i.mp4","blobHash":"","mediaSize":1,'
            '"textOverlay":"Mini $i","thumbnail":"","audience":"public",'
            '"createdAt":0,"reactions":0,"liked":false}',
    ];
    return '[${rows.join(',')}]';
  }

  /// The ForYou list with [n] minis.
  ///
  /// The screen opens in Reels mode -- a `PageView.builder`, already lazy -- so
  /// the eager list 6.6 fixed only appears after the mode toggle, which is what
  /// a user without Reels support sees.
  Future<void> pumpList(WidgetTester tester, int n) async {
    api.stubString('crateFfiMinisMinisFetch', minisJson(n));
    api.stubString('crateFfiMinisMinisSaved', '[]');
    await pump(tester, const MinisScreen());
    await tester.tap(find.byTooltip('Switch to list'));
    await tester.pumpAndSettle();
  }

  /// Row titles are `Mini <n>`; nothing else on this screen starts that way.
  Set<String> visibleRows(WidgetTester tester) => tester
      .widgetList<Text>(find.byType(Text))
      .map((t) => t.data ?? '')
      .where((t) => t.startsWith('Mini '))
      .toSet();

  testWidgets('a large mini registry only builds the rows on screen',
      (tester) async {
    // A pixel assertion passes against the eager version too, so this counts
    // instantiated rows instead: with 800 minis the eager list builds 800 tiles
    // before the first frame.
    await pumpList(tester, 800);

    final built = tester.widgetList(find.byType(ListTile)).length;
    expect(built, lessThan(120),
        reason: 'only the visible window plus cache extent may be built, '
            'but $built tiles were instantiated');
    // The header is still there -- it is built once and indexed into.
    expect(find.text('Rank feed with mini plugin'), findsOneWidget);
    expect(find.text('No minis yet'), findsNothing);
  });

  testWidgets('mini rows are built on demand and released when scrolled away',
      (tester) async {
    await pumpList(tester, 300);

    final before = visibleRows(tester);
    expect(before, isNotEmpty,
        reason: 'rows must be on screen for this to say anything');

    await tester.drag(find.byType(ListView).first, const Offset(0, -20000));
    await tester.pumpAndSettle();

    expect(visibleRows(tester).intersection(before), isEmpty,
        reason: 'rows scrolled off screen must no longer be built');
    expect(tester.takeException(), isNull);
  });

  testWidgets('the last mini is reachable by scrolling to the end',
      (tester) async {
    // `itemCount` is header + items. One too few silently drops the tail row
    // and nothing throws, so the only way to see it is to scroll to the end.
    await pumpList(tester, 40);
    await tester.drag(find.byType(ListView).first, const Offset(0, -40000));
    await tester.pumpAndSettle();

    expect(find.text('Mini 39'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('an empty registry still shows the empty state', (tester) async {
    api.stubString('crateFfiMinisMinisFetch', '[]');
    await pump(tester, const MinisScreen());
    expect(find.text('No minis yet'), findsOneWidget);
  });

  testWidgets('minis user screen empty registry', (tester) async {
    api.stubString('crateFfiMinisMinisFetch', '[]');
    await pump(tester, const MinisUserScreen(pubkey: 'pk-abc'));

    expect(find.text('Minis'), findsWidgets);
    expect(find.text('No minis yet'), findsOneWidget);
  });

  testWidgets('moderation screen renders panels and empty states',
      (tester) async {
    api.stubString('crateFfiModerationModerationGetMuted', '[]');
    api.stubString('crateFfiModerationModerationGetBlocked', '[]');
    api.stubString('crateFfiModerationModerationGetWordFilters', '[]');
    api.stubString('crateFfiModerationModerationListReports', '[]');
    await pump(tester, const ModerationScreen());

    expect(find.text('Moderation'), findsOneWidget);
    expect(find.text('Muted users'), findsOneWidget);
    expect(find.text('Blocked users'), findsOneWidget);
    expect(find.text('Reports'), findsOneWidget);
    expect(find.text('No muted users'), findsOneWidget);
    expect(find.text('No blocked users'), findsOneWidget);
    expect(find.text('No reports'), findsOneWidget);
    expect(find.text('No word filters'), findsOneWidget);
    expect(find.text('Restriction check'), findsOneWidget);
    expect(find.text('Community jury'), findsOneWidget);
  });

  testWidgets('moderation screen with account loads report list',
      (tester) async {
    api.stubString('crateFfiModerationModerationGetMuted', '[]');
    api.stubString('crateFfiModerationModerationGetBlocked', '[]');
    api.stubString('crateFfiModerationModerationGetWordFilters', '[]');
    api.stubString('crateFfiModerationModerationListReports', '[]');
    await pump(tester, const ModerationScreen(), withSession: true);

    expect(find.text('Moderation'), findsOneWidget);
    expect(find.text('No reports'), findsOneWidget);
    expect(api.callCount('crateFfiModerationModerationListReports'), 1);
  });
}
