import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../ffi/db.dart' as db_ffi;
import '../services/groups_service.dart';
import '../services/media_service.dart';
import '../services/session_service.dart';
import '../widgets/blob_image.dart';
import '../widgets/group_sidebar.dart';
import '../widgets/group_tabs.dart';

/// Groups: list, join/leave/create, detail view.
class GroupsScreen extends StatefulWidget {
  /// Groups screen.
  const GroupsScreen({super.key});

  @override
  State<GroupsScreen> createState() => _GroupsScreenState();
}

class _GroupsScreenState extends State<GroupsScreen> {
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey != null) {
        await context.read<GroupsService>().fetchGroups(pubkey);
      }
    } catch (e) {
      debugPrint('groups load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _joinOrLeave(SoshalGroup g) async {
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) throw Exception('Sign in');
      final api = context.read<GroupsService>();
      if (g.isMember) {
        await api.leave(g.id, pubkey);
      } else {
        await api.join(g.id, pubkey);
      }
      await _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Error: $e')));
      }
    }
  }

  Future<void> _createDialog() async {
    final name = TextEditingController();
    final desc = TextEditingController();
    final pic = TextEditingController();

    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Create group'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: name,
              decoration: const InputDecoration(labelText: 'Name *'),
            ),
            TextField(
              controller: desc,
              maxLines: 2,
              decoration: const InputDecoration(labelText: 'About'),
            ),
            TextField(
              controller: pic,
              decoration: const InputDecoration(
                labelText: 'Picture',
                hintText: 'pick from device or paste a URL',
              ),
            ),
            const SizedBox(height: 8),
            OutlinedButton.icon(
              onPressed: () async {
                try {
                  final picked =
                      await FilePicker.pickFile(type: FileType.image);
                  final path = picked?.path;
                  if (path == null) return;
                  final manifest =
                      await context.read<MediaService>().uploadMedia(path);
                  final hash = manifest['blob_hash'] as String? ?? '';
                  if (hash.length != 64) {
                    throw Exception('Bad upload manifest');
                  }
                  pic.text = 'n$hash';
                } catch (e) {
                  if (context.mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(
                        SnackBar(content: SelectableText('Upload error: $e')));
                  }
                }
              },
              icon: const Icon(Icons.add_photo_alternate_outlined),
              label: const Text('Pick image from device'),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Create'),
          ),
        ],
      ),
    );

    if (ok == true) {
      if (!mounted) return;
      try {
        final session = context.read<SessionService>();
        final pubkey = session.activePubkey;
        if (pubkey == null) throw Exception('Sign in to create groups');
        final id =
            'grp${DateTime.now().millisecondsSinceEpoch.toRadixString(16)}';
        await context.read<GroupsService>().create(
              id,
              name.text.trim(),
              desc.text.trim(),
              pic.text.trim(),
              pubkey,
            );
        await _load();
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: SelectableText('Create failed: $e')));
        }
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Groups')),
      floatingActionButton: FloatingActionButton(
        onPressed: _createDialog,
        tooltip: 'Create group',
        child: const Icon(Icons.add),
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : Consumer<GroupsService>(
              builder: (context, api, _) {
                if (api.groups.isEmpty) {
                  return const Center(child: Text('No groups yet'));
                }
                return RefreshIndicator(
                  onRefresh: _load,
                  child: ListView.builder(
                    itemExtent: 72.0,
                    itemCount: api.groups.length,
                    itemBuilder: (context, index) {
                      final g = api.groups[index];
                      return ListTile(
                        leading: g.picture.isNotEmpty
                            ? ClipOval(
                                child: BlobImage(
                                  source: g.picture,
                                  width: 48,
                                  height: 48,
                                  errorBuilder: (_) => CircleAvatar(
                                    child: Text(g.name.isEmpty
                                        ? '?'
                                        : g.name[0].toUpperCase()),
                                  ),
                                ),
                              )
                            : CircleAvatar(
                                child: Text(g.name.isEmpty
                                    ? '?'
                                    : g.name[0].toUpperCase()),
                              ),
                        title: Text(g.name),
                        subtitle: Text(
                          '${g.members} members${g.role.isNotEmpty ? ' · ${g.role}' : ''}',
                        ),
                        trailing: g.isMember
                            ? TextButton(
                                onPressed: () => _joinOrLeave(g),
                                child: const Text('Leave'),
                              )
                            : TextButton(
                                onPressed: () => _joinOrLeave(g),
                                child: const Text('Join'),
                              ),
                        onTap: () => context.push('/groups/${g.id}'),
                      );
                    },
                  ),
                );
              },
            ),
    );
  }
}

