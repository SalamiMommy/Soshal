import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/shell_service.dart';
import 'package:soshal_flutter/services/signer_service.dart';
import 'package:soshal_flutter/widgets/signer_lock_screen.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('signer-lock-test');
  api = env.$1;

  testWidgets('SignerLockScreen displays Unlock Profile and no recovery phrase field',
      (tester) async {
    api.handlers.clear();
    const sessionJson = '''{
      "active_pubkey": "testpubkey123",
      "accounts": [
        {"pubkey": "testpubkey123", "npub": "npub123", "last_used": 1000, "relay_list": []}
      ]
    }''';
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);
    api.stubBool('crateFfiSignerSignerUnlockFromKeyring', true);

    final session = SessionService();
    await session.loadSession();

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider.value(value: session),
          ChangeNotifierProvider(create: (_) => SignerService()),
          ChangeNotifierProvider(create: (_) => ShellService()),
        ],
        child: const MaterialApp(
          home: SignerLockScreen(),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Profile locked'), findsOneWidget);
    expect(find.text('Unlock Profile'), findsOneWidget);
    expect(find.text('Recovery phrase'), findsNothing);
    expect(find.byType(TextField), findsNothing);

    await tester.tap(find.text('Unlock Profile'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiSignerSignerUnlockFromKeyring'), 1);
  });

  testWidgets('SignerLockScreen shows Unlock Profile even when shell has PIN and supports key dialog',
      (tester) async {
    api.handlers.clear();
    const sessionJson = '''{
      "active_pubkey": "testpubkey123",
      "accounts": [
        {"pubkey": "testpubkey123", "npub": "npub123", "last_used": 1000, "relay_list": []}
      ]
    }''';
    api.stubString('crateFfiSessionSessionLoad', sessionJson);
    api.stubString('crateFfiFfiBridgeGetDbPath', env.$2);
    api.stubString('crateFfiSignerSignerUnlock', 'testpubkey123');
    api.stubBool('crateFfiPinPinSet', true);

    final session = SessionService();
    await session.loadSession();
    final shell = ShellService();
    await shell.setPin('1234');

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider.value(value: session),
          ChangeNotifierProvider(create: (_) => SignerService()),
          ChangeNotifierProvider.value(value: shell),
        ],
        child: const MaterialApp(
          home: SignerLockScreen(),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Unlock Profile'), findsOneWidget);
    expect(find.text('Unlock with key or phrase'), findsOneWidget);

    await tester.tap(find.text('Unlock with key or phrase'));
    await tester.pumpAndSettle();

    expect(find.text('Unlock with Key or Phrase'), findsOneWidget);
    expect(find.byType(TextField), findsOneWidget);

    await tester.enterText(find.byType(TextField), 'nsec12345');
    await tester.tap(find.text('Unlock'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiSignerSignerUnlock'), 1);
  });
}
