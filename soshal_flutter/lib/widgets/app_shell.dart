import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/notifications_service.dart';
import '../services/session_service.dart';
import '../services/shell_service.dart';
import 'lock_screen.dart';

/// Adaptive app shell: NavigationRail on wide screens, drawer + hamburger on
/// narrow ones. Mirrors the legacy rust-native sidebar layout (reorderable
/// nav, search box, unread badge, offline banner, lock screen, audio bar,
/// incoming-call banner).
class AppShell extends StatefulWidget {
  final Widget child;
  const AppShell({super.key, required this.child});

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  @override
  void initState() {
    super.initState();
    _bootstrap();
  }

  Future<void> _bootstrap() async {
    final shell = context.read<ShellService>();
    await shell.initialize();
    final session = context.read<SessionService>();
    final pubkey = session.activePubkey;
    if (pubkey != null) {
      context.read<NotificationService>().refreshUnreadCount(pubkey);
    }
    shell.refreshRelayStatus();
  }

  String? get _currentPath {
    final loc = GoRouterState.of(context).uri.toString();
    return loc.split('?').first;
  }

  /// Show the hamburger on top-level shell routes only (detail routes get a
  /// back arrow from their own AppBar).
  bool get _isTopLevel {
    final path = _currentPath ?? '/feed';
    if (path == '/feed' ||
        path == '/notifications' ||
        path == '/inbox' ||
        path == '/groups' ||
        path == '/dating' ||
        path == '/marketplace' ||
        path == '/events' ||
        path == '/minis' ||
        path == '/stories' ||
        path == '/live' ||
        path == '/music' ||
        path == '/chat-random' ||
        path == '/friends' ||
        path == '/profile' ||
        path == '/bookmarks' ||
        path == '/settings' ||
        path == '/scheduled' ||
        path == '/vouch' ||
        path == '/stealth' ||
        path == '/analytics' ||
        path == '/audit' ||
        path == '/network') {
      return true;
    }
    return false;
  }

  @override
  Widget build(BuildContext context) {
    final offline = context.select((ShellService s) => s.offline);
    final incomingCall = context.select((ShellService s) => s.incomingCall);
    final audioPlaying = context.select((ShellService s) => s.audioPlaying);
    final locked = context.select((ShellService s) => s.locked);
    return LayoutBuilder(
      builder: (context, constraints) {
        final wide = constraints.maxWidth >= 800;
        final scaffold = Scaffold(
          body: Stack(
            children: [
              Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (wide) _NavigationRailView(currentPath: _currentPath),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        if (offline) const _OfflineBanner(),
                        Expanded(
                          child: Stack(
                            children: [
                              widget.child,
                              if (!wide && _isTopLevel)
                                const _HamburgerButton(),
                              if (incomingCall != null)
                                _IncomingCallBanner(
                                  call: incomingCall,
                                ),
                              if (audioPlaying) const _GlobalAudioBar(),
                            ],
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ),
              if (locked) const LockScreen(),
            ],
          ),
          drawer: wide ? null : _SidebarDrawer(currentPath: _currentPath),
        );
        return scaffold;
      },
    );
  }
}

// ─── Sidebar content (shared by rail + drawer) ──────────────────────────────

class _SidebarCore extends StatefulWidget {
  final String? currentPath;
  final bool compact;
  final VoidCallback? onNavigate;

  const _SidebarCore({
    required this.currentPath,
    required this.compact,
    this.onNavigate,
  });

  @override
  State<_SidebarCore> createState() => _SidebarCoreState();
}

class _SidebarCoreState extends State<_SidebarCore> {
  final TextEditingController _search = TextEditingController();

  void _navigate(BuildContext context, String route) {
    widget.onNavigate?.call();
    context.go(route);
  }