/// Group detail: tabs (voice / rooms / threads) + members/roles sidebar.
class GroupDetailScreen extends StatefulWidget {
  /// Group detail screen.
  const GroupDetailScreen({super.key, required this.groupId});

  final String groupId;

  @override
  State<GroupDetailScreen> createState() => _GroupDetailScreenState();
}

class _GroupDetailScreenState extends State<GroupDetailScreen> {
  static const double _breakpoint = 700.0;
  static const double _minSidebar = 240.0;
  static const double _maxSidebar = 420.0;
  static const String _widthSetting = 'group_sidebar_width';

  bool _loading = true;
  bool _sidebarOpen = false;
  double _sidebarWidth = 300.0;

  @override
  void initState() {
    super.initState();
    final saved = db_ffi.dbGetSetting(key: _widthSetting);
    if (saved != null) {
      final parsed = double.tryParse(saved);
      if (parsed != null) {
        _sidebarWidth = parsed.clamp(_minSidebar, _maxSidebar);
      }
    }
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final api = context.read<GroupsService>();
      await api.getGroup(widget.groupId);
      try {
        await api.getMembers(widget.groupId);
      } catch (_) {}
      try {
        await api.fetchMembersWithRoles(widget.groupId);
      } catch (_) {}
      try {
        await api.fetchRoles(widget.groupId);
      } catch (_) {}
      try {
        await api.fetchMessages(widget.groupId);
      } catch (e) {
        debugPrint('group messages: $e');
      }
      try {
        await api.fetchRooms(widget.groupId);
      } catch (e) {
        debugPrint('group rooms: $e');
      }
      try {
        await api.fetchThreads(widget.groupId);
      } catch (e) {
        debugPrint('group threads: $e');
      }
      try {
        await api.fetchVoiceChannels(widget.groupId);
      } catch (e) {
        debugPrint('group voice: $e');
      }
    } catch (e) {
      debugPrint('group detail: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  void _persistWidth() {
    db_ffi.dbSetSetting(key: _widthSetting, value: '${_sidebarWidth.round()}');
  }

  Widget _resizeHandle() {
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onHorizontalDragUpdate: (d) => setState(() {
        _sidebarWidth =
            (_sidebarWidth - d.delta.dx).clamp(_minSidebar, _maxSidebar);
      }),
      onHorizontalDragEnd: (_) => _persistWidth(),
      child: MouseRegion(
        cursor: SystemMouseCursors.resizeLeftRight,
        child: Container(
          width: 8,
          color: Theme.of(context)
              .colorScheme
              .outlineVariant
              .withValues(alpha: 0.4),
        ),
      ),
    );
  }

