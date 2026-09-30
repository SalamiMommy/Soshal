import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/groups_service.dart';
import 'group_tabs.dart' show hexColor;

/// Right sidebar: members (with role assignment) + roles management.
class GroupSidebar extends StatefulWidget {
  /// Right sidebar for the group detail screen.
  const GroupSidebar({
    super.key,
    required this.groupId,
    required this.isOwner,
    required this.me,
    required this.onChanged,
  });

  final String groupId;
  final bool isOwner;
  final String? me;
  final VoidCallback onChanged;

  @override
  State<GroupSidebar> createState() => _GroupSidebarState();
}

class _GroupSidebarState extends State<GroupSidebar> {
  Future<void> _kickMember(String memberPubkey) async {
    try {
      final pubkey = widget.me;
      if (pubkey == null) throw Exception('Sign in');
      await context
          .read<GroupsService>()
          .removeMember(widget.groupId, memberPubkey, pubkey);
      widget.onChanged();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Failed: $e')));
      }
    }
  }

  Future<void> _assignRole(String memberPubkey, String role) async {
    try {
      final pubkey = widget.me;
      if (pubkey == null) throw Exception('Sign in');
      await context
          .read<GroupsService>()
          .setMemberRole(widget.groupId, memberPubkey, role, pubkey);
      widget.onChanged();
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
      widget.onChanged();
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
      widget.onChanged();
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
    try {
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
                              color: hexColor(c),
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
    } finally {
      name.dispose();
      position.dispose();
    }
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

  Widget _roleCard(GroupRole role, GroupsService api) {
    final color = hexColor(groupRoleColor(role.color));
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
          if (widget.isOwner)
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
    // One permission parse per role row, memoized across rebuilds. This used to
    // be `groupPermsSet(role.permissions)` *inside* the per-cell loop below, so
    // a server with 20 roles paid 400 `jsonDecode` calls plus 400 list rebuilds
    // every time the matrix was built -- and the matrix is inside a
    // `watch<GroupsService>()` subtree, so any notification re-paid it.
    final permsByRole = <Set<String>>[
      for (final role in roles) _permissionKeysFor(role.permissions),
    ];
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
          for (var i = 0; i < roles.length; i++)
            TableRow(
              children: [
                cell(
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 4),
                    child: Text(roles[i].name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(fontSize: 12)),
                  ),
                ),
                for (final (key, _) in groupPermissionKeys)
                  cell(
                    permsByRole[i].contains(key)
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

  List<GroupMemberWithRole>? _membersRowsCache;
  List<String>? _membersRowsCacheKey;

  /// Parsed permission keys per raw `permissions` blob.
  ///
  /// The permission matrix is inside a `watch<GroupsService>()` subtree, so it
  /// is rebuilt on every notification from anywhere in the group service, while
  /// a role's blob only changes when that role is edited. Hoisting the parse
  /// out of the per-cell loop removed the factor of ~20; this removes the
  /// repeats across rebuilds.
  ///
  /// Keyed on the raw string, so a stale entry can only ever be a *wasted*
  /// entry, never a wrong one — the mapping is a pure function of the key.
  /// That is why this needs no account-switch reset; the size cap is the only
  /// thing that has to be maintained.
  static const int _permKeyCacheCap = 512;
  final Map<String, Set<String>> _permKeyCache = {};

  /// The permission keys active in [raw], as a set.
  Set<String> _permissionKeysFor(String raw) {
    final hit = _permKeyCache[raw];
    if (hit != null) return hit;
    // Cleared wholesale rather than evicted: the contents are derived, so
    // dropping all of them costs one rebuild of the matrix and cannot make the
    // answer wrong.
    if (_permKeyCache.length >= _permKeyCacheCap) _permKeyCache.clear();
    return _permKeyCache[raw] = groupPermsSet(raw).toSet();
  }

  List<GroupMemberWithRole> _sortedMemberRowsCached(GroupsService api) {
    final members = api.members;
    if (!identical(_membersRowsCacheKey, members)) {
      _membersRowsCache = _sortedMemberRows(api);
      _membersRowsCacheKey = members;
    }
    return _membersRowsCache!;
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

  Widget _memberTile(GroupMemberWithRole m, GroupsService api) {
    final (label, hex) = _roleBadge(m.role, api.roles);
    final badgeColor = hexColor(hex);
    final isSelf = m.pubkey == widget.me;
    final isGroupOwner = m.role == 'owner';
    final roleChoices = <String, String>{
      'member': 'Member',
      'admin': 'Admin',
      for (final r in api.roles) r.id: r.name,
    };
    final canManage = widget.isOwner && !isSelf && !isGroupOwner;
    final selRole = roleChoices.containsKey(m.role) ? m.role : 'member';
    return ListTile(
      dense: true,
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

  Future<void> _editPasswordDialog(bool isPrivate) async {
    final passwordController = TextEditingController();
    try {
      var obscure = true;
      final confirmed = await showDialog<bool>(
        context: context,
        builder: (context) => StatefulBuilder(
          builder: (context, setDialogState) => AlertDialog(
            title: Text(isPrivate
                ? 'Change Community Password'
                : 'Set Community Password'),
            content: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  isPrivate
                      ? 'Enter a new password or leave blank to make the community public.'
                      : 'Setting a password will make this community private. Members will need this password to join.',
                  style: Theme.of(context).textTheme.bodyMedium,
                ),
                const SizedBox(height: 12),
                TextField(
                  controller: passwordController,
                  obscureText: obscure,
                  decoration: InputDecoration(
                    labelText: 'New Password',
                    helperText: isPrivate
                        ? 'Min. 8 chars (leave empty to make public)'
                        : 'Min. 8 characters',
                    prefixIcon: const Icon(Icons.lock_outline),
                    suffixIcon: IconButton(
                      icon: Icon(obscure
                          ? Icons.visibility_outlined
                          : Icons.visibility_off_outlined),
                      onPressed: () => setDialogState(() => obscure = !obscure),
                    ),
                  ),
                ),
              ],
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: const Text('Cancel'),
              ),
              FilledButton(
                onPressed: () {
                  final pwd = passwordController.text.trim();
                  if (pwd.isNotEmpty && pwd.length < 8) {
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(
                        content: Text('Password must be at least 8 characters'),
                      ),
                    );
                    return;
                  }
                  Navigator.pop(context, true);
                },
                child: const Text('Save'),
              ),
            ],
          ),
        ),
      );

      if (confirmed == true) {
        if (!mounted) return;
        try {
          final pubkey = widget.me;
          if (pubkey == null) return;
          final pwd = passwordController.text.trim();
          await context.read<GroupsService>().setPassword(
                widget.groupId,
                pwd.isEmpty ? null : pwd,
                pubkey,
              );
          widget.onChanged();
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(
                content: Text(pwd.isEmpty
                    ? 'Community is now open (password removed)'
                    : 'Community password updated'),
              ),
            );
          }
        } catch (e) {
          if (mounted) {
            ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: SelectableText('Error: $e')),
            );
          }
        }
      }
    } finally {
      passwordController.dispose();
    }
  }

  @override
  Widget build(BuildContext context) {
    final api = context.watch<GroupsService>();
    final group = api.current;
    final isPrivate = group?.isPrivate ?? false;

    final memberRows = _sortedMemberRowsCached(api);
    // Everything above the member list is a bounded number of widgets, so it is
    // built once here and indexed into. Order is preserved exactly, empty-state
    // included; the only unbounded part is the member rows below.
    final header = <Widget>[
      if (widget.isOwner) ...[
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
          child: Row(
            children: [
              const Icon(Icons.security_outlined, size: 18),
              const SizedBox(width: 8),
              Text('Privacy & Access',
                  style: Theme.of(context).textTheme.titleSmall),
            ],
          ),
        ),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16),
          child: Container(
            padding: const EdgeInsets.all(12),
            decoration: BoxDecoration(
              color: Theme.of(context).colorScheme.surfaceContainerHighest,
              borderRadius: BorderRadius.circular(12),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(
                      isPrivate ? Icons.lock_outline : Icons.public_outlined,
                      size: 16,
                      color: isPrivate ? Colors.amber : Colors.green,
                    ),
                    const SizedBox(width: 6),
                    Expanded(
                      child: Text(
                        isPrivate
                            ? 'Private (Password protected)'
                            : 'Public (Open to all)',
                        style: const TextStyle(
                            fontWeight: FontWeight.w600, fontSize: 13),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                OutlinedButton.icon(
                  onPressed: () => _editPasswordDialog(isPrivate),
                  icon: Icon(
                      isPrivate ? Icons.edit_outlined : Icons.lock_outline,
                      size: 16),
                  label: Text(isPrivate
                      ? 'Change / Remove Password'
                      : 'Set Password (Make Private)'),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 8),
        const Divider(),
      ],
      Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        child: Row(
          children: [
            Expanded(
              child: Text('Roles (${api.roles.length})',
                  style: Theme.of(context).textTheme.titleSmall,
                  overflow: TextOverflow.ellipsis),
            ),
            if (widget.isOwner)
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
                padding:
                    const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                decoration: BoxDecoration(
                  borderRadius: BorderRadius.circular(999),
                  border: Border.all(color: hexColor(hex), width: 1),
                ),
                child: Text(
                  '$label · ${_memberCountForRole(roleId, api.memberRoles)}',
                  style: TextStyle(fontSize: 12, color: hexColor(hex)),
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
        for (final role in api.roles) _roleCard(role, api),
      if (api.roles.isNotEmpty) ...[
        const SizedBox(height: 12),
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
          child: Text('Permission Matrix',
              style: Theme.of(context).textTheme.titleSmall),
        ),
        const Padding(
          padding: EdgeInsets.only(left: 16, bottom: 8),
          child: Text(
            'System roles have fixed permissions; custom roles shown below.',
            style: TextStyle(fontSize: 12, color: Colors.grey),
          ),
        ),
        _permissionMatrix(api.roles),
      ],
      const Divider(),
      Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        child: Text('Members (${api.members.length})',
            style: Theme.of(context).textTheme.titleSmall),
      ),
      if (api.members.isEmpty)
        const Padding(
          padding: EdgeInsets.symmetric(horizontal: 16),
          child: Text('No members yet.', style: TextStyle(color: Colors.grey)),
        )
    ];

    return ListView.builder(
      // Only the member rows go through `itemBuilder`. A 5 000-member server
      // was building 5 000 tiles on every build, and this list sits inside a
      // `watch<GroupsService>()` subtree, so any notification from anywhere in
      // the group service re-paid the whole list.
      itemCount: header.length + memberRows.length + 1,
      itemBuilder: (context, index) {
        if (index < header.length) return header[index];
        final i = index - header.length;
        if (i < memberRows.length) return _memberTile(memberRows[i], api);
        return const SizedBox(height: 16);
      },
    );
  }
}
