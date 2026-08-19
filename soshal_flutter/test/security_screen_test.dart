import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/security_screen.dart';
import 'package:soshal_flutter/services/auth_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';
import 'package:soshal_flutter/services/shell_service.dart';
import 'package:soshal_flutter/services/signer_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-security');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
    api.stubString('crateFfiDbDbGetSetting', '');
    api.stubBool('crateFfiDbDbSetSetting', true);
  });

  const sessionJson =
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,'
      '"relay_list":["wss://relay.example.com"]}]}';

  Future<void> pumpScreen(WidgetTester tester, {ShellService? shell}) async {
    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    await session.loadSession();
    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: session),
          ChangeNotifierProvider(create: (_) => SignerService()),
          ChangeNotifierProvider<ShellService>.value(
            value: shell ?? ShellService(),
          ),
          ChangeNotifierProvider(create: (_) => AuthService()),
          ChangeNotifierProvider(create: (_) => SettingsService()),
        ],
        child: const MaterialApp(home: SecurityScreen()),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('renders key sections and locked key state', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', true);

    await pumpScreen(tester);

    expect(find.text('Key storage'), findsOneWidget);
    expect(find.text('Screen capture'), findsOneWidget);
    expect(find.text('App lock PIN'), findsOneWidget);
    expect(find.text('Encrypted DMs'), findsOneWidget);
    expect(find.text('Signer pubkey'), findsOneWidget);
    expect(find.text('Locked (keys wiped)'), findsOneWidget);
    expect(find.text('Key tools'), findsOneWidget);
    expect(find.text('Crypto tools'), findsOneWidget);
  });

  testWidgets('shows unlocked key state', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);

    await pumpScreen(tester);

    expect(find.text('Unlocked'), findsOneWidget);
    expect(find.text('Locked (keys wiped)'), findsNothing);
  });

  testWidgets('lock flow: confirm dialog locks signer', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', true);
    api.stubBool('crateFfiSignerSignerLock', true);

    await pumpScreen(tester);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Lock'));
    await tester.pumpAndSettle();
    expect(find.text('Lock session?'), findsOneWidget);

    await tester.tap(find.widgetWithText(FilledButton, 'Lock'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiSignerSignerLock'), 1);
    expect(find.text('Locked (keys wiped)'), findsOneWidget);
  });

  testWidgets('lock flow: cancel does not lock', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);

    await pumpScreen(tester);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Lock'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiSignerSignerLock'), 0);
  });

  testWidgets('keychain save/unlock/remove call signer with pubkey',
      (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stubString('crateFfiDbDbGetSetting', 'true');
    api.stubBool('crateFfiSignerSignerSaveToKeyring', true);
    api.stubBool('crateFfiSignerSignerUnlockFromKeyring', true);
    api.stubBool('crateFfiSignerSignerRemoveFromKeyring', true);

    await pumpScreen(tester);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Save'));
    await tester.pumpAndSettle();
    expect(api.callCount('crateFfiSignerSignerSaveToKeyring'), 1);
    expect(find.textContaining('Saved to keychain'), findsOneWidget);
    var inv = api.callsOf('crateFfiSignerSignerSaveToKeyring').single;
    expect(api.namedArg(inv, 'pubkey'), 'pk123');

    await tester.pump(const Duration(seconds: 5));
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(OutlinedButton, 'Unlock'));
    await tester.pumpAndSettle();
    expect(api.callCount('crateFfiSignerSignerUnlockFromKeyring'), 1);
    expect(find.textContaining('Unlocked from keychain'), findsOneWidget);
    inv = api.callsOf('crateFfiSignerSignerUnlockFromKeyring').single;
    expect(api.namedArg(inv, 'pubkey'), 'pk123');

    await tester.pump(const Duration(seconds: 5));
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(OutlinedButton, 'Remove'));
    await tester.pumpAndSettle();
    expect(api.callCount('crateFfiSignerSignerRemoveFromKeyring'), 1);
    expect(find.textContaining('Removed from keychain'), findsOneWidget);
    inv = api.callsOf('crateFfiSignerSignerRemoveFromKeyring').single;
    expect(api.namedArg(inv, 'pubkey'), 'pk123');
  });

  testWidgets('sign message dialog shows signature', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stubString('crateFfiSignerSignerSignText', 'sig123');

    await pumpScreen(tester);

    await tester.tap(find.text('Sign message'));
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField), 'hello world');
    await tester.tap(find.widgetWithText(FilledButton, 'Sign'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiSignerSignerSignText'), 1);
    expect(find.textContaining('sig123'), findsOneWidget);
  });

  testWidgets('sign message dialog surfaces error', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stub('crateFfiSignerSignerSignText', (_) {
      throw Exception('signer locked');
    });

    await pumpScreen(tester);

    await tester.tap(find.text('Sign message'));
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField), 'hello');
    await tester.tap(find.widgetWithText(FilledButton, 'Sign'));
    await tester.pumpAndSettle();

    expect(find.textContaining('signer locked'), findsOneWidget);
  });

  testWidgets('generate keypair dialog shows pubkey and nsec',
      (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stubString(
      'crateFfiAuthAuthGenerateKeypair',
      '{"publicKey":"pkgen","secretKey":"nsecgen"}',
    );

    await pumpScreen(tester);

    await tester.tap(find.descendant(
      of: find.widgetWithText(ListTile, 'Generate new keypair'),
      matching: find.byType(OutlinedButton),
    ));
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(FilledButton, 'Generate'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiAuthAuthGenerateKeypair'), 1);
    expect(find.text('pkgen'), findsOneWidget);
    expect(find.text('nsecgen'), findsOneWidget);
  });

  testWidgets('generate keypair dialog surfaces error', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stub('crateFfiAuthAuthGenerateKeypair', (_) {
      throw Exception('keygen failed');
    });

    await pumpScreen(tester);

    await tester.tap(find.descendant(
      of: find.widgetWithText(ListTile, 'Generate new keypair'),
      matching: find.byType(OutlinedButton),
    ));
    await tester.pumpAndSettle();

    await tester.tap(find.widgetWithText(FilledButton, 'Generate'));
    await tester.pumpAndSettle();

    expect(find.textContaining('keygen failed'), findsOneWidget);
  });

  testWidgets('derive pubkey dialog returns npub', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stubString('crateFfiAuthAuthPublicKeyFromNsec', 'hexpk');
    api.stubString('crateFfiAuthAuthNpubEncode', 'npub1derived');

    await pumpScreen(tester);

    await tester.tap(find.descendant(
      of: find.widgetWithText(ListTile, 'Derive pubkey from nsec'),
      matching: find.byType(OutlinedButton),
    ));
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField), 'nsec1test');
    // The dialog's FilledButton is enabled only after the StatefulBuilder
    // re-evaluates its closure (typing alone does not rebuild it — latent
    // app quirk). Force the rebuild, then tap.
    tester
        .element(find.ancestor(
          of: find.byType(AlertDialog),
          matching: find.byType(StatefulBuilder),
        ))
        .markNeedsBuild();
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Derive'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiAuthAuthPublicKeyFromNsec'), 1);
    expect(api.callCount('crateFfiAuthAuthNpubEncode'), 1);
    expect(find.text('npub1derived'), findsOneWidget);
  });

  testWidgets('schnorr sign digest dialog signs the digest', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stubString('crateFfiUtilUtilSha256Hex', 'digesthex');
    api.stubString('crateFfiSignerSignerSchnorrSign', 'schnorrsig1');

    await pumpScreen(tester);

    await tester.tap(find.descendant(
      of: find.widgetWithText(ListTile, 'Schnorr sign digest'),
      matching: find.byType(OutlinedButton),
    ));
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField), 'data');
    await tester.tap(find.widgetWithText(FilledButton, 'Sign'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiUtilUtilSha256Hex'), 1);
    expect(api.callCount('crateFfiSignerSignerSchnorrSign'), 1);
    expect(find.textContaining('schnorrsig1'), findsOneWidget);
  });

  testWidgets('sign event JSON dialog returns signed event', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stubString('crateFfiSignerSignerSignUnsigned', '{"id":"evt1"}');

    await pumpScreen(tester);

    await tester.tap(find.descendant(
      of: find.widgetWithText(ListTile, 'Sign event JSON'),
      matching: find.byType(OutlinedButton),
    ));
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField), '{"kind":1}');
    await tester.tap(find.widgetWithText(FilledButton, 'Sign'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiSignerSignerSignUnsigned'), 1);
    expect(find.textContaining('evt1'), findsOneWidget);
  });

  testWidgets('NIP-44 round-trip shows match', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stubString('crateFfiSignerSignerNip44Encrypt', 'ct1');
    api.stubString('crateFfiSignerSignerNip44Decrypt', 'secret msg');

    await pumpScreen(tester);

    await tester.tap(find.descendant(
      of: find.widgetWithText(ListTile, 'NIP-44 encrypt / decrypt'),
      matching: find.byType(OutlinedButton),
    ));
    await tester.pumpAndSettle();

    await tester.enterText(find.byType(TextField).first, 'secret msg');
    await tester.tap(find.widgetWithText(FilledButton, 'Encrypt + decrypt'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiSignerSignerNip44Encrypt'), 1);
    expect(api.callCount('crateFfiSignerSignerNip44Decrypt'), 1);
    expect(find.textContaining('ct1'), findsOneWidget);
    expect(find.textContaining('Round-trip match'), findsOneWidget);
  });

  testWidgets('lock app PIN: no PIN configured shows guidance', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);

    await pumpScreen(tester);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Lock now'));
    await tester.pump();

    expect(
      find.textContaining('No app lock PIN configured'),
      findsOneWidget,
    );
  });

  testWidgets('lock app PIN: configured PIN locks shell', (tester) async {
    api.stubString('crateFfiSignerSignerPubkey', 'pk123');
    api.stubBool('crateFfiSignerSignerIsLocked', false);
    api.stubBool('crateFfiPinPinSet', true);

    final shell = ShellService();
    await shell.setPin('1234');
    expect(shell.hasPin, true);

    await pumpScreen(tester, shell: shell);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Lock now'));
    await tester.pump();

    expect(shell.locked, true);
    expect(find.textContaining('App locked'), findsOneWidget);
  });
}