  @override
  Widget build(BuildContext context) {
    final shell = context.watch<ShellService>();
    final unreadCount =
        context.select<NotificationService, int>((s) => s.unreadCount);
    final theme = Theme.of(context);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (!widget.compact)
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 16, 16, 8),
            child: Row(
              children: [
                Text(
                  'Soshal',
                  style: theme.textTheme.titleLarge?.copyWith(
                    fontWeight: FontWeight.bold,
                  ),
                ),
                const Spacer(),
                IconButton(
                  tooltip: 'Close',
                  icon: const Icon(Icons.close),
                  onPressed: () => Navigator.of(context).pop(),
                ),
              ],
            ),
          ),
        Padding(
          padding: EdgeInsets.fromLTRB(
            widget.compact ? 0 : 16,
            widget.compact ? 12 : 0,
            widget.compact ? 0 : 16,
            8,
          ),
          child: TextField(
            controller: _search,
            decoration: const InputDecoration(
              hintText: 'Search...',
              prefixIcon: Icon(Icons.search),
              isDense: true,
              border: OutlineInputBorder(),
            ),
            onSubmitted: (q) {
              if (q.trim().isNotEmpty) {
                _navigate(
                    context, '/search?q=${Uri.encodeComponent(q.trim())}');
              }
            },
          ),
        ),
        Expanded(
          child: ListView(
            padding: EdgeInsets.symmetric(
              vertical: 4,
              horizontal: widget.compact ? 4 : 8,
            ),
            children: [
              for (int i = 0; i < shell.items.length; i++)
                _NavItemTile(
                  item: shell.items[i],
                  index: i,
                  unread: shell.items[i].id == 'inbox' ? unreadCount : 0,
                  selected: widget.currentPath ==
                      (ShellService.routeForItem[shell.items[i].id] ?? ''),
                  compact: widget.compact,
                  onTap: () => _navigate(context,
                      ShellService.routeForItem[shell.items[i].id] ?? '/feed'),
                ),
            ],
          ),
        ),
      ],
    );
  }
}

class _NavItemTile extends StatelessWidget {
  final NavItem item;
  final int index;
  final int unread;
  final bool selected;
  final bool compact;
  final VoidCallback onTap;

  const _NavItemTile({
    required this.item,
    required this.index,
    required this.unread,
    required this.selected,
    required this.compact,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final shell = context.watch<ShellService>();
    final tile = ListTile(
      leading: Icon(navIconFor(item.id)),
      title: Row(
        children: [
          Expanded(child: Text(item.label)),
          if (unread > 0)
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
              decoration: BoxDecoration(
                color: Theme.of(context).colorScheme.primary,
                borderRadius: BorderRadius.circular(12),
              ),
              child: Text(
                '$unread',
                style: TextStyle(
                  color: Theme.of(context).colorScheme.onPrimary,
                  fontSize: 12,
                ),
              ),
            ),
          if (shell.rearranging) ...[
            const SizedBox(width: 4),
            const Icon(Icons.drag_handle, size: 18),
          ],
        ],
      ),
      selected: selected,
      selectedTileColor: Theme.of(context)
          .colorScheme
          .secondaryContainer
          .withValues(alpha: 0.4),
      onTap: shell.rearranging ? null : onTap,
      onLongPress: shell.rearranging ? null : shell.beginRearrange,
    );

    if (!shell.rearranging) return tile;

    return DragTarget<int>(
      onWillAcceptWithDetails: (details) => details.data != index,
      onAcceptWithDetails: (details) => shell.moveItem(details.data, index),
      builder: (context, candidates, rejected) {
        return LongPressDraggable<int>(
          data: index,
          feedback: Material(
            elevation: 4,
            borderRadius: BorderRadius.circular(8),
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(navIconFor(item.id)),
                  const SizedBox(width: 8),
                  Text(item.label),
                ],
              ),
            ),
          ),
          childWhenDragging: Opacity(opacity: 0.4, child: tile),
          child: tile,
        );
      },
    );
  }
}

/// Icon for a nav item id (shared by rail, drawer, drag feedback).
IconData navIconFor(String id) {
  const icons = <String, IconData>{
    'feed': Icons.rss_feed,
    'notifications': Icons.notifications_outlined,
    'inbox': Icons.chat_bubble_outline,
    'groups': Icons.groups_outlined,
    'dating': Icons.favorite_outline,
    'marketplace': Icons.storefront_outlined,
    'events': Icons.event_outlined,
    'minis': Icons.widgets_outlined,
    'stories': Icons.auto_stories_outlined,
    'live': Icons.videocam_outlined,
    'music': Icons.music_note_outlined,
    'chat_random': Icons.casino_outlined,
    'friends': Icons.people_outline,
    'profile': Icons.person_outline,
    'bookmarks': Icons.bookmark_outline,
    'settings': Icons.settings_outlined,
    'scheduled': Icons.schedule_outlined,
    'vouch': Icons.verified_outlined,
    'stealth': Icons.visibility_off_outlined,
    'analytics': Icons.insights_outlined,
    'audit': Icons.fact_check_outlined,
    'backup': Icons.backup_outlined,
    'network': Icons.hub_outlined,
  };
  return icons[id] ?? Icons.circle_outlined;
}

class _NavigationRailView extends StatelessWidget {
  final String? currentPath;
  const _NavigationRailView({required this.currentPath});

