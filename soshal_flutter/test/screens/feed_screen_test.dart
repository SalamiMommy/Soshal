// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/feed_screen.dart';
import 'package:soshal_flutter/services/feed_service.dart';
import 'package:soshal_flutter/services/layout_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/zap_service.dart';

import '../helpers/test_env.dart';

const _pkA = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const _pkB = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';

/// SessionService with a controllable active pubkey (field setter is private).
class FakeSession extends SessionService {
  FakeSession({this.pubkey});

  final String? pubkey;

  @override
  String? get activePubkey => pubkey;
}

/// ZapService that never touches FFI or notifies: card initState calls
/// fetchTotalMsat during build, and a synchronous failure path would
/// notifyListeners mid-build.
class FakeZapService extends ZapService {
  @override
  Future<int> fetchTotalMsat(String eventId) async => 0;
}

const _layoutJson =
    '[{"id":"ev-1","height_px":220,"media_height_px":0},'
    '{"id":"ev-2","height_px":220,"media_height_px":0}]';

const _twoPostsJson =
    '[{"event_id":"ev-1","pubkey":"$_pkA","content":"first post text",'
    '"created_at":1700000001,"reactions":3,"replies":1,"reposts":2,'
    '"liked":false,"profile_name":"Alice"},'
    '{"event_id":"ev-2","pubkey":"$_pkB","content":"second post text",'
    '"created_at":1700000002,"reactions":1,"replies":0,"reposts":0,'
    '"liked":false,"profile_name":"Bob"}]';

late FakeApi api;

Future<void> pumpFeed(WidgetTester tester, {String? sessionPubkey}) async {
  tester.view.physicalSize = const Size(1080, 2400);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  final router = GoRouter(
    initialLocation: '/feed',
    routes: [
      GoRoute(path: '/feed', builder: (_, __) => const FeedScreen()),
      GoRoute(
        path: '/settings',
        builder: (_, __) => const Scaffold(body: Text('settings')),
      ),
      GoRoute(
        path: '/inbox',
        builder: (_, __) => const Scaffold(body: Text('inbox')),
      ),
      GoRoute(
        path: '/notifications',
        builder: (_, __) => const Scaffold(body: Text('notifications')),
      ),
      GoRoute(
        path: '/profile/:pubkey',
        builder: (_, __) => const Scaffold(body: Text('profile')),
      ),
    ],
  );

  await tester.pumpWidget(
    MultiProvider(
      providers: [
        ChangeNotifierProvider(create: (_) => FeedService()),
        ChangeNotifierProvider<SessionService>(
          create: (_) => FakeSession(pubkey: sessionPubkey),
        ),
        ChangeNotifierProvider(create: (_) => LayoutService()),
        ChangeNotifierProvider<ZapService>(create: (_) => FakeZapService()),
      ],
      child: MaterialApp.router(routerConfig: router),
    ),
  );
  await tester.pumpAndSettle();
}

/// Pump past the SnackBar auto-dismiss timer so no timers stay pending.
Future<void> flushSnackBars(WidgetTester tester) async {
  await tester.pump(const Duration(seconds: 5));
  await tester.pumpAndSettle();
}

