import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/screens/call_screen.dart';
import 'package:soshal_flutter/services/calls_service.dart';
import 'package:soshal_flutter/services/session_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

class FakeCallsService extends CallsService {
  @override
  void startCall({
    required String callId,
    required String peer,
    required String mediaType,
  }) {}

  @override
  void endCall() {}

  @override
  Future<List<CallSignal>> fetchSignals(String myPubkey) async => [];
}

void main() {
  final env = bootstrapTestEnv('test-call');
  api = env.$1;

  setUp(() {
    api.handlers.clear();
    api.calls.clear();
    api.stubString('crateFfiWebrtcWebrtcGetIceConfig', '{}');
    api.stubString('crateFfiWebrtcWebrtcCreatePeerConfig', '{}');
    api.stubListString('crateFfiWebrtcWebrtcGetStunServers', []);
    api.stubString('crateFfiWebrtcWebrtcGetTurnServers', '{}');
  });

  Future<void> pump(WidgetTester tester, {String mediaType = 'voice'}) async {
    tester.view.physicalSize = const Size(900, 2600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    await tester.pumpWidget(MultiProvider(
      providers: [
        ChangeNotifierProvider<SessionService>.value(value: session),
        ChangeNotifierProvider<CallsService>.value(value: FakeCallsService()),
      ],
      child: MaterialApp(
        home: CallScreen(
          peer: 'pk-peer',
          mediaType: mediaType,
          callId: 'call-1',
        ),
      ),
    ));
    await tester.pump(const Duration(milliseconds: 100));
  }

  testWidgets('voice call screen without account shows static frame',
      (tester) async {
    await pump(tester);

    expect(find.text('Voice call'), findsWidgets);
    expect(find.text('Signals: 0'), findsOneWidget);
    expect(find.byIcon(Icons.call_end), findsOneWidget);
    expect(api.callCount('crateFfiCallsCallsFetchSignals'), 0);
    await tester.pump(const Duration(seconds: 1));
  });

  testWidgets('video call screen shows video call title', (tester) async {
    await pump(tester, mediaType: 'video');

    expect(find.text('Video call'), findsWidgets);
    await tester.pump(const Duration(seconds: 1));
  });

  testWidgets('call screen renders setup panel when signed in', (tester) async {
    api.stubString(
      'crateFfiSessionSessionLoad',
      '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123",'
      '"npub":"npub1abc","last_used":0,"relay_list":[]}]}',
    );
    api.stubString('crateFfiCallsCallsFetchSignals', '[]');

    tester.view.physicalSize = const Size(900, 2600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    await session.loadSession();
    await tester.pumpWidget(MultiProvider(
      providers: [
        ChangeNotifierProvider<SessionService>.value(value: session),
        ChangeNotifierProvider<CallsService>.value(value: FakeCallsService()),
      ],
      child: const MaterialApp(
        home: CallScreen(
          peer: 'pk-peer',
          mediaType: 'voice',
          callId: 'call-2',
        ),
      ),
    ));
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pump(const Duration(milliseconds: 100));

    expect(find.text('Call setup'), findsOneWidget);
    expect(find.text('Send offer signal'), findsOneWidget);
    await tester.pump(const Duration(seconds: 1));
  });
}