  Widget _tabs(GroupsService api) {
    return DefaultTabController(
      length: 3,
      child: Column(
        children: [
          TabBar(
            tabs: const [
              Tab(icon: Icon(Icons.headphones_outlined), text: 'Voice'),
              Tab(icon: Icon(Icons.forum_outlined), text: 'Rooms'),
              Tab(icon: Icon(Icons.article_outlined), text: 'Threads'),
            ],
          ),
          Expanded(
            child: TabBarView(
              children: [
                GroupVoiceTab(groupId: widget.groupId),
                GroupRoomsTab(groupId: widget.groupId),
                GroupThreadsTab(groupId: widget.groupId),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _sidebar(BuildContext context) {
    final session = context.watch<SessionService>();
    final me = session.activePubkey;
    final api = context.read<GroupsService>();
    final isOwner = api.current != null && api.current!.owner == me;
    return GroupSidebar(
      groupId: widget.groupId,
      isOwner: isOwner,
      me: me,
      onChanged: _load,
    );
  }

  Widget _desktop(Widget tabs, BuildContext context) {
    return Row(
      children: [
        Expanded(child: tabs),
        _resizeHandle(),
        SizedBox(
          width: _sidebarWidth,
          child: Material(
            color: Theme.of(context).colorScheme.surfaceContainerLow,
            child: _sidebar(context),
          ),
        ),
      ],
    );
  }

  Widget _mobile(Widget tabs, BuildContext context) {
    return Stack(
      children: [
        tabs,
        if (_sidebarOpen) ...[
          Positioned.fill(
            child: GestureDetector(
              onTap: () => setState(() => _sidebarOpen = false),
              child: ColoredBox(
                color: Colors.black.withValues(alpha: 0.3),
              ),
            ),
          ),
          Positioned(
            right: 0,
            top: 0,
            bottom: 0,
            width: _sidebarWidth.clamp(
                240.0, MediaQuery.sizeOf(context).width * 0.9),
            child: Material(
              elevation: 8,
              color: Theme.of(context).colorScheme.surface,
              child: Row(
                children: [
                  _resizeHandle(),
                  Expanded(child: _sidebar(context)),
                ],
              ),
            ),
          ),
        ],
        Positioned(
          right: 0,
          top: 0,
          bottom: 0,
          width: 14,
          child: GestureDetector(
            behavior: HitTestBehavior.opaque,
            onHorizontalDragStart: (_) => setState(() => _sidebarOpen = true),
            child: MouseRegion(
              cursor: SystemMouseCursors.click,
              child: ColoredBox(
                color: Colors.transparent,
              ),
            ),
          ),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final session = context.watch<SessionService>();
    final me = session.activePubkey;

    return Scaffold(
      appBar: AppBar(
        title: const Text('Group'),
        actions: [
          IconButton(
            icon: const Icon(Icons.people_alt_outlined),
            tooltip: 'Members & roles',
            onPressed: () => setState(() => _sidebarOpen = !_sidebarOpen),
          ),
        ],
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : Consumer<GroupsService>(
              builder: (context, api, _) {
                final g = api.current;
                if (g == null) {
                  return const Center(child: Text('Group not found'));
                }
                final tabs = _tabs(api);
                return LayoutBuilder(
                  builder: (context, constraints) {
                    final wide = constraints.maxWidth >= _breakpoint;
                    return Column(
                      children: [
                        Container(
                          width: double.infinity,
                          padding: const EdgeInsets.symmetric(
                              horizontal: 16, vertical: 8),
                          color:
                              Theme.of(context).colorScheme.surfaceContainerLow,
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Row(
                                children: [
                                  Expanded(
                                    child: Text(
                                      g.name,
                                      style: Theme.of(context)
                                          .textTheme
                                          .titleMedium,
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                    ),
                                  ),
                                  if (!g.isMember)
                                    FilledButton(
                                      onPressed: () async {
                                        final pubkey = me;
                                        if (pubkey == null) return;
                                        await context
                                            .read<GroupsService>()
                                            .join(widget.groupId, pubkey);
                                        await _load();
                                      },
                                      child: const Text('Join'),
                                    )
                                  else
                                    OutlinedButton(
                                      onPressed: () async {
                                        final pubkey = me;
                                        if (pubkey == null) return;
                                        await context
                                            .read<GroupsService>()
                                            .leave(widget.groupId, pubkey);
                                        if (context.mounted) {
                                          context.go('/groups');
                                        }
                                      },
                                      child: const Text('Leave'),
                                    ),
                                ],
                              ),
                              if (g.description.isNotEmpty)
                                Text(
                                  g.description,
                                  style: Theme.of(context)
                                      .textTheme
                                      .bodySmall
                                      ?.copyWith(
                                          color: Theme.of(context).hintColor),
                                  maxLines: 2,
                                  overflow: TextOverflow.ellipsis,
                                ),
                            ],
                          ),
                        ),
                        Expanded(
                          child: wide
                              ? _desktop(tabs, context)
                              : _mobile(tabs, context),
                        ),
                      ],
                    );
                  },
                );
              },
            ),
    );
  }
}