void main() {
  final env = bootstrapTestEnv('widget-feed');
  api = env.$1;
  setUp(() {
    // FeedScreen.dispose() calls context.read (lib code, out of test scope);
    // replacing the pumped tree between tests unmounts a deactivated element
    // and trips this known assert during tree finalization. Swallow only that
    // exact teardown error — everything else still fails the test.
        FlutterError.onError = (details) {
      if (details.exceptionAsString().contains('deactivated widget')) return;
      FlutterError.presentError(details);
    };
    api.handlers.clear();
    api.calls.clear();
    api.stubString('crateFfiFeedFeedComputeCardLayouts', _layoutJson);
    api.stubBool('crateFfiSearchSearchRemoveIndexed', true);
  });

  testWidgets('renders posts from stubbed feed data', (tester) async {
    api.stubString('crateFfiFeedFeedFetchEvents', _twoPostsJson);

    await pumpFeed(tester);

    expect(find.text('first post text'), findsOneWidget);
    expect(find.text('second post text'), findsOneWidget);
    expect(find.text('Alice'), findsOneWidget);
    expect(find.text('Bob'), findsOneWidget);
    expect(api.callCount('crateFfiFeedFeedFetchEvents'), 1);
  });

  testWidgets('like tap sends createReaction with "+"', (tester) async {
    api.stubString('crateFfiFeedFeedFetchEvents', _twoPostsJson);
    api.stub('crateFfiFeedFeedCreateReaction', (_) async => 'r-1');

    await pumpFeed(tester);

    await tester.tap(find.byIcon(Icons.favorite).first);
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiFeedFeedCreateReaction'), 1);
    final inv = api.callsOf('crateFfiFeedFeedCreateReaction').single;
    expect(api.namedArg(inv, 'eventId'), 'ev-1');
    expect(api.namedArg(inv, 'reactionType'), '+');
  });

  testWidgets('second like tap sends createReaction with "-"', (tester) async {
    api.stubString('crateFfiFeedFeedFetchEvents', _twoPostsJson);
    api.stub('crateFfiFeedFeedCreateReaction', (_) async => 'r-1');

    await pumpFeed(tester);

    await tester.tap(find.byIcon(Icons.favorite).first);
    await tester.pumpAndSettle();
    await tester.tap(find.byIcon(Icons.favorite).first);
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiFeedFeedCreateReaction'), 2);
    final inv = api.callsOf('crateFfiFeedFeedCreateReaction').last;
    expect(api.namedArg(inv, 'eventId'), 'ev-1');
    expect(api.namedArg(inv, 'reactionType'), '-');
  });

  testWidgets('empty feed shows empty state with Refresh', (tester) async {
    api.stubString('crateFfiFeedFeedFetchEvents', '[]');

    await pumpFeed(tester);

    expect(find.text('No posts yet'), findsOneWidget);
    expect(find.text('Refresh'), findsOneWidget);
  });

  testWidgets('Refresh refetches and renders posts after empty feed',
      (tester) async {
    api.stubString('crateFfiFeedFeedFetchEvents', '[]');

    await pumpFeed(tester);
    expect(find.text('No posts yet'), findsOneWidget);

    api.stubString('crateFfiFeedFeedFetchEvents', _twoPostsJson);
    await tester.tap(find.text('Refresh'));
    await tester.pumpAndSettle();

    expect(find.text('first post text'), findsOneWidget);
    expect(api.callCount('crateFfiFeedFeedFetchEvents'), 2);
  });

  testWidgets('own post delete flow calls deletePost and refetches',
      (tester) async {
    api.stubString('crateFfiFeedFeedFetchEvents', _twoPostsJson);
    api.stub('crateFfiFeedFeedDeletePost', (_) async => 'deleted');

    await pumpFeed(tester, sessionPubkey: _pkA);

    await tester.tap(find.byTooltip('Post actions').first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Delete'));
    await tester.pumpAndSettle();
    expect(find.text('Delete post?'), findsOneWidget);

    await tester.tap(find.text('Delete'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiFeedFeedDeletePost'), 1);
    final inv = api.callsOf('crateFfiFeedFeedDeletePost').single;
    expect(api.namedArg(inv, 'eventId'), 'ev-1');
    expect(find.text('Post deleted'), findsOneWidget);
    // Re-fetch after deletion.
    expect(api.callCount('crateFfiFeedFeedFetchEvents'), 2);

    await flushSnackBars(tester);
  });

  testWidgets('options row is visible under feed posts', (tester) async {
    api.stubString('crateFfiFeedFeedFetchEvents', _twoPostsJson);

    await pumpFeed(tester);

    // Verify all 5 action buttons are rendered for each post
    expect(find.byIcon(Icons.favorite), findsNWidgets(2));
    expect(find.byIcon(Icons.chat_bubble_outline), findsNWidgets(2));
    expect(find.byIcon(Icons.share_outlined), findsNWidgets(2));
    expect(find.byIcon(Icons.mood), findsNWidgets(2));
    expect(find.byIcon(Icons.bolt), findsNWidgets(2));

    // Post 1 stats labels: 3 reactions, 1 reply, 2 reposts
    // Post 2 stats labels: 1 reaction, 0 replies, 0 reposts
    expect(find.text('3'), findsOneWidget);
    expect(find.text('2'), findsOneWidget);
    expect(find.text('1'), findsNWidgets(2));
  });

  testWidgets('tapping share button opens modal with repost and copy options',
      (tester) async {
    api.stubString('crateFfiFeedFeedFetchEvents', _twoPostsJson);
    api.stubString('crateFfiFeedFeedPublishTextNote', 'repost-ev-1');

    await pumpFeed(tester, sessionPubkey: _pkA);

    await tester.tap(find.byIcon(Icons.share_outlined).first);
    await tester.pumpAndSettle();

    expect(find.text('Repost to feed'), findsOneWidget);
    expect(find.text('Copy post text'), findsOneWidget);
    expect(find.text('Copy post ID'), findsOneWidget);

    // Tap Repost to feed
    await tester.tap(find.text('Repost to feed'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiFeedFeedPublishTextNote'), 1);
    final inv = api.callsOf('crateFfiFeedFeedPublishTextNote').single;
    expect(api.namedArg(inv, 'content'), 'nostr:ev-1');

    await flushSnackBars(tester);
  });

  testWidgets('share modal copy post text copies content to clipboard',
      (tester) async {
    api.stubString('crateFfiFeedFeedFetchEvents', _twoPostsJson);

    await pumpFeed(tester);

    await tester.tap(find.byIcon(Icons.share_outlined).first);
    await tester.pumpAndSettle();

    await tester.tap(find.text('Copy post text'));
    await tester.pumpAndSettle();

    expect(find.text('Post text copied to clipboard'), findsOneWidget);

    await flushSnackBars(tester);
  });
}