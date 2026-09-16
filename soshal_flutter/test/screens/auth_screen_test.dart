// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/auth_screen.dart';
import 'package:soshal_flutter/services/auth_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/sync_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('widget-docs');
  api = env.$1;

  testWidgets('auth flow walks welcome -> generate -> confirm mnemonic',
      (tester) async {
    api.handlers.clear();
    const phrase =
        'abandon abandon abandon abandon abandon abandon abandon '
        'abandon abandon abandon abandon about';
    api.stubString('crateFfiAuthAuthGenerateMnemonic', phrase);

    final router = GoRouter(
      initialLocation: '/auth',
      routes: [
        GoRoute(path: '/auth', builder: (_, __) => const AuthScreen()),
        GoRoute(
          path: '/feed',
          builder: (_, __) => const Scaffold(body: Text('feed placeholder')),
        ),
      ],
    );

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider(create: (_) => AuthService()),
          ChangeNotifierProvider(create: (_) => SessionService()),
          ChangeNotifierProvider(create: (_) => SyncService()),
        ],
        child: MaterialApp.router(routerConfig: router),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Welcome to Soshal'), findsOneWidget);
    await tester.tap(find.text('Get Started'));
    await tester.pumpAndSettle();

    expect(find.text('Create or Import?'), findsOneWidget);
    await tester.tap(find.text('Generate New Key'));
    await tester.pumpAndSettle();

    // Mnemonic generated through the fake FFI api and displayed.
    expect(find.textContaining('abandon'), findsOneWidget);
    expect(api.callCount('crateFfiAuthAuthGenerateMnemonic'), 1);
  });

  testWidgets('auth flow offers Import from friends\' cache',
      (tester) async {
    api.handlers.clear();

    final router = GoRouter(
      initialLocation: '/auth',
      routes: [
        GoRoute(path: '/auth', builder: (_, __) => const AuthScreen()),
      ],
    );

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider(create: (_) => AuthService()),
          ChangeNotifierProvider(create: (_) => SessionService()),
          ChangeNotifierProvider(create: (_) => SyncService()),
        ],
        child: MaterialApp.router(routerConfig: router),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.text('Get Started'));
    await tester.pumpAndSettle();

    expect(find.text('Import from friends\' cache'), findsOneWidget);
    await tester.tap(find.text('Import from friends\' cache'));
    await tester.pumpAndSettle();

    expect(find.text('Import from Friends\' Cache'), findsOneWidget);
    expect(
      find.text(
        'Enter your recovery phrase to restore your account from your friends\' cache.',
      ),
      findsOneWidget,
    );
  });
}