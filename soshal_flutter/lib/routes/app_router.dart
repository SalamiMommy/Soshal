import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../screens/splash_screen.dart';
import '../screens/auth_screen.dart';
import '../screens/feed_screen.dart';
import '../screens/profile_screen.dart';
import '../screens/inbox_screen.dart';
import '../screens/settings_screen.dart';
import '../screens/geohash_calculator_screen.dart';
import '../screens/notifications_screen.dart';
import '../screens/edit_profile_screen.dart';
import '../screens/accounts_screen.dart';
import '../screens/backup_screen.dart';
import '../screens/blocked_screen.dart';
import '../screens/moderation_screen.dart';
import '../screens/security_screen.dart';
import '../screens/notification_settings_screen.dart';
import '../screens/thread_screen.dart';
import '../screens/search_screen.dart';
import '../screens/dating_screen.dart';
import '../screens/dating_profile_screen.dart';
import '../screens/events_screen.dart';
import '../screens/groups_screen.dart';
import '../screens/marketplace_screen.dart';
import '../screens/live_screen.dart';
import '../screens/stories_screen.dart';
import '../screens/turso_settings_screen.dart';
import '../screens/minis_screen.dart';
import '../screens/minis_user_screen.dart';
import '../screens/music_screen.dart';
import '../screens/musicloud_user_screen.dart';
import '../screens/chat_random_screen.dart';
import '../screens/friends_screen.dart';
import '../screens/network_screen.dart';
import '../screens/call_screen.dart';
import '../screens/vouch_screen.dart';
import '../screens/stealth_screen.dart';
import '../screens/analytics_screen.dart';
import '../screens/audit_screen.dart';
import '../screens/bookmarks_screen.dart';
import '../screens/scheduled_screen.dart';
import '../screens/appearance_screen.dart';
import '../screens/language_screen.dart';
import '../screens/privacy_screen.dart';
import '../screens/storage_screen.dart';
import '../screens/advanced_screen.dart';
import '../screens/network_settings_screen.dart';
import '../screens/share_app_screen.dart';
import '../screens/live_broadcast_screen.dart';
import '../screens/moq_viewer_screen.dart';
import '../screens/profile_builder_screen.dart';
import '../screens/profile_renderer_screen.dart';
import '../services/music_service.dart';
import '../services/session_service.dart';
import '../widgets/app_shell.dart';

import '../services/signer_service.dart';

