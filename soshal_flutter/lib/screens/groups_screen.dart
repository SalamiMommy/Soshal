import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/groups_service.dart';
import '../services/media_service.dart';
import '../services/session_service.dart';
import '../services/settings_service.dart';
import '../widgets/blob_image.dart';
import '../widgets/group_sidebar.dart';
import '../widgets/group_tabs.dart';
import '../widgets/empty_state.dart';

/// Groups: list, join/leave/create, detail view.
class GroupsScreen extends StatefulWidget {
  /// Groups screen.
  const GroupsScreen({super.key});

  @override
  State<GroupsScreen> createState() => _GroupsScreenState();
}

Future<String?> showCommunityPasswordDialog(
    BuildContext context, String groupName) async {
  final controller = TextEditingController();
  var obscure = true;
  return showDialog<String>(
    context: context,
    builder: (context) => StatefulBuilder(
      builder: (context, setDialogState) => AlertDialog(
        title: Row(
          children: [
            const Icon(Icons.lock_outline, color: Colors.amber, size: 24),
            const SizedBox(width: 8),
            const Flexible(child: Text('Private Community')),
          ],
        ),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              'Enter the password to join "$groupName":',
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            const SizedBox(height: 12),
            TextField(
              controller: controller,
              obscureText: obscure,
              autofocus: true,
              decoration: InputDecoration(
                labelText: 'Password',
                prefixIcon: const Icon(Icons.key_outlined),
                suffixIcon: IconButton(
                  icon: Icon(obscure
                      ? Icons.visibility_outlined
                      : Icons.visibility_off_outlined),
                  onPressed: () => setDialogState(() => obscure = !obscure),
                ),
              ),
              onSubmitted: (val) {
                if (val.trim().isNotEmpty) {
                  Navigator.pop(context, val.trim());
                }
              },
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, null),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () {
              final pwd = controller.text.trim();
              if (pwd.isNotEmpty) {
                Navigator.pop(context, pwd);
              }
            },
            child: const Text('Join'),
          ),
        ],
      ),
    ),
  );
}

