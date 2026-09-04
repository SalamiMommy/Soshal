import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/services/dating_service.dart';
import 'package:soshal_flutter/services/events_service.dart';
import 'package:soshal_flutter/services/friends_service.dart';
import 'package:soshal_flutter/services/media_service.dart';
import 'package:soshal_flutter/services/session_service.dart';
import 'package:soshal_flutter/screens/events_screen.dart';

import '../helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-events-debug');
  final api = env.$1;

  testWidgets('dump rects', (tester) async {
    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);

    final session = SessionService();
    api.stubString('crateFfiSessionSessionLoad',
        '{"active_pubkey":"pk123","accounts":[{"pubkey":"pk123","npub":"npub1abc","last_used":0,"relay_list":[]}]}');
    await session.loadSession();
    api.stubString('crateFfiEventsEventsFetchNearby', '[]');
    api.stubString('crateFfiDatingDatingGetOwnProfile',
        '{"pubkey":"pk123","name":"Me","age":30,"location":"NYC","bio":"","images":[],"interests":["hiking"],"compatibility_score":0,"last_seen":0}');
    api.stub('crateFfiUtilUtilExtractHashtags', (_) => <String>[]);
    api.stubString('crateFfiEventsEventsScoreEvents', '{}');

    await tester.pumpWidget(
      MultiProvider(
        providers: [
          ChangeNotifierProvider<SessionService>.value(value: session),
          ChangeNotifierProvider(create: (_) => EventsService()),
          ChangeNotifierProvider(create: (_) => DatingService()),
          ChangeNotifierProvider(create: (_) => FriendsService()),
          ChangeNotifierProvider(create: (_) => MediaService()),
        ],
        child: const MaterialApp(home: EventsScreen()),
      ),
    );
    await tester.pumpAndSettle();

    debugPrint('SCREEN: ${tester.getRect(find.byType(EventsScreen))}');
    debugPrint('SLIDER: ${tester.getRect(find.byType(Slider))}');
    debugPrint('LIST: ${tester.getRect(find.text('List'))}');
    debugPrint('CAL: ${tester.getRect(find.text('Calendar'))}');
    debugPrint('BODY: ${tester.getRect(find.byType(SegmentedButton<String>))}');
  });
}
