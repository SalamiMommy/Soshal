import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/advanced_screen.dart';
import 'package:soshal_flutter/screens/analytics_screen.dart';
import 'package:soshal_flutter/screens/appearance_screen.dart';
import 'package:soshal_flutter/screens/audit_screen.dart';
import 'package:soshal_flutter/services/analytics_service.dart';
import 'package:soshal_flutter/services/audit_service.dart';
import 'package:soshal_flutter/services/crypto_service.dart';
import 'package:soshal_flutter/services/network_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';
import 'package:soshal_flutter/services/shell_service.dart';
import 'package:soshal_flutter/services/sync_service.dart';
import 'package:soshal_flutter/services/telemetry_service.dart';
import 'package:soshal_flutter/services/theme_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

class FakeAuditService extends AuditService {
  @override
  Future<List<AuditRow>> list({int limit = 100, String? actor}) async {
    await Future<void>.delayed(Duration.zero);
    notifyListeners();
    return [];
  }
}

void main() {
  final env = bootstrapTestEnv('test-advanced');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  Future<void> pump(WidgetTester tester, Widget child,
      {List<ChangeNotifierProvider> extra = const []}) async {
    tester.view.physicalSize = const Size(900, 2600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    await tester.pumpWidget(MultiProvider(
      providers: [
        ...extra,
        ChangeNotifierProvider(create: (_) => NetworkService()),
        ChangeNotifierProvider(create: (_) => SettingsService()),
        ChangeNotifierProvider(create: (_) => SyncService()),
        ChangeNotifierProvider(create: (_) => TelemetryService()),
        ChangeNotifierProvider(create: (_) => ThemeService()),
        ChangeNotifierProvider(create: (_) => ShellService()),
        ChangeNotifierProvider(create: (_) => AnalyticsService()),
        ChangeNotifierProvider(create: (_) => CryptoService()),
        ChangeNotifierProvider<AuditService>.value(value: FakeAuditService()),
      ],
      child: MaterialApp(home: child),
    ));
  }

  testWidgets('advanced screen renders sections and refresh runs', (tester) async {
    api.stubString('crateFfiNetworkNetworkGetSysDiagnostics', '{}');
    api.stubBool('crateFfiNetworkNetworkI2PStatus', true);
    api.stubBool('crateFfiNetworkNetworkFreenetStatus', false);
    api.stubString('crateFfiDbDbGetSetting', '');
    api.stubBool('crateFfiSyncSyncRunning', false);
    api.stubString('crateFfiSyncSyncGetOutboxSummary', '{}');
    api.stubString('crateFfiTelemetryTelemetryInfoJson', '{}');
    api.stubString('crateFfiTelemetryTelemetryReadAllJson', '[]');

    await pump(tester, const AdvancedScreen());
    await tester.pumpAndSettle();

    expect(find.text('Advanced'), findsOneWidget);
    expect(find.text('Transports'), findsOneWidget);
    expect(find.text('Sync engine'), findsOneWidget);
    expect(find.text('Storage & DB'), findsOneWidget);
    expect(find.text('Crypto'), findsOneWidget);
    expect(find.text('Telemetry'), findsOneWidget);
    await tester.scrollUntilVisible(
      find.text('Diagnostics'),
      300,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Diagnostics'), findsOneWidget);
  });

  testWidgets('advanced sync pass taps reach sync service', (tester) async {
    api.stubString('crateFfiNetworkNetworkGetSysDiagnostics', '{}');
    api.stubBool('crateFfiNetworkNetworkI2PStatus', false);
    api.stubBool('crateFfiNetworkNetworkFreenetStatus', false);
    api.stubString('crateFfiDbDbGetSetting', '');
    api.stubBool('crateFfiSyncSyncRunning', false);
    api.stubString('crateFfiSyncSyncGetOutboxSummary', '{}');
    api.stubString('crateFfiTelemetryTelemetryInfoJson', '{}');
    api.stubString('crateFfiTelemetryTelemetryReadAllJson', '[]');
    api.stubInt('crateFfiHeadlessBackgroundSyncTask', 3);

    await pump(tester, const AdvancedScreen());
    await tester.pumpAndSettle();

    await tester.scrollUntilVisible(
      find.text('Run sync pass'),
      300,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.tap(find.text('Run sync pass'));
    await tester.pumpAndSettle();
    expect(api.callCount('crateFfiHeadlessBackgroundSyncTask'), 1);
    expect(find.text('Sync pass done (3 events)'), findsOneWidget);
  });

  testWidgets('analytics screen renders panels', (tester) async {
    await pump(tester, const AnalyticsScreen());
    await tester.pumpAndSettle();

    expect(find.text('Analytics'), findsOneWidget);
    expect(find.text('Post Analytics'), findsOneWidget);
    expect(find.text('Run Analytics'), findsOneWidget);
    expect(find.text('Classify Post'), findsOneWidget);
    expect(find.text('Generate Embedding'), findsOneWidget);
  });

  testWidgets('analytics run triggers compute', (tester) async {
    api.stubString('crateFfiAnalyticsAnalyticsComputeStats', '{}');
    await pump(tester, const AnalyticsScreen());
    await tester.pumpAndSettle();

    await tester.tap(find.text('Run Analytics'));
    await tester.pumpAndSettle();
    expect(api.callCount('crateFfiAnalyticsAnalyticsComputeStats'), 1);
  });

  testWidgets('appearance screen renders theme editor', (tester) async {
    api.stubString('crateFfiDbDbGetSetting', '');
    await pump(tester, const AppearanceScreen());
    await tester.pumpAndSettle();

    expect(find.text('Appearance'), findsOneWidget);
    expect(find.text('Theme'), findsOneWidget);
    expect(find.text('Accent color'), findsOneWidget);
    expect(find.text('Font size scale'), findsOneWidget);
    expect(find.text('Font family'), findsOneWidget);
    expect(find.text('Save Theme'), findsOneWidget);
    expect(find.text('Reset to default'), findsNWidgets(2));
  });

  testWidgets('audit screen empty state', (tester) async {
    api.stubString('crateFfiAuditAuditList', '[]');
    await pump(tester, const AuditScreen());
    await tester.pumpAndSettle();

    expect(find.text('Audit Log'), findsOneWidget);
    expect(find.text('No audit entries yet'), findsOneWidget);
    expect(
      find.text('Security events will appear here as they happen.'),
      findsOneWidget,
    );
  });
}
