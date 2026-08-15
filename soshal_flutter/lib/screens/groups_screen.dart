import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/groups_service.dart';
import '../services/session_service.dart';

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
              decoration: const InputDecoration(labelText: 'Picture URL'),
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
                            ? CircleAvatar(
                                backgroundImage: ResizeImage.resizeIfNeeded(
                                    128, 128, NetworkImage(g.picture)),
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

/// Group detail: members + admin actions.
class GroupDetailScreen extends StatefulWidget {
  /// Group detail screen.
  const GroupDetailScreen({super.key, required this.groupId});

  final String groupId;

  @override
  State<GroupDetailScreen> createState() => _GroupDetailScreenState();
}

class _GroupDetailScreenState extends State<GroupDetailScreen> {
  bool _loading = true;
  bool _sending = false;
  final TextEditingController _chat = TextEditingController();

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _chat.dispose();
    super.dispose();
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
    } catch (e) {
      debugPrint('group detail: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _sendMessage() async {
    final text = _chat.text.trim();
    if (text.isEmpty) return;
    setState(() => _sending = true);
    try {
      await context.read<GroupsService>().postMessage(widget.groupId, text);
      _chat.clear();
      if (!mounted) return;
      await context.read<GroupsService>().fetchMessages(widget.groupId);
      if (mounted) setState(() {});
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Send failed: $e')));
      }
    } finally {
      if (mounted) setState(() => _sending = false);
    }
  }

  String? _activePubkey() => context.read<SessionService>().activePubkey;

  Future<void> _kickMember(String memberPubkey) async {
    try {
      final pubkey = _activePubkey();
      if (pubkey == null) throw Exception('Sign in');
      await context
          .read<GroupsService>()
          .removeMember(widget.groupId, memberPubkey, pubkey);
      await _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Failed: $e')));
      }
    }
  }

  Future<void> _assignRole(String memberPubkey, String role) async {
    try {
      final pubkey = _activePubkey();
      if (pubkey == null) throw Exception('Sign in');
      await context
          .read<GroupsService>()
          .setMemberRole(widget.groupId, memberPubkey, role, pubkey);
      await _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Failed: $e')));
      }
    }
  }

  Future<void> _saveRole({
    String? roleId,
    required String name,
    required String color,
    required int position,
    required List<String> permissions,
  }) async {
    try {
      await context.read<GroupsService>().upsertRole(
            widget.groupId,
            roleId: roleId ?? '',
            name: name,
            color: color,
            position: position,
            permissions: groupPermsToJson(permissions),
          );
      await _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Save failed: $e')));
      }
    }
  }

  Future<void> _deleteRole(GroupRole role) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Delete role?'),
        content: Text(
            'Delete "${role.name}"? This cannot be undone. Members assigned to it lose their role badge.'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(context).colorScheme.error,
            ),
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Delete'),
          ),
        ],
      ),
    );
    if (confirmed != true || !mounted) return;
    try {
      await context.read<GroupsService>().deleteRole(role.id);
      await _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Delete failed: $e')));
      }
    }
  }

  Future<void> _editRoleDialog({GroupRole? role}) async {
    final name = TextEditingController(text: role?.name ?? '');
    final position =
        TextEditingController(text: (role?.position ?? 1).toString());
    var color = groupRoleColor(role?.color ?? '#8b5cf6');
    final perms = groupPermsSet(role?.permissions ?? '[]').toSet();

    await showDialog<void>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, setDialogState) => AlertDialog(
          title: Text(role == null ? 'New role' : 'Edit role'),
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
                  controller: position,
                  keyboardType: TextInputType.number,
                  decoration: const InputDecoration(labelText: 'Position'),
                ),
                const SizedBox(height: 8),
                const Text('Color',
                    style: TextStyle(fontWeight: FontWeight.w600)),
                const SizedBox(height: 6),
                Wrap(
                  spacing: 6,
                  runSpacing: 6,
                  children: [
                    for (final c in groupRoleColors)
                      GestureDetector(
                        onTap: () => setDialogState(() => color = c),
                        child: Container(
                          width: 26,
                          height: 26,
                          decoration: BoxDecoration(
                            color: _hexColor(c),
                            shape: BoxShape.circle,
                            border: Border.all(
                              width: 2,
                              color: color == c
                                  ? Colors.white
                                  : Colors.transparent,
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
                const SizedBox(height: 12),
                Text(
                  'Permissions (${perms.length} enabled)',
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
                for (final (key, label) in groupPermissionKeys)
                  CheckboxListTile(
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    controlAffinity: ListTileControlAffinity.leading,
                    title: Text(label, style: const TextStyle(fontSize: 13)),
                    value: perms.contains(key),
                    onChanged: (v) => setDialogState(() {
                      if (v == true) {
                        perms.add(key);
                      } else {
                        perms.remove(key);
                      }
                    }),
                  ),
              ],
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () {
                final n = name.text.trim();
                if (n.isEmpty) return;
                Navigator.pop(context);
                _saveRole(
                  roleId: role?.id,
                  name: n,
                  color: color,
                  position: int.tryParse(position.text.trim()) ?? 0,
                  permissions: perms.toList(),
                );
              },
              child: const Text('Save'),
            ),
          ],
        ),
      ),
    );
  }

  (String, String) _roleBadge(String role, List<GroupRole> roles) {
    switch (role) {
      case 'owner':
        return ('Owner', '#f59e0b');
      case 'admin':
        return ('Admin', '#f59e0b');
      case 'member':
        return ('Member', '#6b7280');
    }
    for (final r in roles) {
      if (r.id == role) return (r.name, groupRoleColor(r.color));
    }
    return (role, '#6b7280');
  }

  int _memberRank(String role, List<GroupRole> roles) {
    if (role == 'owner' || role == 'admin') return 0;
    if (roles.any((r) => r.id == role)) return 1;
    return 2;
  }

  int _memberCountForRole(String roleId, List<GroupMemberWithRole> members) =>
      members.where((m) => m.role == roleId).length;

  Color _hexColor(String hex) =>
      Color(0xFF000000 | int.parse(hex.substring(1), radix: 16));

  Widget _roleCard(GroupRole role, GroupsService api, bool isOwner) {
    final color = _hexColor(groupRoleColor(role.color));
    final count = _memberCountForRole(role.id, api.memberRoles);
    final perms = groupPermsSet(role.permissions);
    return Container(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(12),
        border: Border(left: BorderSide(color: color, width: 4)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Container(
                      width: 10,
                      height: 10,
                      decoration:
                          BoxDecoration(color: color, shape: BoxShape.circle),
                    ),
                    const SizedBox(width: 8),
                    Flexible(
                      child: Text(role.name,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(fontWeight: FontWeight.w600)),
                    ),
                    const SizedBox(width: 8),
                    Text('$count members',
                        style: Theme.of(context)
                            .textTheme
                            .bodySmall
                            ?.copyWith(color: Theme.of(context).hintColor)),
                  ],
                ),
                if (perms.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(top: 6),
                    child: Wrap(
                      spacing: 4,
                      runSpacing: 4,
                      children: [
                        for (final p in perms.take(3))
                          Container(
                            padding: const EdgeInsets.symmetric(
                                horizontal: 8, vertical: 2),
                            decoration: BoxDecoration(
                              color: Theme.of(context)
                                  .colorScheme
                                  .surfaceContainer,
                              borderRadius: BorderRadius.circular(999),
                            ),
                            child: Text(groupPermLabel(p),
                                style: const TextStyle(fontSize: 12)),
                          ),
                        if (perms.length > 3)
                          Container(
                            padding: const EdgeInsets.symmetric(
                                horizontal: 8, vertical: 2),
                            decoration: BoxDecoration(
                              color: Theme.of(context)
                                  .colorScheme
                                  .surfaceContainer,
                              borderRadius: BorderRadius.circular(999),
                            ),
                            child: Text('+${perms.length - 3} more',
                                style: const TextStyle(fontSize: 12)),
                          ),
                      ],
                    ),
                  ),
              ],
            ),
          ),
          if (isOwner)
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                IconButton(
                  icon: const Icon(Icons.edit_outlined, size: 18),
                  tooltip: 'Edit role',
                  onPressed: () => _editRoleDialog(role: role),
                ),
                IconButton(
                  icon: const Icon(Icons.delete_outline, size: 18),
                  tooltip: 'Delete role',
                  onPressed: () => _deleteRole(role),
                ),
              ],
            ),
        ],
      ),
    );
  }

  Widget _permissionMatrix(List<GroupRole> roles) {
    Widget cell(Widget child, {bool header = false}) {
      return Container(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 4),
        alignment: Alignment.center,
        child: child,
      );
    }

    final headerStyle =
        Theme.of(context).textTheme.labelSmall?.copyWith(fontSize: 10);
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      padding: const EdgeInsets.symmetric(horizontal: 16),
      child: Table(
        border: TableBorder.all(
          color: Theme.of(context).colorScheme.outlineVariant,
          width: 1,
        ),
        defaultVerticalAlignment: TableCellVerticalAlignment.middle,
        columnWidths: const {0: FixedColumnWidth(140)},
        children: [
          TableRow(
            children: [
              cell(Text('Role', style: headerStyle), header: true),
              for (final (_, label) in groupPermissionKeys)
                cell(
                  Text(label, style: headerStyle, textAlign: TextAlign.center),
                  header: true,
                ),
            ],
          ),
          for (final role in roles)
            TableRow(
              children: [
                cell(
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 4),
                    child: Text(role.name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(fontSize: 12)),
                  ),
                ),
                for (final (key, _) in groupPermissionKeys)
                  cell(
                    groupPermsSet(role.permissions).contains(key)
                        ? Icon(Icons.check,
                            size: 14,
                            color: Theme.of(context).colorScheme.primary)
                        : const Icon(Icons.remove,
                            size: 14, color: Colors.grey),
                  ),
              ],
            ),
        ],
      ),
    );
  }

  List<GroupMemberWithRole> _sortedMemberRows(GroupsService api) {
    final rows = api.memberRoles.isNotEmpty
        ? [...api.memberRoles]
        : [
            for (final p in api.members)
              GroupMemberWithRole(pubkey: p, role: 'member')
          ];
    rows.sort((a, b) => _memberRank(a.role, api.roles)
        .compareTo(_memberRank(b.role, api.roles)));
    return rows;
  }

  Widget _memberTile(
      GroupMemberWithRole m, GroupsService api, bool isOwner, String? me) {
    final (label, hex) = _roleBadge(m.role, api.roles);
    final badgeColor = _hexColor(hex);
    final isSelf = m.pubkey == me;
    final isGroupOwner = m.role == 'owner';
    final roleChoices = <String, String>{
      'member': 'Member',
      'admin': 'Admin',
      for (final r in api.roles) r.id: r.name,
    };
    final canManage = isOwner && !isSelf && !isGroupOwner;
    final selRole = roleChoices.containsKey(m.role) ? m.role : 'member';
    return ListTile(
      leading: const Icon(Icons.person_outline),
      title: Text(m.pubkey, maxLines: 1, overflow: TextOverflow.ellipsis),
      subtitle: Align(
        alignment: Alignment.centerLeft,
        child: Container(
          margin: const EdgeInsets.only(top: 2),
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
          decoration: BoxDecoration(
            borderRadius: BorderRadius.circular(999),
            border: Border.all(color: badgeColor),
          ),
          child: Text(label, style: TextStyle(fontSize: 11, color: badgeColor)),
        ),
      ),
      trailing: canManage
          ? Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                DropdownButton<String>(
                  value: selRole,
                  underline: const SizedBox.shrink(),
                  style: const TextStyle(fontSize: 13),
                  onChanged: (v) {
                    if (v != null && v != m.role) {
                      _assignRole(m.pubkey, v);
                    }
                  },
                  items: [
                    for (final entry in roleChoices.entries)
                      DropdownMenuItem(
                        value: entry.key,
                        child: Text(entry.value),
                      ),
                  ],
                ),
                IconButton(
                  icon: const Icon(Icons.person_remove_outlined),
                  tooltip: 'Remove member',
                  onPressed: () => _kickMember(m.pubkey),
                ),
              ],
            )
          : null,
    );
  }

  @override
  Widget build(BuildContext context) {
    final session = context.watch<SessionService>();
    final me = session.activePubkey;

    return Scaffold(
      appBar: AppBar(title: const Text('Group')),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : Consumer<GroupsService>(
              builder: (context, api, _) {
                final g = api.current;
                if (g == null) {
                  return const Center(child: Text('Group not found'));
                }
                final isOwner = g.owner == me;
                return Column(
                  children: [
                    Expanded(
                      child: ListView(
                        children: [
                          Padding(
                            padding: const EdgeInsets.all(16),
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(g.name,
                                    style: Theme.of(context)
                                        .textTheme
                                        .headlineSmall),
                                if (g.description.isNotEmpty)
                                  Padding(
                                    padding: const EdgeInsets.only(top: 8),
                                    child: Text(g.description),
                                  ),
                                const SizedBox(height: 16),
                                Wrap(
                                  children: [
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
                                      ),
                                    if (g.isMember)
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
                              ],
                            ),
                          ),
                          const Divider(),
                          Padding(
                            padding: const EdgeInsets.symmetric(
                                horizontal: 16, vertical: 8),
                            child: Row(
                              children: [
                                Text('Roles (${api.roles.length})',
                                    style: Theme.of(context)
                                        .textTheme
                                        .titleMedium),
                                const Spacer(),
                                if (isOwner)
                                  TextButton.icon(
                                    onPressed: () => _editRoleDialog(),
                                    icon: const Icon(Icons.add, size: 18),
                                    label: const Text('New Role'),
                                  ),
                              ],
                            ),
                          ),
                          Padding(
                            padding: const EdgeInsets.symmetric(horizontal: 16),
                            child: Wrap(
                              spacing: 8,
                              runSpacing: 8,
                              children: [
                                for (final (label, roleId, hex) in [
                                  ('Owner', 'owner', '#f59e0b'),
                                  ('Admin', 'admin', '#f59e0b'),
                                  ('Member', 'member', '#6b7280'),
                                ])
                                  Container(
                                    padding: const EdgeInsets.symmetric(
                                        horizontal: 10, vertical: 4),
                                    decoration: BoxDecoration(
                                      borderRadius: BorderRadius.circular(999),
                                      border: Border.all(
                                          color: _hexColor(hex), width: 1),
                                    ),
                                    child: Text(
                                      '$label · '
                                      '${_memberCountForRole(roleId, api.memberRoles)}',
                                      style: TextStyle(
                                          fontSize: 12, color: _hexColor(hex)),
                                    ),
                                  ),
                              ],
                            ),
                          ),
                          const SizedBox(height: 8),
                          if (api.roles.isEmpty)
                            const Padding(
                              padding: EdgeInsets.symmetric(horizontal: 16),
                              child: Text('No custom roles yet.',
                                  style: TextStyle(color: Colors.grey)),
                            )
                          else
                            for (final role in api.roles)
                              _roleCard(role, api, isOwner),
                          if (api.roles.isNotEmpty) ...[
                            const SizedBox(height: 12),
                            Padding(
                              padding: const EdgeInsets.symmetric(
                                  horizontal: 16, vertical: 4),
                              child: Text('Permission Matrix',
                                  style:
                                      Theme.of(context).textTheme.titleMedium),
                            ),
                            const Padding(
                              padding: EdgeInsets.only(left: 16, bottom: 8),
                              child: Text(
                                'System roles have fixed permissions; custom roles shown below.',
                                style:
                                    TextStyle(fontSize: 12, color: Colors.grey),
                              ),
                            ),
                            _permissionMatrix(api.roles),
                          ],
                          const SizedBox(height: 8),
                          const Divider(),
                          Padding(
                            padding: const EdgeInsets.symmetric(
                                horizontal: 16, vertical: 8),
                            child: Text('Members (${api.members.length})',
                                style: Theme.of(context).textTheme.titleMedium),
                          ),
                          if (api.members.isEmpty)
                            const Padding(
                              padding: EdgeInsets.symmetric(horizontal: 16),
                              child: Text('No members yet.',
                                  style: TextStyle(color: Colors.grey)),
                            )
                          else
                            for (final m in _sortedMemberRows(api))
                              _memberTile(m, api, isOwner, me),
                          const Divider(),
                          Padding(
                            padding: const EdgeInsets.symmetric(
                                horizontal: 16, vertical: 8),
                            child: Text('Chat (${api.messages.length})',
                                style: Theme.of(context).textTheme.titleMedium),
                          ),
                          if (api.messages.isEmpty)
                            const Padding(
                              padding: EdgeInsets.all(16),
                              child: Text('No messages yet'),
                            )
                          else
                            for (final m in api.messages.reversed)
                              ListTile(
                                dense: true,
                                leading:
                                    const Icon(Icons.person_outline, size: 20),
                                title: Text(
                                  m.senderPubkey.substring(
                                      0,
                                      m.senderPubkey.length >= 12
                                          ? 12
                                          : m.senderPubkey.length),
                                  style: const TextStyle(fontSize: 12),
                                ),
                                subtitle: Text(m.content),
                                isThreeLine: true,
                              ),
                        ],
                      ),
                    ),
                    Container(
                      padding: const EdgeInsets.all(8),
                      child: Row(
                        children: [
                          Expanded(
                            child: TextField(
                              controller: _chat,
                              decoration: InputDecoration(
                                hintText: 'Message group',
                                border: OutlineInputBorder(
                                  borderRadius: BorderRadius.circular(24),
                                ),
                                contentPadding: const EdgeInsets.symmetric(
                                  horizontal: 16,
                                  vertical: 8,
                                ),
                              ),
                              enabled: !_sending,
                            ),
                          ),
                          const SizedBox(width: 8),
                          IconButton(
                            icon: const Icon(Icons.send),
                            onPressed: _sending ? null : _sendMessage,
                          ),
                        ],
                      ),
                    ),
                  ],
                );
              },
            ),
    );
  }
}
