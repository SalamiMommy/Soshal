// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/thread_screen.dart';
import 'package:soshal_flutter/services/feed_service.dart';
import 'package:soshal_flutter/services/session_service.dart';

import 'helpers/test_env.dart';

const mePubkey = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';
const eventId = 'root-event-id';
const rootPubkey = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb';

late FakeApi api;

Map<String, dynamic> postJson({
  required String id,
  required String pubkey,
  required String content,
  int reactions = 0,
  int replies = 0,
  String? profileName,
}) =>
    {
      'event_id': id,
      'pubkey': pubkey,
      'content': content,
      'created_at': 1700000000,
      'reactions': reactions,
      'replies': replies,
      'reposts': 0,
      'liked': false,
      'profile_name': profileName,
    };

void stubSession(FakeApi api) {
  api.stubString(
    'crateFfiSessionSessionLoad',
    jsonEncode({
      'active_pubkey': mePubkey,
      'accounts': [
        {'pubkey': mePubkey, 'npub': '', 'last_used': 0, 'relay_list': []}
      ],
    }),
  );
}

void stubThread(FakeApi api) {
  api.stubString(
    'crateFfiFeedFeedFetchThread',
    jsonEncode([
      postJson(
        id: eventId,
        pubkey: rootPubkey,
        content: 'root post',
        reactions: 3,
        replies: 2,
        profileName: 'Alice',
      ),
      postJson(
        id: 'reply-1',
        pubkey: mePubkey,
        content: 'first reply',
        replies: 0,
        profileName: 'Me',
      ),
    ]),
  );
  api.stub('crateFfiFeedFeedPublishReply', (_) async => 'new-reply-event');
  api.stub('crateFfiFeedFeedCreateReaction', (_) async => 'reaction-event');
}

Future<SessionService> seedSession() async {
  final session = SessionService();
  await session.loadSession();
  return session;
}

Future<void> pumpThread(
  WidgetTester tester, {
  SessionService? session,
  FeedService? feed,
}) async {
  await tester.pumpWidget(
    MultiProvider(
      providers: [
        ChangeNotifierProvider(create: (_) => feed ?? FeedService()),
        ChangeNotifierProvider(create: (_) => session ?? SessionService()),
      ],
      child: const MaterialApp(
        home: ThreadScreen(eventId: eventId),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  final env = bootstrapTestEnv('widget-thread');
  api = env.$1;

  testWidgets('renders root post and replies from stubbed thread',
      (tester) async {
    api.handlers.clear();
    api.calls.clear();
    stubSession(api);
    stubThread(api);

    await pumpThread(tester, session: await seedSession());

    expect(find.text('Alice'), findsOneWidget);
    expect(find.text('root post'), findsOneWidget);
    expect(find.text('Me'), findsOneWidget);
    expect(find.text('first reply'), findsOneWidget);
    expect(find.text('3'), findsOneWidget);
    expect(find.text('↩ 2'), findsOneWidget);
  });

  testWidgets('reply input sends and reloads thread', (tester) async {
    api.handlers.clear();
    api.calls.clear();
    stubSession(api);
    stubThread(api);

    await pumpThread(tester, session: await seedSession());

    await tester.enterText(find.byType(TextField), 'nice reply');
    await tester.tap(find.byIcon(Icons.send));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiFeedFeedPublishReply'), 1);
    final replyCall = api
        .callsOf('crateFfiFeedFeedPublishReply')
        .single;
    expect(api.namedArg(replyCall, 'content'), 'nice reply');
    expect(api.namedArg(replyCall, 'rootEventId'), eventId);
    expect(api.namedArg(replyCall, 'replyToEventId'), eventId);
    // Input cleared + thread reloaded after publish.
    expect(tester.widget<TextField>(find.byType(TextField)).controller!.text,
        isEmpty);
    expect(api.callCount('crateFfiFeedFeedFetchThread'), 2);
  });

  testWidgets('empty reply does not publish', (tester) async {
    api.handlers.clear();
    api.calls.clear();
    stubSession(api);
    stubThread(api);

    await pumpThread(tester, session: await seedSession());

    await tester.tap(find.byIcon(Icons.send));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiFeedFeedPublishReply'), 0);
  });

  testWidgets('shows thread not found when fetch returns no posts', (tester) async {
    api.handlers.clear();
    api.calls.clear();
    stubSession(api);
    api.stubString('crateFfiFeedFeedFetchThread', jsonEncode([]));

    await pumpThread(tester, session: await seedSession());

    expect(find.text('Thread not found'), findsOneWidget);
  });

  testWidgets('reply without active session shows error snackbar',
      (tester) async {
    api.handlers.clear();
    api.calls.clear();
    stubThread(api);

    await pumpThread(tester); // no session stubbed -> activePubkey null

    await tester.enterText(find.byType(TextField), 'hello');
    await tester.tap(find.byIcon(Icons.send));
    await tester.pumpAndSettle();

    expect(find.byType(SnackBar), findsOneWidget);
    expect(find.textContaining('Sign in to reply', findRichText: true),
        findsOneWidget);

    // Drain the snackbar auto-dismiss timer.
    await tester.pump(const Duration(seconds: 5));
    await tester.pumpAndSettle();
  });

  testWidgets('root reaction tap calls createReaction', (tester) async {
    api.handlers.clear();
    api.calls.clear();
    stubSession(api);
    stubThread(api);

    await pumpThread(tester, session: await seedSession());

    await tester.tap(find.byIcon(Icons.favorite_border));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiFeedFeedCreateReaction'), 1);
    final reaction = api.callsOf('crateFfiFeedFeedCreateReaction').single;
    expect(api.namedArg(reaction, 'eventId'), eventId);
    expect(api.namedArg(reaction, 'reactionType'), '+');
    expect(api.callCount('crateFfiFeedFeedFetchThread'), 2);
  });
}