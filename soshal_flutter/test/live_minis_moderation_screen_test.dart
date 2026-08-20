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

import 'helpers/test_env.dart';

late FakeApi api;

class FakeModerationService extends ModerationService {
  @override
  Future<void> load(String pubkey) async {
    await Future<void>.delayed(Duration.zero);
    notifyListeners();
  }
}

class NoCamerasPlatform extends CameraPlatform
    with MockPlatformInterfaceMixin {
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

  const sessionJson =
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
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
        Provider(create: (_) => MinisService()),
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