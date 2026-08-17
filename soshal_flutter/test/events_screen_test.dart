import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/events_screen.dart';
import 'package:soshal_flutter/services/dating_service.dart';
import 'package:soshal_flutter/services/events_service.dart';
import 'package:soshal_flutter/services/session_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-events');
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

  const eventJson =
      '{"id":"ev1","creator_pubkey":"pk123","title":"Jazz Night",'
      '"description":"live jazz","location":"","latitude":0,"longitude":0,'
      '"start_time":0,"end_time":0,"image":"","attendees":3,'
      '"rsvp_status":"","created_at":0}';

  Future<void> pumpScreen(WidgetTester tester) async {
    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    await session.loadSession();

    // Default load stubs — only when the test did not register its own.
    void stubDefault(String method, String result) {
      if (!api.handlers.containsKey(Symbol(method))) {
        api.stubString(method, result);
      }
    }

    stubDefault('crateFfiEventsEventsFetchNearby', '[]');
    stubDefault('crateFfiDatingDatingGetOwnProfile', ownProfileJson);
    if (!api.handlers.containsKey(Symbol('crateFfiUtilUtilExtractHashtags'))) {
      api.stub('crateFfiUtilUtilExtractHashtags', (_) => <String>[]);
    }

    final router = GoRouter(
      initialLocation: '/events',
      routes: [
        GoRoute(path: '/events', builder: (_, __) => const EventsScreen()),
        GoRoute(
          path: '/events/:eventId',
          builder: (_, state) =>
              EventDetailScreen(eventId: state.pathParameters['eventId']!),
        ),
      ],
    );

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: session),
          ChangeNotifierProvider(create: (_) => EventsService()),
          ChangeNotifierProvider(create: (_) => DatingService()),
        ],
        child: MaterialApp.router(routerConfig: router),
      ),
    );
    await tester.pumpAndSettle();
  }

  Future<void> pumpDetail(WidgetTester tester) async {
    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    await session.loadSession();

    // Default detail stubs — only when the test did not register its own.
    void stubDefault(String method, String result) {
      if (!api.handlers.containsKey(Symbol(method))) {
        api.stubString(method, result);
      }
    }

    stubDefault('crateFfiEventsEventsGetEvent', eventJson);
    stubDefault('crateFfiEventsEventsRemindersList', '[]');
    if (!api.handlers.containsKey(Symbol('crateFfiEventsEventsGetAttendees'))) {
      api.stubListString('crateFfiEventsEventsGetAttendees', []);
    }

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: session),
          ChangeNotifierProvider(create: (_) => EventsService()),
          ChangeNotifierProvider(create: (_) => DatingService()),
        ],
        child: const MaterialApp(home: EventDetailScreen(eventId: 'ev1')),
      ),
    );
    await tester.pumpAndSettle();
  }

  /// The nearby-mode bottom bar (radius Slider) fills the whole viewport
  /// height on Flutter 3.27+ sliders, collapsing the body — toggle to
  /// "Mine" (hides the bar) before interacting with the list.
  Future<void> pumpMine(WidgetTester tester) async {
    await pumpScreen(tester);
    await tester.tap(find.text('Mine'));
    await tester.pumpAndSettle();
  }

  testWidgets('empty state shows no events yet with controls',
      (tester) async {
    api.stubString('crateFfiEventsEventsFetchUserEvents', '[]');

    await pumpMine(tester);

    expect(find.text('No events yet'), findsOneWidget);
    expect(find.byTooltip('Create event'), findsOneWidget);
    expect(find.text('All'), findsOneWidget);
    expect(find.text('List'), findsOneWidget);
    expect(find.text('Calendar'), findsOneWidget);
    expect(find.byType(Slider), findsNothing);
  });

  testWidgets('renders event rows with location time and attendees',
      (tester) async {
    api.stubString('crateFfiEventsEventsFetchUserEvents', '[$eventJson]');
    api.stub('crateFfiUtilUtilExtractHashtags', (_) => <String>['tag']);
    api.stub('crateFfiEventsEventsInterestScore',
        (_) => '{"score":42,"common":[]}');

    await pumpMine(tester);

    expect(find.text('Jazz Night'), findsOneWidget);
    expect(find.text('Remote · flexible · 3 going'), findsOneWidget);
    expect(api.callCount('crateFfiEventsEventsInterestScore'), 1);
  });

  testWidgets('load failure shows empty state without crash', (tester) async {
    api.stub('crateFfiEventsEventsFetchUserEvents', (_) {
      throw Exception('relay down');
    });

    await pumpMine(tester);

    expect(find.text('No events yet'), findsOneWidget);
  });

  testWidgets('create dialog creates event with entered fields',
      (tester) async {
    api.stubString('crateFfiEventsEventsFetchUserEvents', '[]');
    api.stubString('crateFfiEventsEventsCreate', 'ev9');

    await pumpMine(tester);

    await tester.tap(find.byType(FloatingActionButton));
    await tester.pumpAndSettle();
    expect(find.text('Create event'), findsOneWidget);

    final fields = find.descendant(
        of: find.byType(AlertDialog), matching: find.byType(TextField));
    await tester.enterText(fields.at(0), 'Potluck');
    await tester.enterText(fields.at(1), 'bring a dish');
    await tester.enterText(fields.at(2), 'Riverside Park');
    await tester.tap(find.widgetWithText(FilledButton, 'Create'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiEventsEventsCreate'), 1);
    final inv = api.callsOf('crateFfiEventsEventsCreate').single;
    expect(api.namedArg(inv, 'creatorPubkey'), 'pk123');
    expect(api.namedArg(inv, 'title'), 'Potluck');
    expect(api.namedArg(inv, 'description'), 'bring a dish');
    expect(api.namedArg(inv, 'location'), 'Riverside Park');
    expect(api.namedArg(inv, 'latitude'), 0.0);
    expect(api.namedArg(inv, 'longitude'), 0.0);
    expect(api.namedArg(inv, 'imageUrl'), '');
    expect(api.namedArg(inv, 'startTime') as BigInt, greaterThan(BigInt.zero));
    expect(api.callCount('crateFfiEventsEventsFetchUserEvents'), 2);
  });

  testWidgets('create failure surfaces snackbar', (tester) async {
    api.stubString('crateFfiEventsEventsFetchUserEvents', '[]');
    api.stub('crateFfiEventsEventsCreate', (_) {
      throw Exception('publish failed');
    });

    await pumpMine(tester);

    await tester.tap(find.byType(FloatingActionButton));
    await tester.pumpAndSettle();
    await tester.enterText(
        find
            .descendant(
                of: find.byType(AlertDialog),
                matching: find.byType(TextField))
            .at(0),
        'Broken');
    await tester.tap(find.widgetWithText(FilledButton, 'Create'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Create failed'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('mine toggle fetches user events', (tester) async {
    api.stubString('crateFfiEventsEventsFetchUserEvents', '[]');

    await pumpScreen(tester);

    await tester.tap(find.text('Mine'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiEventsEventsFetchUserEvents'), 1);
    final inv = api.callsOf('crateFfiEventsEventsFetchUserEvents').single;
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(find.text('All'), findsOneWidget);
    expect(find.byType(Slider), findsNothing);
  });

  testWidgets('calendar view shows month grid and day selection',
      (tester) async {
    const monthNames = [
      'January',
      'February',
      'March',
      'April',
      'May',
      'June',
      'July',
      'August',
      'September',
      'October',
      'November',
      'December',
    ];
    api.stubString('crateFfiEventsEventsFetchUserEvents', '[]');

    await pumpMine(tester);

    await tester.tap(find.text('Calendar'));
    await tester.pumpAndSettle();

    final now = DateTime.now();
    expect(find.text('${monthNames[now.month - 1]} ${now.year}'),
        findsOneWidget);
    expect(find.text('Sun'), findsOneWidget);
    expect(find.text('Mon'), findsOneWidget);

    await tester.tap(find.text('15'));
    await tester.pumpAndSettle();
    expect(find.text('No events this day.'), findsOneWidget);
  });

  testWidgets('event tap opens detail and RSVP calls bridge',
      (tester) async {
    api.stubString('crateFfiEventsEventsFetchUserEvents', '[$eventJson]');
    api.stubString('crateFfiEventsEventsGetEvent', eventJson);
    api.stubString('crateFfiEventsEventsRemindersList', '[]');
    api.stubListString('crateFfiEventsEventsGetAttendees', ['pkA', 'pkB']);
    api.stubBool('crateFfiEventsEventsRsvp', true);

    await pumpMine(tester);

    await tester.tap(find.text('Jazz Night'));
    await tester.pumpAndSettle();

    expect(find.text('3 attending · 2 attending'), findsOneWidget);
    expect(find.widgetWithText(FilledButton, 'Going'), findsOneWidget);
    expect(find.widgetWithText(OutlinedButton, 'Not going'), findsOneWidget);
    expect(find.widgetWithText(OutlinedButton, 'Maybe'), findsOneWidget);
    expect(find.widgetWithText(OutlinedButton, 'Check in'), findsOneWidget);

    await tester.tap(find.widgetWithText(FilledButton, 'Going'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiEventsEventsRsvp'), 1);
    final inv = api.callsOf('crateFfiEventsEventsRsvp').single;
    expect(api.namedArg(inv, 'eventId'), 'ev1');
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(api.namedArg(inv, 'rsvpStatus'), 'accepted');
    expect(find.text('accepted'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('rsvp failure surfaces snackbar', (tester) async {
    api.stub('crateFfiEventsEventsRsvp', (_) {
      throw Exception('relay down');
    });

    await pumpDetail(tester);

    await tester.tap(find.widgetWithText(FilledButton, 'Going'));
    await tester.pumpAndSettle();

    expect(find.textContaining('RSVP failed'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('check in calls bridge and shows confirmation', (tester) async {
    api.stubBool('crateFfiEventsEventsCheckIn', true);

    await pumpDetail(tester);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Check in'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiEventsEventsCheckIn'), 1);
    final inv = api.callsOf('crateFfiEventsEventsCheckIn').single;
    expect(api.namedArg(inv, 'eventId'), 'ev1');
    expect(api.namedArg(inv, 'userPubkey'), 'pk123');
    expect(api.namedArg(inv, 'latitude'), 0.0);
    expect(api.namedArg(inv, 'longitude'), 0.0);
    expect(find.text('Checked in!'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('add reminder dialog upserts reminder for event',
      (tester) async {
    api.stubString('crateFfiEventsEventsReminderUpsert', 'r1');

    await pumpDetail(tester);

    await tester.tap(find.widgetWithText(TextButton, 'Add'));
    await tester.pumpAndSettle();
    expect(find.text('Add reminder'), findsOneWidget);

    await tester.tap(find.widgetWithText(FilledButton, 'Save'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiEventsEventsReminderUpsert'), 1);
    final inv = api.callsOf('crateFfiEventsEventsReminderUpsert').single;
    expect(api.namedArg(inv, 'reminderId'), '');
    expect(api.namedArg(inv, 'eventId'), 'ev1');
    expect(api.namedArg(inv, 'title'), 'Jazz Night');
    expect(api.namedArg(inv, 'minutesBefore'), 10);
  });

  testWidgets('delete reminder calls bridge', (tester) async {
    api.stubString(
        'crateFfiEventsEventsRemindersList',
        '[{"id":"r1","event_id":"ev1","title":"Heads up","start_time":0,'
        '"minutes_before":10,"created_at":0}]');
    api.stubBool('crateFfiEventsEventsReminderDelete', true);

    await pumpDetail(tester);

    expect(find.text('Heads up'), findsOneWidget);
    expect(find.textContaining('10 min'), findsOneWidget);

    await tester.tap(find.byTooltip('Delete reminder'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiEventsEventsReminderDelete'), 1);
    final inv = api.callsOf('crateFfiEventsEventsReminderDelete').single;
    expect(api.namedArg(inv, 'reminderId'), 'r1');
    // Service refetches inside deleteReminder + screen refresh.
    expect(api.callCount('crateFfiEventsEventsRemindersList'), 3);
  });

  testWidgets('missing event shows not found', (tester) async {
    api.stub('crateFfiEventsEventsGetEvent', (_) {
      throw Exception('no such event');
    });

    await pumpDetail(tester);

    expect(find.text('Event not found'), findsOneWidget);
  });

  testWidgets('attendees modal lists attendees', (tester) async {
    api.stubListString('crateFfiEventsEventsGetAttendees', ['pkA', 'pkB']);

    await pumpDetail(tester);

    await tester.tap(find.text('3 attending · 2 attending'));
    await tester.pumpAndSettle();

    expect(find.text('Attendees · Jazz Night'), findsOneWidget);
    expect(find.text('2 attending'), findsWidgets);
    await tester.tap(find.widgetWithText(TextButton, 'Close'));
    await tester.pumpAndSettle();
  });
}