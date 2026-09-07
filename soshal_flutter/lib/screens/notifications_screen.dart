import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/notifications_service.dart';
import '../services/session_service.dart';
import '../utils/safe_url.dart';
import '../widgets/empty_state.dart';

/// Notifications screen: all/unread/mentions/reactions/replies/follows.
class NotificationsScreen extends StatefulWidget {
  /// Notifications screen
  const NotificationsScreen({super.key});

  @override
  State<NotificationsScreen> createState() => _NotificationsScreenState();
}

class _NotificationsScreenState extends State<NotificationsScreen>
    with SingleTickerProviderStateMixin {
  late final TabController _tabController;

  @override
  void initState() {
    super.initState();
    _tabController = TabController(length: 9, vsync: this);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      _load();
    });
  }

  @override
  void dispose() {
    _tabController.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final api = context.read<NotificationService>();
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey != null) {
        await api.fetchNotifications(pubkey);
        await api.refreshUnreadCount(pubkey);
      }
    } catch (e) {
      debugPrint('notifications load: $e');
    }
  }

  Future<void> _loadType(String type) async {
    var api = context.read<NotificationService>();
    var session = context.read<SessionService>();
    final pubkey = session.activePubkey;
    if (pubkey == null) return;
    try {
      await api.fetchByType(pubkey, type, 50);
    } catch (e) {
      debugPrint('type load: $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final session = context.select((SessionService s) => s.activePubkey);
    final pubkey = session;
    final isLoading = context.select((NotificationService s) => s.isLoading);
    final unreadCount =
        context.select((NotificationService s) => s.unreadCount);

    if (pubkey == null) {
      return Scaffold(
        appBar: AppBar(title: const Text('Notifications')),
        body: const EmptyState(
          icon: Icons.notifications_off_outlined,
          title: 'Sign in to see notifications',
        ),
      );
    }

    return Scaffold(
      appBar: AppBar(
        title: const Text('Notifications'),
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh),
            tooltip: 'Refresh unread',
            onPressed: () async {
              final session = context.read<SessionService>();
              final api = context.read<NotificationService>();
              final key = session.activePubkey;
              if (key == null) return;
              try {
                final unread = await api.fetchUnread(key);
                await api.refreshUnreadCount(key);
                if (!context.mounted) return;
                ScaffoldMessenger.of(context).showSnackBar(
                  SnackBar(
                    content: Text(
                      '${unread.length} unread notification'
                      '${unread.length == 1 ? '' : 's'}',
                    ),
                  ),
                );
              } catch (e) {
                debugPrint('unread refresh: $e');
              }
            },
          ),
          IconButton(
            icon: const Icon(Icons.done_all),
            tooltip: 'Mark all read',
            onPressed: () async {
              final session = context.read<SessionService>();
              final api = context.read<NotificationService>();
              final key = session.activePubkey;
              if (key != null) {
                await api.markAllRead(key);
              }
            },
          ),
          if (unreadCount > 0)
            Center(
              child: Padding(
                padding: const EdgeInsets.only(right: 8),
                child: Text(
                  '$unreadCount unread',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ),
            ),
        ],
        bottom: TabBar(
          controller: _tabController,
          isScrollable: true,
          onTap: (i) {
            const types = [
              '',
              'mention',
              'like',
              'reply',
              'follow',
              'group',
              'event',
              'marketplace',
              'dating',
            ];
            if (i > 0) _loadType(types[i]);
          },
          tabs: const [
            Tab(text: 'All'),
            Tab(text: 'Mentions'),
            Tab(text: 'Reactions'),
            Tab(text: 'Comments'),
            Tab(text: 'Friend Requests'),
            Tab(text: 'Group Activity'),
            Tab(text: 'Events'),
            Tab(text: 'Marketplace'),
            Tab(text: 'Dating'),
          ],
        ),
      ),
      body: isLoading
          ? const Center(child: CircularProgressIndicator())
          : TabBarView(
              controller: _tabController,
              children: [
                _NotificationList(pubkey: pubkey, unreadOnly: false),
                _NotificationList(
                  pubkey: pubkey,
                  unreadOnly: false,
                  type: 'mention',
                  onRefresh: () => _loadType('mention'),
                ),
                _NotificationList(
                  pubkey: pubkey,
                  unreadOnly: false,
                  type: 'like',
                  onRefresh: () => _loadType('like'),
                ),
                _NotificationList(
                  pubkey: pubkey,
                  unreadOnly: false,
                  type: 'reply',
                  onRefresh: () => _loadType('reply'),
                ),
                _NotificationList(
                  pubkey: pubkey,
                  unreadOnly: false,
                  type: 'follow',
                  onRefresh: () => _loadType('follow'),
                ),
                _NotificationList(
                  pubkey: pubkey,
                  unreadOnly: false,
                  type: 'group',
                  onRefresh: () => _loadType('group'),
                ),
                _NotificationList(
                  pubkey: pubkey,
                  unreadOnly: false,
                  type: 'event',
                  onRefresh: () => _loadType('event'),
                ),
                _NotificationList(
                  pubkey: pubkey,
                  unreadOnly: false,
                  type: 'marketplace',
                  onRefresh: () => _loadType('marketplace'),
                ),
                _NotificationList(
                  pubkey: pubkey,
                  unreadOnly: false,
                  type: 'dating',
                  onRefresh: () => _loadType('dating'),
                ),
              ],
            ),
    );
  }
}

