import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/network_screen.dart';
import 'package:soshal_flutter/screens/network_settings_screen.dart';
import 'package:soshal_flutter/screens/notification_settings_screen.dart';
import 'package:soshal_flutter/screens/privacy_screen.dart';
import 'package:soshal_flutter/screens/profile_builder_screen.dart';
import 'package:soshal_flutter/screens/profile_renderer_screen.dart';
import 'package:soshal_flutter/services/ebpf_service.dart';
import 'package:soshal_flutter/services/layout_service.dart';
import 'package:soshal_flutter/services/messaging_service.dart';
import 'package:soshal_flutter/services/mesh_service.dart';
import 'package:soshal_flutter/services/network_service.dart';
import 'package:soshal_flutter/services/notifications_service.dart';
import 'package:soshal_flutter/services/p2p_service.dart';
import 'package:soshal_flutter/services/profile_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/services/settings_service.dart';
import 'package:soshal_flutter/services/shell_service.dart';
import 'package:soshal_flutter/services/stealth_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

class FakeNetworkService extends NetworkService {
  @override
  Future<void> loadTransportMode() async {}

  @override
  Future<void> refresh() async {}

  @override
  Future<List<RelayInfo>> fetchRelayStatus() async => [];
}

void main() {
  final env = bootstrapTestEnv('test-network-settings-screens');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  Future<void> pump(WidgetTester tester, Widget child) async {
    tester.view.physicalSize = const Size(900, 2600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(MultiProvider(
      providers: [
        ChangeNotifierProvider(create: (_) => SessionService()),
        ChangeNotifierProvider<NetworkService>.value(
          value: FakeNetworkService(),
        ),
        ChangeNotifierProvider(create: (_) => MeshService()),
        ChangeNotifierProvider(create: (_) => P2pService()),
        ChangeNotifierProvider(create: (_) => EbpfService()),
        ChangeNotifierProvider(create: (_) => LayoutService()),
        ChangeNotifierProvider(create: (_) => SettingsService()),
        ChangeNotifierProvider(create: (_) => ShellService()),
        ChangeNotifierProvider(create: (_) => StealthService()),
        ChangeNotifierProvider(create: (_) => NotificationService()),
        ChangeNotifierProvider(create: (_) => IdentityService()),
        ChangeNotifierProvider(create: (_) => ProfileService()),
      ],
      child: MaterialApp(home: child),
    ));
    await tester.pumpAndSettle();
  }

  testWidgets('network screen renders relay tab', (tester) async {
    api.stubString('crateFfiNetworkNetworkGetRelayStatus', '{}');
    await pump(tester, const NetworkScreen());

    expect(find.text('Network'), findsOneWidget);
    expect(find.text('Relays'), findsOneWidget);
    expect(find.text('Transports'), findsOneWidget);
    expect(find.text('Relay Connections'), findsOneWidget);
    expect(find.text('Reconnect from account relays'), findsOneWidget);
  });

  testWidgets('network settings screen renders transport tiles',
      (tester) async {
    api.stubString('crateFfiDbDbGetSetting', '');
    api.stubString('crateFfiNetworkNetworkSetTransportMode', 'ok');
    api.stubBool('crateFfiNetworkNetworkI2PStatus', false);
    api.stubBool('crateFfiNetworkNetworkFreenetStatus', false);
    api.stubString('crateFfiNetworkNetworkGetRelayStatus', '{}');
    await pump(tester, const NetworkSettingsScreen());

    expect(find.text('Network'), findsOneWidget);
    expect(find.text('Transports'), findsOneWidget);
    expect(find.text('I2P tunnel (local i2pd SOCKS 7656)'), findsOneWidget);
    expect(find.text('Freenet gateway (local port 8888)'), findsOneWidget);
    expect(find.text('Bundled Daemons'), findsOneWidget);
    expect(find.text('Relays'), findsWidgets);
  });

  testWidgets('notification settings screen renders panels', (tester) async {
    api.stubString('crateFfiSessionSessionGetActive', '{}');
    await pump(tester, const NotificationSettingsScreen());

    expect(find.text('Notifications'), findsOneWidget);
    expect(find.text('In-app notifications'), findsOneWidget);
    expect(find.text('Push notifications'), findsOneWidget);
    expect(find.text('About'), findsOneWidget);
  });

  testWidgets('privacy screen renders levels', (tester) async {
    api.stubString('crateFfiDbDbGetSetting', '');
    await pump(tester, const PrivacyScreen());

    expect(find.text('Privacy'), findsOneWidget);
    expect(find.text('Privacy level'), findsOneWidget);
    expect(find.text('Stealth whitelist'), findsOneWidget);
    expect(find.text('Lock app'), findsOneWidget);
  });

  testWidgets('profile builder screen empty state', (tester) async {
    await pump(tester, const ProfileBuilderScreen());

    expect(find.text('Edit Profile'), findsOneWidget);
    expect(find.text('Save'), findsOneWidget);
    expect(find.text('Your profile is empty.'), findsOneWidget);
    expect(find.text('Add widgets to customize it!'), findsOneWidget);
  });

  testWidgets('profile renderer screen renders default post history',
      (tester) async {
    await pump(tester, const ProfileRendererScreen(pubkey: 'pk-abc'));

    expect(find.text('Profile'), findsOneWidget);
    expect(find.text('Post History'), findsOneWidget);
    expect(find.text('Recent posts will appear here'), findsOneWidget);
  });
}