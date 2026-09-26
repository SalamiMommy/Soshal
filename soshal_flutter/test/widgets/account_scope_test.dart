import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/services/feed_service.dart';
import 'package:soshal_flutter/services/messaging_service.dart';
import 'package:soshal_flutter/services/notifications_service.dart';
import 'package:soshal_flutter/widgets/account_scope.dart';

void main() {
  group('AccountScope widget tests', () {
    testWidgets('provides account-scoped services to descendants',
        (tester) async {
      FeedService? capturedFeed;
      MessagingService? capturedMessaging;
      NotificationService? capturedNotifications;

      await tester.pumpWidget(
        MaterialApp(
          home: AccountScope(
            pubkey: 'alice_123',
            child: Builder(
              builder: (context) {
                capturedFeed = context.read<FeedService>();
                capturedMessaging = context.read<MessagingService>();
                capturedNotifications = context.read<NotificationService>();
                return const Scaffold(
                  body: Text('AccountScope Content'),
                );
              },
            ),
          ),
        ),
      );

      expect(find.text('AccountScope Content'), findsOneWidget);
      expect(capturedFeed, isNotNull);
      expect(capturedMessaging, isNotNull);
      expect(capturedNotifications, isNotNull);
    });

    testWidgets('switching pubkey recreates fresh scoped services',
        (tester) async {
      FeedService? feedInstance1;
      FeedService? feedInstance2;

      final pubkeyNotifier = ValueNotifier<String>('alice_123');

      await tester.pumpWidget(
        MaterialApp(
          home: ValueListenableBuilder<String>(
            valueListenable: pubkeyNotifier,
            builder: (context, pubkey, _) {
              return AccountScope(
                pubkey: pubkey,
                child: Builder(
                  builder: (context) {
                    final feed = context.read<FeedService>();
                    if (pubkey == 'alice_123') {
                      feedInstance1 = feed;
                    } else {
                      feedInstance2 = feed;
                    }
                    return Text('Current: $pubkey');
                  },
                ),
              );
            },
          ),
        ),
      );

      expect(find.text('Current: alice_123'), findsOneWidget);
      expect(feedInstance1, isNotNull);

      // Switch account to Bob
      pubkeyNotifier.value = 'bob_456';
      await tester.pumpAndSettle();

      expect(find.text('Current: bob_456'), findsOneWidget);
      expect(feedInstance2, isNotNull);
      // The new instance must NOT be the same as the previous instance
      expect(identical(feedInstance1, feedInstance2), isFalse);
    });
  });
}
