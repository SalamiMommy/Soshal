import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/notifications_service.dart';
import '../services/session_service.dart';

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
  bool _loading = false;

  @override
  void initState() {
    super.initState();
    _tabController = TabController(length: 4, vsync: this);
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
    setState(() => _loading = true);
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
    if (mounted) setState(() => _loading = false);
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
    final session = context.watch<SessionService>();
    final pubkey = session.activePubkey;

    if (pubkey == null) {
      return Scaffold(
        appBar: AppBar(title: const Text('Notifications')),
        body: const Center(child: Text('Sign in to see notifications')),
      );
    }

    return Scaffold(
      appBar: AppBar(
        title: const Text('Notifications'),
        actions: [
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
          Consumer<NotificationService>(
            builder: (context, api, _) {
              if (api.unreadCount == 0) return const SizedBox.shrink();
              return Center(
                child: Padding(
                  padding: const EdgeInsets.only(right: 8),
                  child: Text(
                    '${api.unreadCount} unread',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ),
              );
            },
          ),
        ],
        bottom: TabBar(
          controller: _tabController,
          onTap: (i) {
            const types = ['', 'mention', 'reaction', 'reply'];
            if (i > 0) _loadType(types[i]);
          },
          tabs: const [
            Tab(text: 'All'),
            Tab(text: 'Mentions'),
            Tab(text: 'Reactions'),
            Tab(text: 'Replies'),
          ],
        ),
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : TabBarView(
              controller: _tabController,
              children: [
                _NotificationList(pubkey: pubkey, unreadOnly: false),
                _NotificationList(pubkey: pubkey, unreadOnly: true),
                _NotificationList(pubkey: pubkey, unreadOnly: true),
                _NotificationList(pubkey: pubkey, unreadOnly: true),
              ],
            ),
    );
  }
}

class _NotificationList extends StatelessWidget {
  /// Notification list with pull-to-refresh and mark-read.
  const _NotificationList({
    required this.pubkey,
    required this.unreadOnly,
  });

  final String pubkey;
  final bool unreadOnly;

  @override
  Widget build(BuildContext context) {
    return Consumer<NotificationService>(
      builder: (context, api, _) {
        final items =
            api.notifications.where((n) => !unreadOnly || !n.read).toList();
        if (items.isEmpty) {
          return RefreshIndicator(
            onRefresh: () async {
              var session = context.read<SessionService>();
              var key = session.activePubkey;
              if (key != null) {
                await api.fetchNotifications(key);
              }
            },
            child: ListView(
              physics: const AlwaysScrollableScrollPhysics(),
              children: const [
                SizedBox(height: 240),
                Center(child: Text('Nothing here yet')),
              ],
            ),
          );
        }
        return RefreshIndicator(
          onRefresh: () async {
            var session = context.read<SessionService>();
            var key = session.activePubkey;
            if (key != null) {
              await api.fetchNotifications(key);
            }
          },
          child: ListView.builder(
            itemExtent: 72.0,
            itemCount: items.length,
            itemBuilder: (context, index) {
              final n = items[index];
              return ListTile(
                leading: CircleAvatar(
                  backgroundImage: n.fromAvatar.isNotEmpty
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
                trailing: n.read
                    ? null
                    : const Icon(Icons.circle, size: 12, color: Colors.blue),
                onTap: () async {
                  await api.markRead(n.id);
                },
              );
            },
          ),
        );
      },
    );
  }
}
