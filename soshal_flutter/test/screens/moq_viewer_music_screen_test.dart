import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/moq_viewer_screen.dart';
import 'package:soshal_flutter/screens/music_screen.dart';
import 'package:soshal_flutter/screens/musicloud_user_screen.dart';
import 'package:soshal_flutter/services/music_service.dart';
import 'package:soshal_flutter/services/p2p_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/shell_service.dart';
import 'package:soshal_flutter/services/streaming_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

class FakeMusicService extends MusicService {
  @override
  Future<List<MusicTrack>> fetchTracks({String? author, int limit = 50}) async {
    await Future<void>.delayed(Duration.zero);
    notifyListeners();
    return [];
  }
}

void main() {
  final env = bootstrapTestEnv('test-music-moq-screens');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  Future<void> pump(WidgetTester tester, Widget child,
      {bool settle = true}) async {
    tester.view.physicalSize = const Size(900, 2600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(MultiProvider(
      providers: [
        ChangeNotifierProvider(create: (_) => SessionService()),
        ChangeNotifierProvider(create: (_) => StreamingService()),
        ChangeNotifierProvider(create: (_) => P2pService()),
        ChangeNotifierProvider<MusicService>.value(
          value: FakeMusicService(),
        ),
        ChangeNotifierProvider(create: (_) => ShellService()),
      ],
      child: MaterialApp(home: child),
    ));
    if (settle) {
      await tester.pumpAndSettle();
    } else {
      await tester.pump(const Duration(milliseconds: 200));
      await tester.pump(const Duration(milliseconds: 200));
    }
  }

  testWidgets('moq viewer renders waiting state', (tester) async {
    api.stubString('crateFfiStreamingStreamingMoqSubscribeStream',
        '{"status":"ok"}');
    api.stubString('crateFfiP2PP2PMoqSubscribeFetch', '{"groups":[]}');
    await pump(
      tester,
      const MoqViewerScreen(addr: '127.0.0.1:4242', streamId: 'stream-1'),
      settle: false,
    );

    expect(find.text('Live'), findsOneWidget);
    expect(find.text('Leave'), findsOneWidget);
    expect(find.byType(CircularProgressIndicator), findsOneWidget);
    expect(find.text('0 frames'), findsOneWidget);

    await tester.tap(find.widgetWithText(TextButton, 'Leave'), warnIfMissed: false);
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpWidget(const SizedBox());
    await tester.pump(const Duration(milliseconds: 300));
  });

  testWidgets('musicloud user screen empty state', (tester) async {
    await pump(tester, const MusicloudUserScreen(pubkey: 'pk-abc'));

    expect(find.text('Music'), findsOneWidget);
    expect(find.text('No tracks published'), findsOneWidget);
    expect(
      find.text('This author has not published any Musicloud tracks yet.'),
      findsOneWidget,
    );
  });

  testWidgets('music screen empty state', (tester) async {
    await pump(tester, const MusicloudScreen());

    expect(find.text('Musicloud'), findsOneWidget);
    expect(find.text('No songs found'), findsOneWidget);
    expect(
      find.text('Be the first to publish an audio track on Musicloud — tap +.'),
      findsOneWidget,
    );
    expect(find.byIcon(Icons.add), findsOneWidget);
  });
}