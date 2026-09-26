import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/widgets/shimmer_loading.dart';

void main() {
  group('ShimmerLoading Widgets', () {
    testWidgets('ShimmerPostCard renders without errors', (tester) async {
      await tester.pumpWidget(
        const MaterialApp(
          home: Scaffold(
            body: ShimmerPostCard(),
          ),
        ),
      );

      expect(find.byType(ShimmerPostCard), findsOneWidget);
      expect(find.byType(Card), findsOneWidget);
    });

    testWidgets('ShimmerStoryCircle renders without errors', (tester) async {
      await tester.pumpWidget(
        const MaterialApp(
          home: Scaffold(
            body: ShimmerStoryCircle(),
          ),
        ),
      );

      expect(find.byType(ShimmerStoryCircle), findsOneWidget);
    });

    testWidgets('ShimmerNotificationTile renders without errors', (tester) async {
      await tester.pumpWidget(
        const MaterialApp(
          home: Scaffold(
            body: ShimmerNotificationTile(),
          ),
        ),
      );

      expect(find.byType(ShimmerNotificationTile), findsOneWidget);
    });
  });
}
