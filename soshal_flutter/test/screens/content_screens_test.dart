import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/profile_screen.dart';
import 'package:soshal_flutter/screens/scheduled_screen.dart';
import 'package:soshal_flutter/screens/search_screen.dart';
import 'package:soshal_flutter/screens/share_app_screen.dart';
import 'package:soshal_flutter/screens/stealth_screen.dart';
import 'package:soshal_flutter/screens/storage_screen.dart';
import 'package:soshal_flutter/screens/stories_screen.dart';
import 'package:soshal_flutter/screens/turso_settings_screen.dart';
import 'package:soshal_flutter/screens/vouch_screen.dart';
import 'package:soshal_flutter/services/feed_service.dart';
import 'package:soshal_flutter/services/media_service.dart';
import 'package:soshal_flutter/services/messaging_service.dart';
import 'package:soshal_flutter/services/network_service.dart';
import 'package:soshal_flutter/services/scheduled_service.dart';
import 'package:soshal_flutter/services/search_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';
import 'package:soshal_flutter/services/stealth_service.dart';
import 'package:soshal_flutter/services/streaming_service.dart';
import 'package:soshal_flutter/services/turso_service.dart';
import 'package:soshal_flutter/services/vouch_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

class FakeStealthService extends StealthService {
  @override
  Future<List<String>> load([String? pubkey]) async {
    await Future<void>.delayed(Duration.zero);
    notifyListeners();
    return [];
  }
}

