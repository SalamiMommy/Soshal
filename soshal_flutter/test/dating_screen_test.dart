import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/dating_screen.dart';
import 'package:soshal_flutter/services/dating_service.dart';
import 'package:soshal_flutter/services/session_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-dating');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  const sessionJson =
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,"relay_list":[]}]}';

  const ownProfileJson =
      '{"pubkey":"pk123","name":"Me","age":30,"location":"NYC",'
      '"bio":"","images":[],"interests":["hiking"],'
      '"compatibility_score":0,"last_seen":0}';

  const cardJson =
      '[{"pubkey":"pkX","name":"Alice","age":25,"location":"NYC",'
      '"bio":"hello there","images":[],"interests":["hiking","art"],'
      '"compatibility_score":0,"last_seen":0}]';

  Future<void> pumpScreen(WidgetTester tester, {String? session}) async {
    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final sessionService = SessionService();
    api.stubString('crateFfiSessionSessionLoad', session ?? sessionJson);
    await sessionService.loadSession();

    final router = GoRouter(
      initialLocation: '/dating',
      routes: [
        GoRoute(path: '/dating', builder: (_, __) => const DatingScreen()),
        GoRoute(
          path: '/dating/me',
          builder: (_, __) =>
              const Scaffold(body: Text('dating me placeholder')),
        ),
        GoRoute(
          path: '/inbox/:pk',
          builder: (_, __) =>
              const Scaffold(body: Text('inbox placeholder')),
        ),
      ],
    );

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: sessionService),
          ChangeNotifierProvider(create: (_) => DatingService()),
        ],
        child: MaterialApp.router(routerConfig: router),
      ),
    );
    await tester.pumpAndSettle();
  }

  /// Shared load stubs: own profile present, one browse card, score 42%.
  void stubLoaded({String cards = cardJson, double score = 42.0}) {
    api.stubString('crateFfiDatingDatingGetOwnProfile', ownProfileJson);
    api.stubString('crateFfiDatingDatingFetchProfiles', cards);
    api.stub('crateFfiDatingDatingCalculateScore', (_) => score);
  }

  testWidgets('signed out state prompts sign in', (tester) async {
    await pumpScreen(
      tester,
      session: '{"active_pubkey":null,"accounts":[]}',
    );

    expect(find.text('Sign in to use Dating'), findsOneWidget);
  });

  testWidgets('no profile shows create CTA that pushes profile screen',
      (tester) async {
    api.stub('crateFfiDatingDatingGetOwnProfile', (_) {
      throw Exception('no profile');
    });
    api.stubString('crateFfiDatingDatingFetchProfiles', '[]');

    await pumpScreen(tester);

    expect(find.text('Create your dating profile to start'), findsOneWidget);

    await tester.tap(find.text('Create profile'));
    await tester.pumpAndSettle();
    expect(find.text('dating me placeholder'), findsOneWidget);
  });

  testWidgets('profile with no cards shows empty browse', (tester) async {
    stubLoaded(cards: '[]');

    await pumpScreen(tester);

    expect(find.text('No profiles nearby yet'), findsOneWidget);
    expect(find.text('Browse'), findsOneWidget);
    expect(find.text('Matches'), findsOneWidget);
    expect(find.text('Likes'), findsOneWidget);
  });

  testWidgets('browse renders card details and actions', (tester) async {
    stubLoaded();

    await pumpScreen(tester);

    expect(find.text('Alice'), findsOneWidget);
    expect(find.text('25'), findsOneWidget);
    expect(find.text('hello there'), findsOneWidget);
    expect(find.text('#hiking'), findsOneWidget);
    expect(find.text('#art'), findsOneWidget);
    expect(find.text('42% match'), findsOneWidget);
    expect(find.text('Block'), findsOneWidget);
    expect(find.text('Report'), findsOneWidget);
    expect(find.byTooltip('Pass'), findsOneWidget);
    expect(find.byTooltip('Like'), findsOneWidget);
    expect(find.byTooltip('Superlike'), findsOneWidget);
  });

  testWidgets('pass calls bridge with pubkeys', (tester) async {
    stubLoaded();
    api.stubBool('crateFfiDatingDatingPass', true);

    await pumpScreen(tester);

    await tester.tap(find.byTooltip('Pass'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiDatingDatingPass'), 1);
    final inv = api.callsOf('crateFfiDatingDatingPass').single;
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(api.namedArg(inv, 'profileId'), 'pkX');
    expect(find.text('No profiles nearby yet'), findsOneWidget);
  });

  testWidgets('like surfaces match overlay when peer liked back',
      (tester) async {
    stubLoaded();
    api.stubBool('crateFfiDatingDatingLike', true);
    // fetchLikesForMatch hits the same bridge fn as the Likes tab.
    api.stubString('crateFfiDatingDatingFetchLikes', cardJson);

    await pumpScreen(tester);

    await tester.tap(find.byTooltip('Like'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiDatingDatingLike'), 1);
    final inv = api.callsOf('crateFfiDatingDatingLike').single;
    expect(api.namedArg(inv, 'profileId'), 'pkX');
    expect(find.text("It's a Match!"), findsOneWidget);
    expect(find.text('You and Alice liked each other.'), findsOneWidget);

    await tester.tap(find.text('Keep Browsing'));
    await tester.pumpAndSettle();
    expect(find.text("It's a Match!"), findsNothing);
    expect(find.text('No profiles nearby yet'), findsOneWidget);
  });

  testWidgets('block calls bridge and shows snackbar', (tester) async {
    stubLoaded();
    api.stubBool('crateFfiDatingDatingBlockProfile', true);

    await pumpScreen(tester);

    await tester.tap(find.text('Block'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiDatingDatingBlockProfile'), 1);
    final inv = api.callsOf('crateFfiDatingDatingBlockProfile').single;
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(api.namedArg(inv, 'targetPubkey'), 'pkX');
    expect(find.text('Blocked'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('filter dialog applies age and interest filters', (tester) async {
    stubLoaded();
    api.stubString('crateFfiDatingDatingFilterProfiles', cardJson);

    await pumpScreen(tester);

    await tester.tap(find.byIcon(Icons.tune));
    await tester.pumpAndSettle();
    expect(find.text('Filter profiles'), findsOneWidget);

    await tester.enterText(find.byType(TextField).at(0), '25');
    await tester.enterText(find.byType(TextField).at(1), '35');
    await tester.enterText(find.byType(TextField).at(4), 'hiking, art');
    await tester.tap(find.widgetWithText(FilledButton, 'Apply'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiDatingDatingFilterProfiles'), 1);
    final inv = api.callsOf('crateFfiDatingDatingFilterProfiles').single;
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(api.namedArg(inv, 'minAge'), 25);
    expect(api.namedArg(inv, 'maxAge'), 35);
    expect(api.namedArg(inv, 'interestsJson'), '["hiking","art"]');
  });

  testWidgets('matches tab renders match and unmatch flow', (tester) async {
    stubLoaded();
    api.stubString('crateFfiDatingDatingFetchMatches', cardJson);
    api.stubBool('crateFfiDatingDatingUnmatch', true);

    await pumpScreen(tester);

    await tester.tap(find.text('Matches'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiDatingDatingFetchMatches'), 1);
    expect(find.text('Alice'), findsOneWidget);
    expect(find.text('Message'), findsOneWidget);
    expect(find.text('42% match'), findsOneWidget);

    await tester.tap(find.byTooltip('More'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Unmatch'));
    await tester.pumpAndSettle();
    expect(find.text('Unmatch?'), findsOneWidget);

    await tester.tap(find.widgetWithText(FilledButton, 'Unmatch'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiDatingDatingUnmatch'), 1);
    final inv = api.callsOf('crateFfiDatingDatingUnmatch').single;
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(api.namedArg(inv, 'profileId'), 'pkX');
    expect(find.text('Unmatched'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('likes tab match back likes peer and shows snackbar',
      (tester) async {
    stubLoaded();
    api.stubString('crateFfiDatingDatingFetchLikes', cardJson);
    api.stubBool('crateFfiDatingDatingLike', true);

    await pumpScreen(tester);

    await tester.tap(find.text('Likes'));
    await tester.pumpAndSettle();

    expect(find.text('Liked you'), findsOneWidget);

    await tester.tap(find.text('Match back'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 400));

    expect(api.callCount('crateFfiDatingDatingLike'), 1);
    final inv = api.callsOf('crateFfiDatingDatingLike').single;
    expect(api.namedArg(inv, 'profileId'), 'pkX');
    expect(find.textContaining("It's a match!"), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });
}