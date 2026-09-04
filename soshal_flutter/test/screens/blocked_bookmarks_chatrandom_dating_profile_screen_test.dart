import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/blocked_screen.dart';
import 'package:soshal_flutter/screens/bookmarks_screen.dart';
import 'package:soshal_flutter/screens/chat_random_screen.dart';
import 'package:soshal_flutter/screens/dating_profile_screen.dart';
import 'package:soshal_flutter/services/bookmarks_service.dart';
import 'package:soshal_flutter/services/chatrandom_service.dart';
import 'package:soshal_flutter/services/dating_service.dart';
import 'package:soshal_flutter/services/messaging_service.dart';
import 'package:soshal_flutter/services/session_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-account-screens');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  const sessionJson =
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,'
      '"relay_list":["wss://relay.example.com"]}]}';

  Future<SessionService> pump(
    WidgetTester tester,
    Widget Function(SessionService session) build, {
    bool withSession = false,
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
        ChangeNotifierProvider(create: (_) => BookmarksService()),
        ChangeNotifierProvider(create: (_) => ChatrandomService()),
        ChangeNotifierProvider(create: (_) => DatingService()),
        ChangeNotifierProvider(create: (_) => IdentityService()),
      ],
      child: MaterialApp(home: build(session)),
    ));
    await tester.pumpAndSettle();
    return session;
  }

  testWidgets('blocked screen empty state without account', (tester) async {
    await pump(tester, (_) => const BlockedScreen());

    expect(find.text('Blocked Users'), findsOneWidget);
    expect(find.text('No blocked users'), findsOneWidget);
    expect(api.callCount('crateFfiIdentityIdentityGetBlockedUsers'), 0);
  });

  testWidgets('blocked screen with account and empty list', (tester) async {
    api.stubString('crateFfiIdentityIdentityGetBlockedUsers', '[]');
    await pump(tester, (_) => const BlockedScreen(), withSession: true);

    expect(find.text('Blocked Users'), findsOneWidget);
    expect(find.text('No blocked users'), findsOneWidget);
    expect(api.callCount('crateFfiIdentityIdentityGetBlockedUsers'), 1);
  });

  testWidgets('bookmarks screen without account shows error', (tester) async {
    await pump(tester, (_) => const BookmarksScreen());

    expect(find.text('Bookmarks'), findsOneWidget);
    expect(find.textContaining('No active account'), findsOneWidget);
  });

  testWidgets('bookmarks screen with account and empty list', (tester) async {
    api.stubString('crateFfiBookmarksBookmarksList', '[]');
    await pump(tester, (_) => const BookmarksScreen(), withSession: true);

    expect(find.text('Bookmarks'), findsOneWidget);
    expect(find.text('No bookmarks yet'), findsOneWidget);
    expect(
      find.text('Saved posts will appear here.'),
      findsOneWidget,
    );
    expect(api.callCount('crateFfiBookmarksBookmarksList'), 1);
  });

  testWidgets('chat random screen without account shows sign-in wall',
      (tester) async {
    await pump(tester, (_) => const ChatRandomScreen());

    expect(find.text('Chat Random'), findsOneWidget);
    expect(find.text('Sign in required'), findsOneWidget);
    expect(api.callCount('crateFfiChatrandomChatrandomFetch'), 0);
  });

  testWidgets('chat random screen with account renders discovery panel',
      (tester) async {
    api.stubString('crateFfiChatrandomChatrandomAvailableContent', '[]');
    api.stubString('crateFfiChatrandomChatrandomFetch', '[]');
    await pump(tester, (_) => const ChatRandomScreen(), withSession: true);

    expect(find.text('Chat Random'), findsOneWidget);
    expect(find.text('Your Status'), findsOneWidget);
    expect(find.text('Find a peer'), findsOneWidget);
    expect(find.text('No peers yet. Find a peer above.'), findsOneWidget);
    expect(api.callCount('crateFfiChatrandomChatrandomFetch'), 1);
  });

  testWidgets('dating profile screen without account shows create form',
      (tester) async {
    await pump(tester, (_) => const DatingProfileScreen());

    expect(find.text('Create dating profile'), findsOneWidget);
    expect(find.text('Save'), findsOneWidget);
    expect(find.text('Name'), findsOneWidget);
    expect(find.text('Bio'), findsOneWidget);
    expect(find.text('Interests'), findsOneWidget);
    expect(api.callCount('crateFfiDatingDatingGetOwnProfile'), 0);
  });
}