class _GroupsScreenState extends State<GroupsScreen> {
  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey != null) {
        await context.read<GroupsService>().fetchGroups(pubkey);
      }
    } catch (e) {
      debugPrint('groups load: $e');
    }
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
        String? pwd;
        if (g.isPrivate) {
          pwd = await showCommunityPasswordDialog(context, g.name);
          if (pwd == null) return;
        }
        await api.join(g.id, pubkey, password: pwd);
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
    final password = TextEditingController();
    var isPrivate = false;
    var obscure = true;

    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) => AlertDialog(
          title: const Text('Create group'),
          content: SingleChildScrollView(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
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
                      if (path == null || !context.mounted) return;
                      final manifest =
                          await context.read<MediaService>().uploadMedia(path);
                      final hash = manifest['blob_hash'] as String? ?? '';
                      if (hash.length != 64) {
                        throw Exception('Bad upload manifest');
                      }
                      pic.text = 'n$hash';
                    } catch (e) {
                      if (context.mounted) {
                        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                            content: SelectableText('Upload error: $e')));
                      }
                    }
                  },
                  icon: const Icon(Icons.add_photo_alternate_outlined),
                  label: const Text('Pick image from device'),
                ),
                const SizedBox(height: 12),
                const Divider(),
                SwitchListTile(
                  contentPadding: EdgeInsets.zero,
                  title: const Text('Private community'),
                  subtitle: const Text('Require a password to get in'),
                  value: isPrivate,
                  onChanged: (v) => setDialogState(() => isPrivate = v),
                ),
                if (isPrivate) ...[
                  const SizedBox(height: 4),
                  TextField(
                    controller: password,
                    obscureText: obscure,
                    decoration: InputDecoration(
                      labelText: 'Community Password *',
                      helperText: 'Min. 8 characters required to join',
                      prefixIcon: const Icon(Icons.lock_outline),
                      suffixIcon: IconButton(
                        icon: Icon(obscure
                            ? Icons.visibility_outlined
                            : Icons.visibility_off_outlined),
                        onPressed: () =>
                            setDialogState(() => obscure = !obscure),
                      ),
                    ),
                  ),
                ],
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () {
                if (isPrivate && password.text.trim().length < 8) {
                  ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(
                      content: Text('Password must be at least 8 characters'),
                    ),
                  );
                  return;
                }
                Navigator.pop(context, true);
              },
              child: const Text('Create'),
            ),
          ],
        ),
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
              isPrivate: isPrivate,
              password: isPrivate ? password.text.trim() : null,
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
      body: Consumer<GroupsService>(
              builder: (context, api, _) {
                if (api.groupsLoading && api.groups.isEmpty) {
                  return const Center(child: CircularProgressIndicator());
                }
                if (api.groups.isEmpty) {
                  return const EmptyState(
                    icon: Icons.groups_outlined,
                    title: 'No groups yet',
                  );
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
                        title: Row(
                          children: [
                            Flexible(
                              child: Text(
                                g.name,
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                            if (g.isPrivate) ...[
                              const SizedBox(width: 6),
                              const Icon(
                                Icons.lock_outline,
                                size: 16,
                                color: Colors.amber,
                              ),
                            ],
                          ],
                        ),
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
    final saved = context.read<SettingsService>().getSetting(_widthSetting);
    if (saved.isNotEmpty) {
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
    context
        .read<SettingsService>()
        .setSetting(_widthSetting, '${_sidebarWidth.round()}');
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
                                    child: Row(
                                      children: [
                                        Flexible(
                                          child: Text(
                                            g.name,
                                            style: Theme.of(context)
                                                .textTheme
                                                .titleMedium,
                                            maxLines: 1,
                                            overflow: TextOverflow.ellipsis,
                                          ),
                                        ),
                                        if (g.isPrivate) ...[
                                          const SizedBox(width: 6),
                                          const Icon(
                                            Icons.lock_outline,
                                            size: 18,
                                            color: Colors.amber,
                                          ),
                                        ],
                                      ],
                                    ),
                                  ),
                                  if (!g.isMember)
                                    FilledButton(
                                      onPressed: () async {
                                        final pubkey = me;
                                        if (pubkey == null) return;
                                        String? pwd;
                                        if (g.isPrivate) {
                                          pwd =
                                              await showCommunityPasswordDialog(
                                                  context, g.name);
                                          if (pwd == null) return;
                                        }
                                        if (!context.mounted) return;
                                        try {
                                          await context
                                              .read<GroupsService>()
                                              .join(widget.groupId, pubkey,
                                                  password: pwd);
                                          await _load();
                                        } catch (e) {
                                          if (context.mounted) {
                                            ScaffoldMessenger.of(context)
                                                .showSnackBar(SnackBar(
                                                    content: SelectableText(
                                                        'Join failed: $e')));
                                          }
                                        }
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
                          child: (g.isPrivate && !g.isMember)
                              ? Center(
                                  child: SingleChildScrollView(
                                    padding: const EdgeInsets.all(24),
                                    child: Container(
                                      constraints:
                                          const BoxConstraints(maxWidth: 420),
                                      padding: const EdgeInsets.all(24),
                                      decoration: BoxDecoration(
                                        color: Theme.of(context)
                                            .colorScheme
                                            .surfaceContainerHighest,
                                        borderRadius: BorderRadius.circular(16),
                                        border: Border.all(
                                          color: Colors.amber
                                              .withValues(alpha: 0.5),
                                        ),
                                      ),
                                      child: Column(
                                        mainAxisSize: MainAxisSize.min,
                                        children: [
                                          const Icon(Icons.lock_outline,
                                              size: 52, color: Colors.amber),
                                          const SizedBox(height: 12),
                                          Text(
                                            'Private Community',
                                            style: Theme.of(context)
                                                .textTheme
                                                .titleLarge
                                                ?.copyWith(
                                                    fontWeight:
                                                        FontWeight.bold),
                                          ),
                                          const SizedBox(height: 8),
                                          Text(
                                            'This community is password-protected. Join with the correct password to access channels, rooms, and discussions.',
                                            textAlign: TextAlign.center,
                                            style: Theme.of(context)
                                                .textTheme
                                                .bodyMedium
                                                ?.copyWith(
                                                    color: Theme.of(context)
                                                        .hintColor),
                                          ),
                                          const SizedBox(height: 20),
                                          FilledButton.icon(
                                            icon:
                                                const Icon(Icons.key_outlined),
                                            label: const Text(
                                                'Enter Password to Join'),
                                            onPressed: () async {
                                              final pubkey = me;
                                              if (pubkey == null) return;
                                              String? pwd;
                                              if (g.isPrivate) {
                                                pwd =
                                                    await showCommunityPasswordDialog(
                                                        context, g.name);
                                                if (pwd == null) return;
                                              }
                                              if (!context.mounted) return;
                                              try {
                                                await context
                                                    .read<GroupsService>()
                                                    .join(
                                                        widget.groupId, pubkey,
                                                        password: pwd);
                                                await _load();
                                              } catch (e) {
                                                if (context.mounted) {
                                                  ScaffoldMessenger.of(context)
                                                      .showSnackBar(SnackBar(
                                                          content: SelectableText(
                                                              'Join failed: $e')));
                                                }
                                              }
                                            },
                                          ),
                                        ],
                                      ),
                                    ),
                                  ),
                                )
                              : wide
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
