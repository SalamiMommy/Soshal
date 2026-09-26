import 'package:flutter/widgets.dart';
import 'package:provider/provider.dart';

import '../services/bookmarks_service.dart';
import '../services/calls_service.dart';
import '../services/chatrandom_service.dart';
import '../services/dating_service.dart';
import '../services/events_service.dart';
import '../services/feed_service.dart';
import '../services/friends_service.dart';
import '../services/groups_service.dart';
import '../services/marketplace_service.dart';
import '../services/messaging_service.dart';
import '../services/minis_service.dart';
import '../services/moderation_service.dart';
import '../services/music_service.dart';
import '../services/notifications_service.dart';
import '../services/scheduled_service.dart';
import '../services/search_service.dart';
import '../services/stealth_service.dart';
import '../services/streaming_service.dart';
import '../services/zap_service.dart';

/// AccountScope wraps the authenticated widget subtree in a scoped dependency
/// container tied to the active account pubkey.
///
/// When the user logs out or switches accounts, the Key `ValueKey(pubkey)`
/// changes, causing Flutter to automatically unmount and dispose all previous
/// domain services (clearing their in-memory caches, subscriptions, and state).
/// A fresh set of services is then mounted for the new account, completely
/// eliminating manual reset cascades and preventing cross-account state leaks.
class AccountScope extends StatelessWidget {
  /// The active account's pubkey. Changing this key forces a complete
  /// disposal and recreation of the scoped services.
  final String pubkey;
  final Widget child;

  const AccountScope({
    super.key,
    required this.pubkey,
    required this.child,
  });

  @override
  Widget build(BuildContext context) {
    return KeyedSubtree(
      key: ValueKey('account_scope_$pubkey'),
      child: MultiProvider(
        providers: [
          ChangeNotifierProvider(create: (_) => FeedService()),
          ChangeNotifierProvider(create: (_) => MessagingService()),
          ChangeNotifierProvider(create: (_) => NotificationService()),
          ChangeNotifierProvider(create: (_) => SearchService()),
          ChangeNotifierProvider(create: (_) => DatingService()),
          ChangeNotifierProvider(create: (_) => EventsService()),
          ChangeNotifierProvider(create: (_) => GroupsService()),
          ChangeNotifierProvider(create: (_) => MarketplaceService()),
          ChangeNotifierProvider(create: (_) => BookmarksService()),
          ChangeNotifierProvider(create: (_) => ModerationService()),
          ChangeNotifierProvider(create: (_) => CallsService()),
          ChangeNotifierProvider(create: (_) => FriendsService()),
          ChangeNotifierProvider(create: (_) => MinisService()),
          ChangeNotifierProvider(create: (_) => MusicService()),
          ChangeNotifierProvider(create: (_) => ZapService()),
          ChangeNotifierProvider(create: (_) => StreamingService()),
          ChangeNotifierProvider(create: (_) => ScheduledService()),
          ChangeNotifierProvider(create: (_) => StealthService()),
          ChangeNotifierProvider(create: (_) => ChatrandomService()),
        ],
        child: child,
      ),
    );
  }
}
