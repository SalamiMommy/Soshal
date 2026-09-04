import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/splash_screen.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';
import 'package:soshal_flutter/services/shell_service.dart';
import 'package:soshal_flutter/services/signer_service.dart';
import 'package:soshal_flutter/services/sync_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-splash');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
    api.stubString('crateFfiDbDbGetSetting', '');
    api.stubBool('crateFfiDbDbSetSetting', true);
  });

  Future<void> pumpSplash(WidgetTester tester) async {
    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final router = GoRouter(
      initialLocation: '/splash',
      routes: [
        GoRoute(path: '/splash', builder: (_, __) => const SplashScreen()),
        GoRoute(
          path: '/auth',
          builder: (_, __) => const Scaffold(body: Text('auth placeholder')),
        ),
      ],
    );

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider(create: (_) => SessionService()),
          ChangeNotifierProvider(create: (_) => SignerService()),
          ChangeNotifierProvider(create: (_) => ShellService()),
          ChangeNotifierProvider(create: (_) => SyncService()),
          ChangeNotifierProvider(create: (_) => SettingsService()),
        ],
        child: MaterialApp.router(routerConfig: router),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('bridge init failure surfaces initialization error snackbar',
      (tester) async {
    // FfiBridge.init() always fails under test: RustLib is already
    // mock-initialized and the cdylib is absent, so the splash catch-path
    // (snackbar + /auth) is the only branch reachable with the FakeApi
    // harness. Session/signer/sync routing needs a real bridge init.
    await pumpSplash(tester);

    expect(find.textContaining('Initialization error'), findsOneWidget);
  });

  testWidgets('init failure routes to auth', (tester) async {
    await pumpSplash(tester);

    expect(find.text('auth placeholder'), findsOneWidget);
  });
}