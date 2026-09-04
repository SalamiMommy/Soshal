import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/accounts_screen.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';
import 'package:soshal_flutter/services/shell_service.dart';
import 'package:soshal_flutter/services/signer_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-accounts');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
    api.stubBool('crateFfiSignerSignerLock', true);
    api.stubBool('crateFfiSignerSignerUnlockFromKeyring', true);
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stubString('crateFfiDbDbGetSetting', 'false');
  });

  const twoAccountsJson =
      '{"active_pubkey":"pk123","accounts":['
      '{"pubkey":"pk123","npub":"npub1abc","last_used":0,"relay_list":[]},'
      '{"pubkey":"pk456","npub":"npub1xyz","last_used":1,"relay_list":[]}]}';

  const oneAccountJson =
      '{"active_pubkey":"pk123","accounts":['
      '{"pubkey":"pk123","npub":"npub1abc","last_used":0,"relay_list":[]}]}';

  Future<void> pumpScreen(
    WidgetTester tester, {
    SessionService? session,
    String? sessionJson,
  }) async {
    tester.view.physicalSize = const Size(800, 1600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final s = session ?? SessionService();
    if (sessionJson != null) {
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      await s.loadSession();
    }

    final router = GoRouter(
      initialLocation: '/accounts',
      routes: [
        GoRoute(path: '/accounts', builder: (_, __) => const AccountsScreen()),
        GoRoute(
          path: '/auth',
          builder: (_, __) => const Scaffold(body: Text('auth placeholder')),
        ),
      ],
    );

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: s),
          ChangeNotifierProvider(create: (_) => SignerService()),
          ChangeNotifierProvider(create: (_) => ShellService()),
          ChangeNotifierProvider(create: (_) => SettingsService()),
        ],
        child: MaterialApp.router(routerConfig: router),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('empty state shows add account gate to auth', (tester) async {
    await pumpScreen(tester);

    expect(find.text('No accounts yet'), findsOneWidget);
    await tester.tap(find.widgetWithText(FilledButton, 'Add account'));
    await tester.pumpAndSettle();
    expect(find.text('auth placeholder'), findsOneWidget);
  });

  testWidgets('renders account list with active marker', (tester) async {
    await pumpScreen(tester, sessionJson: twoAccountsJson);

    expect(find.text('npub1abc'), findsOneWidget);
    expect(find.text('npub1xyz'), findsOneWidget);
    // Active account: check icon, no Switch button.
    expect(
      find.descendant(
        of: find.widgetWithText(ListTile, 'npub1abc'),
        matching: find.byIcon(Icons.check_circle),
      ),
      findsOneWidget,
    );
    expect(find.text('Switch'), findsOneWidget);
    expect(find.byIcon(Icons.delete_outline), findsNWidgets(2));
  });

  testWidgets('switch account swaps active marker', (tester) async {
    api.stubBool('crateFfiSessionSessionSwitchAccount', true);

    await pumpScreen(tester, sessionJson: twoAccountsJson);

    await tester.tap(find.text('Switch'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiSessionSessionSwitchAccount'), 1);
    final inv = api.callsOf('crateFfiSessionSessionSwitchAccount').single;
    expect(api.namedArg(inv, 'pubkey'), 'pk456');
    // Switch button moved to the now-inactive account.
    expect(
      find.descendant(
        of: find.widgetWithText(ListTile, 'npub1abc'),
        matching: find.text('Switch'),
      ),
      findsOneWidget,
    );
    expect(
      find.descendant(
        of: find.widgetWithText(ListTile, 'npub1xyz'),
        matching: find.byIcon(Icons.check_circle),
      ),
      findsOneWidget,
    );
  });

  testWidgets('remove account: cancel keeps account', (tester) async {
    await pumpScreen(tester, sessionJson: twoAccountsJson);

    await tester.tap(find.byIcon(Icons.delete_outline).first);
    await tester.pumpAndSettle();
    expect(find.text('Remove account?'), findsOneWidget);
    expect(find.textContaining('Key material stays'), findsOneWidget);

    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();

    expect(find.text('npub1abc'), findsOneWidget);
    expect(find.text('npub1xyz'), findsOneWidget);
  });

  testWidgets('remove inactive account removes it', (tester) async {
    await pumpScreen(tester, sessionJson: twoAccountsJson);

    await tester.tap(find.byIcon(Icons.delete_outline).last);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Remove'));
    await tester.pumpAndSettle();

    expect(find.text('npub1xyz'), findsNothing);
    expect(find.text('npub1abc'), findsOneWidget);
  });

  testWidgets('remove active account falls back to remaining account',
      (tester) async {
    await pumpScreen(tester, sessionJson: twoAccountsJson);

    await tester.tap(find.byIcon(Icons.delete_outline).first);
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Remove'));
    await tester.pumpAndSettle();

    expect(find.text('npub1abc'), findsNothing);
    expect(
      find.descendant(
        of: find.widgetWithText(ListTile, 'npub1xyz'),
        matching: find.byIcon(Icons.check_circle),
      ),
      findsOneWidget,
    );
  });

  testWidgets('remove last account shows empty state', (tester) async {
    await pumpScreen(tester, sessionJson: oneAccountJson);

    await tester.tap(find.byIcon(Icons.delete_outline));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Remove'));
    await tester.pumpAndSettle();

    expect(find.text('No accounts yet'), findsOneWidget);
  });

  testWidgets('refresh from Rust syncs accounts and shows snackbar',
      (tester) async {
    api.stubString('crateFfiSessionSessionGetActive', '{"pubkey":"pk123"}');
    api.stubString(
      'crateFfiSessionSessionListAccounts',
      '[{"pubkey":"pk123","npub":"npub1abc","last_used":0,"relay_list":[]},'
      '{"pubkey":"pk456","npub":"npub1xyz","last_used":1,"relay_list":[]}]',
    );

    await pumpScreen(tester, sessionJson: twoAccountsJson);

    await tester.tap(find.byIcon(Icons.sync));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiSessionSessionGetActive'), 1);
    expect(api.callCount('crateFfiSessionSessionListAccounts'), 1);
    expect(find.text('Synced 2 account(s) from Rust'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('refresh from Rust failure surfaces snackbar', (tester) async {
    api.stub('crateFfiSessionSessionGetActive', (_) {
      throw Exception('session file corrupt');
    });

    await pumpScreen(tester, sessionJson: twoAccountsJson);

    await tester.tap(find.byIcon(Icons.sync));
    await tester.pumpAndSettle();

    expect(find.textContaining('Refresh error'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });
}