  @override
  Widget build(BuildContext context) {
    final shell = context.watch<ShellService>();
    return Expanded(
      child: SingleChildScrollView(
        child: NavigationRail(
          selectedIndex: _selectedIndex(shell, currentPath),
          onDestinationSelected: (i) {
            if (i >= 0 && i < shell.items.length) {
              context
                  .go(ShellService.routeForItem[shell.items[i].id] ?? '/feed');
            }
          },
          extended: true,
          leading: const Padding(
            padding: EdgeInsets.symmetric(vertical: 12),
            child: Text(
              'Soshal',
              style: TextStyle(fontWeight: FontWeight.bold, fontSize: 18),
            ),
          ),
          destinations: [
            for (final item in shell.items)
              NavigationRailDestination(
                icon: Icon(navIconFor(item.id)),
                selectedIcon: Icon(navIconFor(item.id)),
                label: Text(item.label),
              ),
          ],
        ),
      ),
    );
  }

  static int _selectedIndex(ShellService shell, String? path) {
    for (int i = 0; i < shell.items.length; i++) {
      if (path == ShellService.routeForItem[shell.items[i].id]) return i;
    }
    return 0;
  }
}

class _SidebarDrawer extends StatelessWidget {
  final String? currentPath;
  const _SidebarDrawer({required this.currentPath});

  @override
  Widget build(BuildContext context) {
    return SafeArea(
      child: _SidebarCore(
        currentPath: currentPath,
        compact: false,
        onNavigate: () => Navigator.of(context).pop(),
      ),
    );
  }
}

// ─── Small shell chrome widgets ─────────────────────────────────────────────

class _OfflineBanner extends StatelessWidget {
  const _OfflineBanner();

  @override
  Widget build(BuildContext context) {
    return Material(
      color: Theme.of(context).colorScheme.error,
      child: SafeArea(
        bottom: false,
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 16),
          child: Text(
            'Offline — no relays connected',
            textAlign: TextAlign.center,
            style: TextStyle(
              color: Theme.of(context).colorScheme.onError,
              fontSize: 12,
            ),
          ),
        ),
      ),
    );
  }
}

class _HamburgerButton extends StatelessWidget {
  const _HamburgerButton();

  @override
  Widget build(BuildContext context) {
    return Positioned(
      top: 4,
      left: 4,
      child: SafeArea(
        bottom: false,
        child: Material(
          elevation: 2,
          shape: const CircleBorder(),
          color: Theme.of(context).colorScheme.surface,
          child: IconButton(
            icon: const Icon(Icons.menu),
            tooltip: 'Open Menu',
            onPressed: () => Scaffold.of(context).openDrawer(),
          ),
        ),
      ),
    );
  }
}

class _IncomingCallBanner extends StatelessWidget {
  final Map<String, dynamic> call;
  const _IncomingCallBanner({required this.call});

  @override
  Widget build(BuildContext context) {
    final shell = context.read<ShellService>();
    final peer = call['pubkey'] as String? ?? '';
    final callId = call['call_id'] as String? ?? '';
    final content =
        call['content'] is String ? call['content'] as String : '{}';
    String mediaType = 'voice';
    try {
      mediaType = (jsonDecode(content) as Map<String, dynamic>)['media_type']
              as String? ??
          'voice';
    } catch (_) {}
    final short = peer.length > 12
        ? '${peer.substring(0, 6)}…${peer.substring(peer.length - 4)}'
        : peer;
    return Positioned(
      top: 8,
      left: 0,
      right: 0,
      child: SafeArea(
        child: Card(
          margin: const EdgeInsets.symmetric(horizontal: 16),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            child: Row(
              children: [
                const Icon(Icons.call),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                      'Incoming ${mediaType == 'video' ? 'video ' : ''}call from $short'),
                ),
                FilledButton(
                  onPressed: () {
                    shell.acceptCall();
                    context.push(
                      '/call/$peer/$mediaType/$callId',
                    );
                  },
                  child: const Text('Accept'),
                ),
                const SizedBox(width: 4),
                OutlinedButton(
                  onPressed: () => shell.declineCall(),
                  child: const Text('Decline'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _GlobalAudioBar extends StatelessWidget {
  const _GlobalAudioBar();

  @override
  Widget build(BuildContext context) {
    final shell = context.watch<ShellService>();
    return Positioned(
      left: 0,
      right: 0,
      bottom: 0,
      child: SafeArea(
        top: false,
        child: Card(
          margin: const EdgeInsets.fromLTRB(12, 0, 12, 8),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            child: Row(
              children: [
                const Icon(Icons.music_note),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    shell.audioTitle,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                IconButton(
                  icon: const Icon(Icons.close),
                  tooltip: 'Stop',
                  onPressed: () => shell.stopAudio(),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
