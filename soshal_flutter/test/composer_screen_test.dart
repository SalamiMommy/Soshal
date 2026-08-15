// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/composer_screen.dart';
import 'package:soshal_flutter/services/feed_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/signer_service.dart';

import 'helpers/test_env.dart';

/// SessionService with a controllable active pubkey (field setter is private).
class FakeSession extends SessionService {
  FakeSession({this.pubkey});

  final String? pubkey;

  @override
  String? get activePubkey => pubkey;
}

const _pubkey =
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';

late FakeApi api;

Future<void> pumpComposer(WidgetTester tester, {String? sessionPubkey}) async {
  final navKey = GlobalKey<NavigatorState>();
  await tester.pumpWidget(
    MultiProvider(
      providers: [
        ChangeNotifierProvider(create: (_) => FeedService()),
        ChangeNotifierProvider<SessionService>(
          create: (_) => FakeSession(pubkey: sessionPubkey),
        ),
        ChangeNotifierProvider(create: (_) => SignerService()),
      ],
      child: MaterialApp(
        navigatorKey: navKey,
        home: const Scaffold(
          body: Center(child: Text('root placeholder')),
        ),
      ),
    ),
  );
  navKey.currentState!.push(
    MaterialPageRoute(
      builder: (_) => const Scaffold(body: ComposerScreen()),
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
  final env = bootstrapTestEnv('widget-composer');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
    api.stubListString('crateFfiUtilUtilExtractHashtags', []);
    api.stubBool('crateFfiSignerSignerIsLocked', false);
  });

  testWidgets('renders composer with input, header and media affordance',
      (tester) async {
    await pumpComposer(tester);

    expect(find.text('New Post'), findsOneWidget);
    expect(find.text("What's happening?"), findsOneWidget);
    expect(find.byType(TextField), findsOneWidget);
    expect(find.byIcon(Icons.attach_file), findsOneWidget);
  });

  testWidgets('typing detects hashtags and shows tag chips', (tester) async {
    api.stubListString(
      'crateFfiUtilUtilExtractHashtags',
      ['flutter', 'dart'],
    );

    await pumpComposer(tester);

    await tester.enterText(find.byType(TextField), '#flutter #dart hello');
    await tester.pump(const Duration(milliseconds: 301));

    expect(find.text('#flutter'), findsOneWidget);
    expect(find.text('#dart'), findsOneWidget);
  });

  testWidgets('publish with empty text shows error and does not publish',
      (tester) async {
    await pumpComposer(tester);

    await tester.tap(find.text('Publish'));
    await tester.pump();

    expect(find.text('Post content cannot be empty'), findsOneWidget);
    expect(api.callCount('crateFfiFeedFeedPublishTextNote'), 0);

    await flushSnackBars(tester);
  });

  testWidgets('valid publish calls service and pops the composer',
      (tester) async {
    api.stubBool('crateFfiFeedFeedValidateNote', true);
    api.stub('crateFfiFeedFeedPublishTextNote', (_) async => 'n-1');

    await pumpComposer(tester, sessionPubkey: _pubkey);

    await tester.enterText(find.byType(TextField), 'hello world');
    await tester.pump();
    await tester.tap(find.text('Publish'));
    await tester.pumpAndSettle();

    expect(find.text('root placeholder'), findsOneWidget);
    expect(find.text('Publish'), findsNothing);
    expect(find.text('Post published!'), findsOneWidget);
    expect(api.callCount('crateFfiFeedFeedPublishTextNote'), 1);
    final inv = api.callsOf('crateFfiFeedFeedPublishTextNote').single;
    expect(api.namedArg(inv, 'content'), 'hello world');
    expect(api.namedArg(inv, 'tagsJson'), '[]');

    await flushSnackBars(tester);
  });

  testWidgets('rejected content shows validator error and does not publish',
      (tester) async {
    api.stubBool('crateFfiFeedFeedValidateNote', false);

    await pumpComposer(tester, sessionPubkey: _pubkey);

    await tester.enterText(find.byType(TextField), 'bad content');
    await tester.pump();
    await tester.tap(find.text('Publish'));
    await tester.pump();

    expect(find.text('Note rejected by content validator'), findsOneWidget);
    expect(api.callCount('crateFfiFeedFeedPublishTextNote'), 0);
    expect(find.text('Publish'), findsOneWidget); // composer stays open

    await flushSnackBars(tester);
  });

  testWidgets('publish without active account errors and does not publish',
      (tester) async {
    api.stubBool('crateFfiFeedFeedValidateNote', true);

    await pumpComposer(tester);

    await tester.enterText(find.byType(TextField), 'hello world');
    await tester.pump();
    await tester.tap(find.text('Publish'));
    await tester.pump();

    expect(find.textContaining('No active account'), findsOneWidget);
    expect(api.callCount('crateFfiFeedFeedPublishTextNote'), 0);

    await flushSnackBars(tester);
  });
}