void main() {
  final env = bootstrapTestEnv('test-content-screens');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  Future<void> pump(WidgetTester tester, Widget child) async {
    tester.view.physicalSize = const Size(900, 2600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(MultiProvider(
      providers: [
        ChangeNotifierProvider(create: (_) => SessionService()),
        ChangeNotifierProvider(create: (_) => IdentityService()),
        ChangeNotifierProvider(create: (_) => FeedService()),
        ChangeNotifierProvider(create: (_) => NetworkService()),
        ChangeNotifierProvider(create: (_) => ScheduledService()),
        ChangeNotifierProvider(create: (_) => SearchService()),
        ChangeNotifierProvider<StealthService>.value(
          value: FakeStealthService(),
        ),
        ChangeNotifierProvider(create: (_) => SettingsService()),
        ChangeNotifierProvider(create: (_) => MediaService()),
        ChangeNotifierProvider(create: (_) => StreamingService()),
        ChangeNotifierProvider(create: (_) => TursoService()),
        ChangeNotifierProvider(create: (_) => VouchService()),
      ],
      child: MaterialApp(home: child),
    ));
    await tester.pumpAndSettle();
  }

  testWidgets('profile screen without account shows wall', (tester) async {
    await pump(tester, const ProfileScreen());

    expect(find.text('Profile'), findsOneWidget);
    expect(find.text('No profile loaded'), findsOneWidget);
  });

  testWidgets('profile screen with pubkey loads author posts', (tester) async {
    const pk = '1122334455667788990011223344556677889900112233445566778899001122';
    api.stubString('crateFfiDbDbGetSetting', '');
    api.stubString('crateFfiIdentityIdentityGetProfile',
        '{"pubkey":"$pk","name":"alice","display_name":"Alice D","about":"Hello world","picture":"","banner":"","nip05":"","nip05_valid":false,"created_at":0,"followers":0,"following":0,"is_following":false,"wot_status":"trusted"}');
    api.stubString('crateFfiIdentityIdentityGetWotStatus', 'trusted');
    api.stub('crateFfiIdentityIdentityGetTrustScore', (_) => 95.0);
    api.stubBool('crateFfiIdentityIdentityIsBlocked', false);
    api.stubString('crateFfiFeedFeedFetchWindow',
        '[{"id":"p1","pubkey":"$pk","content":"Alice first post","created_at":1000}]');

    await pump(tester, const ProfileScreen(pubkey: pk));

    expect(find.text('Alice D'), findsOneWidget);
    expect(find.text('Alice first post'), findsOneWidget);
  });

  testWidgets('scheduled screen without account shows sign-in wall',
      (tester) async {
    await pump(tester, const ScheduledScreen());

    expect(find.text('Scheduled Posts'), findsOneWidget);
    expect(find.text('Sign in required'), findsOneWidget);
  });

  testWidgets('search screen renders tabs and trending empty state',
      (tester) async {
    api.stubString('crateFfiSearchSearchTrendingProfiles', '[]');
    api.stubString('crateFfiDbDbGetTrendingHashtags', '[]');
    await pump(tester, const SearchScreen());

    expect(find.text('Trending hashtags'), findsOneWidget);
    expect(find.text('Trending profiles'), findsOneWidget);
    expect(find.text('Nothing trending yet'), findsOneWidget);
    expect(find.text('All'), findsOneWidget);
    expect(find.text('Posts'), findsOneWidget);
    expect(find.text('People'), findsOneWidget);
    expect(find.text('Tags'), findsOneWidget);
    expect(find.text('Mentions'), findsOneWidget);
    expect(find.text('Search posts, people, #tags'), findsOneWidget);
  });

  testWidgets('share app screen without account shows fallback',
      (tester) async {
    await pump(tester, const ShareAppScreen());

    expect(find.text('Share Soshal'), findsOneWidget);
    expect(find.text('No active account'), findsOneWidget);
  });

  testWidgets('stealth screen renders whitelist editor', (tester) async {
    api.stubString('crateFfiDbDbGetSetting', '');
    await pump(tester, const StealthScreen());

    expect(find.text('Stealth Whitelist'), findsOneWidget);
    expect(find.text('Save Whitelist'), findsOneWidget);
    expect(find.text('One pubkey per line (hex or npub)'), findsOneWidget);
  });

  testWidgets('storage screen renders sections', (tester) async {
    api.stubString('crateFfiDbDbStorageStats', '[]');
    api.stubString('crateFfiMediaMediaGetCachePath', '');
    api.stubString('crateFfiDbDbGetSetting', '');
    await pump(tester, const StorageScreen());

    expect(find.text('Storage'), findsOneWidget);
    expect(find.text('Auto-download media'), findsOneWidget);
    expect(find.text('Auto-play media'), findsOneWidget);
    expect(find.text('Clear old posts'), findsOneWidget);
    expect(find.text('Clear all posts'), findsOneWidget);
    expect(find.text('Local media server'), findsOneWidget);
    expect(find.text('Database'), findsOneWidget);
  });

  testWidgets('stories screen empty state', (tester) async {
    api.stubString('crateFfiStreamingStreamingFetchStories', '[]');
    await pump(tester, const StoriesScreen());

    expect(find.text('Stories'), findsOneWidget);
    expect(find.text('No stories from people you follow'), findsOneWidget);
    expect(find.byIcon(Icons.add), findsOneWidget);
  });

  testWidgets('turso settings screen renders status panel', (tester) async {
    api.stubString('crateFfiTursoDbTursoStatus', '{}');
    await pump(tester, const TursoSettingsScreen());

    expect(find.text('Turso Database Sync'), findsOneWidget);
    expect(find.text('Turso Edge Replication'), findsOneWidget);
    expect(find.text('Status'), findsOneWidget);
    expect(find.text('Database URL'), findsOneWidget);
    expect(find.text('Auth Token'), findsOneWidget);
    expect(find.text('Save & Connect'), findsOneWidget);
    expect(find.text('Sync Now'), findsOneWidget);
  });

  testWidgets('vouch screen renders form and empty state', (tester) async {
    await pump(tester, const VouchScreen());

    expect(find.text('Vouch'), findsOneWidget);
    expect(find.text('Target pubkey'), findsOneWidget);
    expect(find.text('Endorsement'), findsOneWidget);
    expect(find.text('Publish Vouch'), findsOneWidget);
    expect(
      find.text('No vouches yet. Enter a target pubkey and publish one.'),
      findsOneWidget,
    );
  });
}