import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/settings_screen.dart';
import 'package:soshal_flutter/services/network_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/zap_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-settings');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  const sessionJson =
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,'
      '"relay_list":["wss://relay.example.com"]}]}';

  Future<void> pumpScreen(WidgetTester tester, {ZapService? zap}) async {
    tester.view.physicalSize = const Size(900, 2600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    await session.loadSession();

    final router = GoRouter(
      initialLocation: '/settings',
      routes: [
        GoRoute(path: '/settings', builder: (_, __) => const SettingsScreen()),
        for (final p in const [
          'backup',
          'turso',
          'blocked',
          'moderation',
          'security',
          'appearance',
          'language',
          'network',
          'notifications',
          'storage',
          'advanced',
        ])
          GoRoute(
            path: '/settings/$p',
            builder: (_, __) => Scaffold(body: Text('$p placeholder')),
          ),
        GoRoute(
          path: '/auth',
          builder: (_, __) => const Scaffold(body: Text('auth placeholder')),
        ),
      ],
    );

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: session),
          ChangeNotifierProvider<ZapService>.value(
            value: zap ?? ZapService(),
          ),
          ChangeNotifierProvider(create: (_) => NetworkService()),
        ],
        child: MaterialApp.router(routerConfig: router),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('renders all sections and navigation tiles', (tester) async {
    await pumpScreen(tester);

    expect(find.text('Settings'), findsOneWidget);
    expect(find.text('Account'), findsOneWidget);
    expect(find.text('Privacy & Security'), findsOneWidget);
    expect(find.text('Relays'), findsOneWidget);
    expect(find.text('Appearance'), findsNWidgets(2)); // section + tile
    expect(find.text('Lightning'), findsOneWidget);
    expect(find.text('Network'), findsOneWidget);
    expect(find.text('Notifications'), findsOneWidget);
    expect(find.text('About'), findsOneWidget);

    expect(find.text('Backup'), findsOneWidget);
    expect(find.text('Turso Database Sync'), findsOneWidget);
    expect(find.text('Share Soshal'), findsOneWidget);
    expect(find.text('Blocked Users'), findsOneWidget);
    expect(find.text('Moderation'), findsOneWidget);
    expect(find.text('Session Security'), findsOneWidget);
    expect(find.text('Relay Configuration'), findsOneWidget);
    expect(find.text('1 relay(s)'), findsOneWidget);
    expect(find.text('wss://relay.example.com'), findsOneWidget);
    expect(find.text('Add Relay'), findsOneWidget);
    expect(find.text('Language'), findsOneWidget);
    expect(find.text('NWC Wallet'), findsOneWidget);
    expect(find.text('Not connected'), findsOneWidget);
    expect(find.text('Resolve LNURL'), findsOneWidget);
    expect(find.text('Network Settings'), findsOneWidget);
    expect(find.text('Transports'), findsOneWidget);
    expect(find.text('Push Notifications'), findsOneWidget);
    expect(find.text('Storage'), findsOneWidget);
    expect(find.text('Advanced'), findsOneWidget);
    expect(find.text('App Version'), findsOneWidget);
    expect(find.text('0.1.0'), findsOneWidget);
    expect(find.text('Logout'), findsOneWidget);
  });

  testWidgets('navigation tile pushes its route', (tester) async {
    await pumpScreen(tester);

    await tester.tap(find.text('Backup'));
    await tester.pumpAndSettle();
    expect(find.text('backup placeholder'), findsOneWidget);
  });

  testWidgets('share soshal pushes share screen', (tester) async {
    await pumpScreen(tester);

    await tester.tap(find.text('Share Soshal'));
    await tester.pumpAndSettle();
    expect(find.text('Share Soshal'), findsOneWidget); // share appbar
    expect(find.textContaining('https://soshal.app/u/'), findsOneWidget);
  });

  testWidgets('logout dialog: cancel stays on settings', (tester) async {
    await pumpScreen(tester);

    await tester.tap(find.text('Logout'));
    await tester.pumpAndSettle();
    expect(find.text('Are you sure you want to logout?'), findsOneWidget);

    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(find.text('Settings'), findsOneWidget);
  });

  testWidgets('logout dialog: confirm goes to auth', (tester) async {
    await pumpScreen(tester);

    await tester.tap(find.text('Logout'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, 'Logout'));
    await tester.pumpAndSettle();

    expect(find.text('auth placeholder'), findsOneWidget);
  });

  testWidgets('add relay persists and publishes relay list', (tester) async {
    api.stubBool('crateFfiSessionSessionSave', true);
    api.stubString('crateFfiIdentityIdentityPublishRelayList', 'ok');

    await pumpScreen(tester);

    await tester.tap(find.text('Add Relay'));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byType(TextField),
      'wss://new.example.com',
    );
    await tester.tap(find.widgetWithText(TextButton, 'Add'));
    await tester.pumpAndSettle();

    expect(find.text('wss://new.example.com'), findsOneWidget);
    expect(find.text('2 relay(s)'), findsOneWidget);
    expect(api.callCount('crateFfiSessionSessionSave'), 1);
    expect(api.callCount('crateFfiIdentityIdentityPublishRelayList'), 1);
    final inv = api.callsOf('crateFfiIdentityIdentityPublishRelayList').single;
    final urls = api.namedArg(inv, 'relayUrls') as List<String>;
    expect(urls, containsAll(['wss://relay.example.com', 'wss://new.example.com']));
  });

  testWidgets('delete relay persists and publishes remaining list',
      (tester) async {
    api.stubBool('crateFfiSessionSessionSave', true);
    api.stubString('crateFfiIdentityIdentityPublishRelayList', 'ok');

    await pumpScreen(tester);

    await tester.tap(find.byIcon(Icons.delete));
    await tester.pumpAndSettle();

    expect(find.text('wss://relay.example.com'), findsNothing);
    expect(find.text('0 relay(s)'), findsOneWidget);
    expect(api.callCount('crateFfiSessionSessionSave'), 1);
    final inv = api.callsOf('crateFfiIdentityIdentityPublishRelayList').single;
    expect(api.namedArg(inv, 'relayUrls') as List<String>, isEmpty);
  });

  testWidgets('NWC connect dialog connects wallet and shows pubkey',
      (tester) async {
    api.stubBool('crateFfiZapZapConnectNwc', true);
    api.stubString('crateFfiZapZapGetNwcStatus', 'connected');
    api.stubString('crateFfiZapZapGetNwcPubkey', 'pkNWC1234567890abcdef');

    await pumpScreen(tester);

    await tester.tap(find.byIcon(Icons.add_link));
    await tester.pumpAndSettle();
    expect(find.text('Connect NWC wallet'), findsOneWidget);

    await tester.enterText(
      find.byType(TextField),
      'nostr+walletconnect://alice?relay=wss://relay.example.com&secret=x',
    );
    await tester.tap(find.widgetWithText(FilledButton, 'Connect'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiZapZapConnectNwc'), 1);
    expect(find.textContaining('Connected'), findsOneWidget);
  });

  testWidgets('NWC connect failure surfaces snackbar', (tester) async {
    api.stub('crateFfiZapZapConnectNwc', (_) {
      throw Exception('nwc unreachable');
    });

    await pumpScreen(tester);

    await tester.tap(find.byIcon(Icons.add_link));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'nostr+walletconnect://x');
    await tester.tap(find.widgetWithText(FilledButton, 'Connect'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Connect failed'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('NWC disconnect flow shows confirmation and snackbar',
      (tester) async {
    api.stubString('crateFfiZapZapGetNwcStatus', 'connected');
    api.stubString('crateFfiZapZapGetNwcPubkey', 'pkNWC1234567890abcdef');
    api.stubBool('crateFfiZapZapDisconnectNwc', true);

    final zap = ZapService();
    await zap.refreshStatus();

    await pumpScreen(tester, zap: zap);
    expect(find.textContaining('Connected'), findsOneWidget);

    await tester.tap(find.byIcon(Icons.link_off));
    await tester.pumpAndSettle();
    expect(find.text('Disconnect NWC wallet?'), findsOneWidget);

    await tester.tap(find.widgetWithText(FilledButton, 'Disconnect'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiZapZapDisconnectNwc'), 1);
    expect(find.text('NWC wallet disconnected'), findsOneWidget);
    expect(find.text('Not connected'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('NWC disconnect failure surfaces snackbar', (tester) async {
    api.stubString('crateFfiZapZapGetNwcStatus', 'connected');
    api.stubString('crateFfiZapZapGetNwcPubkey', 'pkNWC');
    api.stub('crateFfiZapZapDisconnectNwc', (_) {
      throw Exception('keychain busy');
    });

    final zap = ZapService();
    await zap.refreshStatus();

    await pumpScreen(tester, zap: zap);

    await tester.tap(find.byIcon(Icons.link_off));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Disconnect'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Disconnect failed'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('LNURL resolve shows resolved metadata dialog', (tester) async {
    api.stubString(
      'crateFfiZapZapParseLnurlMetadata',
      '{"name":"Alice","domain":"example.com","callback":"https://cb"}',
    );

    await pumpScreen(tester);

    await tester.tap(find.text('Resolve LNURL'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'alice@example.com');
    await tester.tap(find.widgetWithText(FilledButton, 'Resolve'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiZapZapParseLnurlMetadata'), 1);
    expect(find.text('LNURL resolved'), findsOneWidget);
    expect(find.text('Name: Alice'), findsOneWidget);
    expect(find.text('Domain: example.com'), findsOneWidget);
  });

  testWidgets('LNURL resolve failure surfaces snackbar', (tester) async {
    api.stub('crateFfiZapZapParseLnurlMetadata', (_) {
      throw Exception('bad lnurl');
    });

    await pumpScreen(tester);

    await tester.tap(find.text('Resolve LNURL'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'bad@example.com');
    await tester.tap(find.widgetWithText(FilledButton, 'Resolve'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Parse failed'), findsOneWidget);
    await tester.pump(const Duration(seconds: 5));
  });

  testWidgets('transports refresh updates I2P/Freenet status', (tester) async {
    api.stubBool('crateFfiNetworkNetworkI2PStatus', true);
    api.stubBool('crateFfiNetworkNetworkFreenetStatus', false);

    await pumpScreen(tester);
    expect(
      find.textContaining('I2P: unknown · Freenet: unknown'),
      findsOneWidget,
    );

    await tester.tap(find.byIcon(Icons.refresh));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiNetworkNetworkI2PStatus'), 1);
    expect(api.callCount('crateFfiNetworkNetworkFreenetStatus'), 1);
    expect(
      find.textContaining('I2P: up · Freenet: down'),
      findsOneWidget,
    );
  });
}