class _NotificationList extends StatelessWidget {
  /// Notification list with pull-to-refresh, mark-read, and swipe-to-delete.
  const _NotificationList({
    required this.pubkey,
    required this.unreadOnly,
    this.type,
    this.onRefresh,
  });

  final String pubkey;
  final bool unreadOnly;

  /// Category key (mention/like/reply/message/follow); null shows the "All"
  /// list.
  final String? type;
  final Future<void> Function()? onRefresh;

  Future<void> _refresh(NotificationService api) async {
    if (onRefresh != null) {
      await onRefresh!();
    } else {
      await api.fetchNotifications(pubkey);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Consumer<NotificationService>(
      builder: (context, api, _) {
        final source = type == null ? api.notifications : api.byType(type!);
        final items = source.where((n) => !unreadOnly || !n.read).toList();
        if (items.isEmpty) {
          return RefreshIndicator(
            onRefresh: () => _refresh(api),
            child: ListView(
              physics: const AlwaysScrollableScrollPhysics(),
              children: const [
                SizedBox(height: 120),
                EmptyState(
                  icon: Icons.notifications_none,
                  title: 'Nothing here yet',
                ),
              ],
            ),
          );
        }
        return RefreshIndicator(
          onRefresh: () => _refresh(api),
          child: ListView.builder(
            itemExtent: 72.0,
            itemCount: items.length,
            itemBuilder: (context, index) {
              final n = items[index];
              return Dismissible(
                key: ValueKey(n.id),
                direction: DismissDirection.endToStart,
                background: Container(
                  color: Colors.red,
                  alignment: Alignment.centerRight,
                  padding: const EdgeInsets.only(right: 16),
                  child: const Icon(Icons.delete, color: Colors.white),
                ),
                onDismissed: (_) {
                  api.deleteNotification(n.id);
                },
                child: ListTile(
                  leading: CircleAvatar(
                    backgroundImage: n.fromAvatar.isNotEmpty &&
                            SafeUrl.isSafeMediaUrl(n.fromAvatar)
                        ? ResizeImage.resizeIfNeeded(
                            128, 128, NetworkImage(n.fromAvatar))
                        : null,
                    child: n.fromName.isNotEmpty ? Text(n.fromName[0]) : null,
                  ),
                  title: Text(
                    n.fromName.isNotEmpty ? n.fromName : n.fromPubkey,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  subtitle: Text(
                    n.contentPreview,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      if (!n.read)
                        Icon(Icons.circle,
                            size: 10,
                            color: Theme.of(context).colorScheme.primary),
                      PopupMenuButton<String>(
                        icon: const Icon(Icons.more_vert, size: 18),
                        tooltip: 'Options',
                        onSelected: (val) {
                          if (val == 'ignore') {
                            api.ignoreNotification(n.id);
                            ScaffoldMessenger.of(context).showSnackBar(
                              const SnackBar(
                                  content: Text('Notification ignored')),
                            );
                          } else if (val == 'ignore_user') {
                            api.ignoreUser(n.fromPubkey,
                                userPubkey: context
                                    .read<SessionService>()
                                    .activePubkey);
                            ScaffoldMessenger.of(context).showSnackBar(
                              const SnackBar(content: Text('User ignored')),
                            );
                          } else if (val == 'ignore_thread' &&
                              n.eventId != null) {
                            api.ignoreThread(n.eventId!,
                                userPubkey: context
                                    .read<SessionService>()
                                    .activePubkey);
                            ScaffoldMessenger.of(context).showSnackBar(
                              const SnackBar(
                                  content:
                                      Text('Thread notifications turned off')),
                            );
                          }
                        },
                        itemBuilder: (context) => [
                          const PopupMenuItem(
                            value: 'ignore',
                            child: Row(
                              children: [
                                Icon(Icons.visibility_off_outlined, size: 18),
                                SizedBox(width: 8),
                                Text('Ignore this notification'),
                              ],
                            ),
                          ),
                          if (n.fromPubkey.isNotEmpty)
                            const PopupMenuItem(
                              value: 'ignore_user',
                              child: Row(
                                children: [
                                  Icon(Icons.person_off_outlined, size: 18),
                                  SizedBox(width: 8),
                                  Text('Ignore user'),
                                ],
                              ),
                            ),
                          if (n.eventId != null)
                            const PopupMenuItem(
                              value: 'ignore_thread',
                              child: Row(
                                children: [
                                  Icon(Icons.notifications_off_outlined,
                                      size: 18),
                                  SizedBox(width: 8),
                                  Text('Turn off thread notifications'),
                                ],
                              ),
                            ),
                        ],
                      ),
                    ],
                  ),
                  onTap: () async {
                    await api.markRead(n.id);
                  },
                ),
              );
            },
          ),
        );
      },
    );
  }
}
