import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/backup_screen.dart';
import 'package:soshal_flutter/services/backup_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-backup');
  api = env.$1;
  final root = env.$2;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
    api.stubString('crateFfiDbDbPath', '$root/soshal.db');
  });

  const sessionJson =
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,'
      '"relay_list":["wss://relay.example.com"]}]}';

  void stubCounts() {
    api.stub('crateFfiDbDbCount', (inv) {
      final t = api.namedArg(inv, 'table');
      if (t == 'posts') return 3;
      if (t == 'messages') return 1;
      return 0;
    });
  }

  Future<void> pumpScreen(WidgetTester tester, {bool signedIn = true}) async {
    tester.view.physicalSize = const Size(800, 1600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    if (signedIn) {
      api.stubString('crateFfiSessionSessionLoad', sessionJson);
      await session.loadSession();
    }
    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: session),
          ChangeNotifierProvider(create: (_) => BackupService()),
          ChangeNotifierProvider(create: (_) => SettingsService()),
        ],
        child: const MaterialApp(home: BackupScreen()),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('renders account info, db path and local counts', (tester) async {
    stubCounts();

    await pumpScreen(tester);

    expect(find.text('Public key (hex)'), findsOneWidget);
    expect(find.text('pk123'), findsOneWidget);
    expect(find.text('NIP-19 npub'), findsOneWidget);
    expect(find.text('npub1abc'), findsOneWidget);
    expect(find.text('Relays'), findsOneWidget);
    expect(find.text('wss://relay.example.com'), findsOneWidget);
    expect(find.text('Database path'), findsOneWidget);
    expect(find.text('$root/soshal.db'), findsOneWidget);
    expect(find.text('Stored locally'), findsOneWidget);
    expect(find.text('posts: 3'), findsOneWidget);
    expect(find.text('messages: 1'), findsOneWidget);
  });

  testWidgets('shows sign-in gate without an account', (tester) async {
    await pumpScreen(tester, signedIn: false);

    expect(find.text('Sign in to view backup info'), findsOneWidget);
    expect(find.text('Export backup'), findsNothing);
    expect(find.text('Restore'), findsNothing);
  });

  testWidgets('export backup writes to backup path and reports result',
      (tester) async {
    stubCounts();
    api.stubString('crateFfiDbDbBackup', 'exported-ok');

    await pumpScreen(tester);

    await tester.tap(find.text('Export backup'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiDbDbBackup'), 1);
    final inv = api.callsOf('crateFfiDbDbBackup').single;
    expect(api.namedArg(inv, 'backupPath'), '$root/soshal_backup.db');
    expect(find.textContaining('Backup written'), findsOneWidget);
  });

  testWidgets('export failure shows error snackbar, no crash', (tester) async {
    stubCounts();
    api.stub('crateFfiDbDbBackup', (_) {
      throw Exception('disk full');
    });

    await pumpScreen(tester);

    await tester.tap(find.text('Export backup'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Export failed'), findsOneWidget);
  });

  testWidgets('restore restores from backup path and reports success',
      (tester) async {
    stubCounts();
    api.stubString('crateFfiDbDbRestore', 'restored');

    await pumpScreen(tester);

    await tester.tap(find.text('Restore'));
    await tester.pumpAndSettle();

    expect(api.callCount('crateFfiDbDbRestore'), 1);
    final inv = api.callsOf('crateFfiDbDbRestore').single;
    expect(api.namedArg(inv, 'backupPath'), '$root/soshal_backup.db');
    expect(find.textContaining('Restored from backup'), findsOneWidget);
  });

  testWidgets('restore failure shows error snackbar, no crash', (tester) async {
    stubCounts();
    api.stub('crateFfiDbDbRestore', (_) {
      throw Exception('corrupt backup');
    });

    await pumpScreen(tester);

    await tester.tap(find.text('Restore'));
    await tester.pumpAndSettle();

    expect(find.textContaining('Restore failed'), findsOneWidget);
  });
}