class AppRouter {
  static final GoRouter router = GoRouter(
    initialLocation: '/',
    redirect: (context, state) {
      final path = state.uri.path;
      if (path == '/' || path == '/auth') return null;
      final session = context.read<SessionService>();
      if (session.activePubkey == null) return '/';
      // Also redirect when the signer is locked: a session record exists
      // (activePubkey set) but the in-memory key is wiped. Signing operations
      // would fail at the Rust layer; redirect to splash so the user can
      // re-authenticate via keychain or recovery phrase.
      final signer = context.read<SignerService>();
      if (signer.locked) return '/';
      return null;
    },
    errorBuilder: (context, state) => Scaffold(
      body: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            const Icon(Icons.error_outline, size: 64, color: Colors.grey),
            const SizedBox(height: 16),
            Text('Page not found: ${state.uri.path}'),
            const SizedBox(height: 16),
            ElevatedButton(
              onPressed: () => context.go('/feed'),
              child: const Text('Back to Feed'),
            ),
          ],
        ),
      ),
    ),
    routes: [
      GoRoute(
        path: '/',
        builder: (context, state) => const SplashScreen(),
      ),
      GoRoute(
        path: '/auth',
        builder: (context, state) => const AuthScreen(),
      ),
      ShellRoute(
        builder: (context, state, child) => AppShell(child: child),
        routes: [
          GoRoute(
            path: '/feed',
            builder: (context, state) => const FeedScreen(),
          ),
          GoRoute(
            path: '/profile/:pubkey',
            builder: (context, state) => ProfileScreen(
              pubkey: state.pathParameters['pubkey'],
            ),
          ),
          GoRoute(
            path: '/profile',
            builder: (context, state) => const ProfileScreen(),
          ),
          GoRoute(
            path: '/profile-builder',
            builder: (context, state) => const ProfileBuilderScreen(),
          ),
          GoRoute(
            path: '/profile-renderer/:pubkey',
            builder: (context, state) => ProfileRendererScreen(
              pubkey: state.pathParameters['pubkey'] ?? '',
              initialEditMode: state.uri.queryParameters['editMode'] == 'true',
            ),
          ),
          GoRoute(
            path: '/inbox/:pubkey',
            builder: (context, state) => InboxScreen(
              otherPubkey: state.pathParameters['pubkey'],
            ),
          ),
          GoRoute(
            path: '/inbox',
            builder: (context, state) => const InboxScreen(),
          ),
          GoRoute(
            path: '/search',
            builder: (context, state) => const SearchScreen(),
          ),
          GoRoute(
            path: '/dating',
            builder: (context, state) => const DatingScreen(),
          ),
          GoRoute(
            path: '/dating/me',
            builder: (context, state) => const DatingProfileScreen(),
          ),
          GoRoute(
            path: '/events',
            builder: (context, state) => const EventsScreen(),
          ),
          GoRoute(
            path: '/events/discovery',
            builder: (context, state) => const EventsAudienceDiscoveryScreen(),
          ),
          GoRoute(
            path: '/events/:eventId',
            builder: (context, state) => EventDetailScreen(
              eventId: state.pathParameters['eventId'] ?? '',
            ),
          ),
          GoRoute(
            path: '/groups',
            builder: (context, state) => const GroupsScreen(),
          ),
          GoRoute(
            path: '/groups/:groupId',
            builder: (context, state) => GroupDetailScreen(
              groupId: state.pathParameters['groupId'] ?? '',
            ),
          ),
          GoRoute(
            path: '/marketplace',
            builder: (context, state) => const MarketplaceScreen(),
          ),
          GoRoute(
            path: '/live',
            builder: (context, state) => const LiveScreen(),
          ),
          GoRoute(
            path: '/live/broadcast/:streamId',
            builder: (context, state) => LiveBroadcastScreen(
              streamId: state.pathParameters['streamId'] ?? '',
              title: state.uri.queryParameters['title'] ?? '',
            ),
          ),
          GoRoute(
            path: '/live/viewer/:streamId',
            builder: (context, state) => MoqViewerScreen(
              addr: state.uri.queryParameters['addr'] ?? '',
              streamId: state.pathParameters['streamId'] ?? '',
            ),
          ),
          GoRoute(
            path: '/stories',
            builder: (context, state) => const StoriesScreen(),
          ),
          GoRoute(
            path: '/notifications',
            builder: (context, state) => const NotificationsScreen(),
          ),
          GoRoute(
            path: '/minis',
            builder: (context, state) => const MinisScreen(),
          ),
          GoRoute(
            path: '/minis/:pubkey',
            builder: (context, state) => MinisUserScreen(
              pubkey: state.pathParameters['pubkey'] ?? '',
            ),
          ),
          GoRoute(
            path: '/music',
            builder: (context, state) => const MusicloudScreen(),
          ),
          GoRoute(
            path: '/music/track',
            builder: (context, state) {
              final track = state.extra as MusicTrack?;
              if (track == null) {
                WidgetsBinding.instance.addPostFrameCallback((_) {
                  if (context.mounted) context.go('/music');
                });
                return const SizedBox.shrink();
              }
              return TrackDetailScreen(track);
            },
          ),
          GoRoute(
            path: '/music/:pubkey',
            builder: (context, state) => MusicloudUserScreen(
              pubkey: state.pathParameters['pubkey'] ?? '',
            ),
          ),
          GoRoute(
            path: '/chat-random',
            builder: (context, state) => const ChatRandomScreen(),
          ),
          GoRoute(
            path: '/friends',
            builder: (context, state) => const FriendsScreen(),
          ),
          GoRoute(
            path: '/network',
            builder: (context, state) => const NetworkScreen(),
          ),
          GoRoute(
            path: '/vouch',
            builder: (context, state) => const VouchScreen(),
          ),
          GoRoute(
            path: '/stealth',
            builder: (context, state) => const StealthScreen(),
          ),
          GoRoute(
            path: '/analytics',
            builder: (context, state) => const AnalyticsScreen(),
          ),
          GoRoute(
            path: '/audit',
            builder: (context, state) => const AuditScreen(),
          ),
          GoRoute(
            path: '/bookmarks',
            builder: (context, state) => const BookmarksScreen(),
          ),
          GoRoute(
            path: '/scheduled',
            builder: (context, state) => const ScheduledScreen(),
          ),
          GoRoute(
            path: '/call/:peer/:mediaType/:callId',
            builder: (context, state) => CallScreen(
              peer: state.pathParameters['peer'] ?? '',
              mediaType: state.pathParameters['mediaType'] ?? 'voice',
              callId: state.pathParameters['callId'] ?? '',
            ),
          ),
          GoRoute(
            path: '/settings',
            builder: (context, state) => const SettingsScreen(),
          ),
          GoRoute(
            path: '/settings/share',
            builder: (context, state) => const ShareAppScreen(),
          ),
          GoRoute(
            path: '/settings/edit-profile',
            builder: (context, state) => const EditProfileScreen(),
          ),
          GoRoute(
            path: '/settings/accounts',
            builder: (context, state) => const AccountsScreen(),
          ),
          GoRoute(
            path: '/settings/backup',
            builder: (context, state) => const BackupScreen(),
          ),
          GoRoute(
            path: '/settings/turso',
            builder: (context, state) => const TursoSettingsScreen(),
          ),
          GoRoute(
            path: '/settings/blocked',
            builder: (context, state) => const BlockedScreen(),
          ),
          GoRoute(
            path: '/settings/moderation',
            builder: (context, state) => const ModerationScreen(),
          ),
          GoRoute(
            path: '/settings/security',
            builder: (context, state) => const SecurityScreen(),
          ),
          GoRoute(
            path: '/settings/notifications',
            builder: (context, state) => const NotificationSettingsScreen(),
          ),
          GoRoute(
            path: '/settings/appearance',
            builder: (context, state) => const AppearanceScreen(),
          ),
          GoRoute(
            path: '/settings/language',
            builder: (context, state) => const LanguageScreen(),
          ),
          GoRoute(
            path: '/settings/privacy',
            builder: (context, state) => const PrivacyScreen(),
          ),
          GoRoute(
            path: '/settings/privacy/stealth',
            builder: (context, state) => const StealthEditorScreen(),
          ),
          GoRoute(
            path: '/settings/storage',
            builder: (context, state) => const StorageScreen(),
          ),
          GoRoute(
            path: '/settings/advanced',
            builder: (context, state) => const AdvancedScreen(),
          ),
          GoRoute(
            path: '/settings/network',
            builder: (context, state) => const NetworkSettingsScreen(),
          ),
          GoRoute(
            path: '/settings/geohash',
            builder: (context, state) => const GeohashCalculatorScreen(),
          ),
          GoRoute(
            path: '/post/:eventId',
            builder: (context, state) => ThreadScreen(
              eventId: state.pathParameters['eventId'] ?? '',
            ),
          ),
        ],
      ),
    ],
  );
}
