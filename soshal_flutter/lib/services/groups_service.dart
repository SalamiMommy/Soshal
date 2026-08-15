// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Groups Service
/// NIP-29 group membership, info, messages and admin actions.
class GroupsService extends ChangeNotifier with LastErrorMixin, DeferredNotify {
  List<SoshalGroup> _groups = [];
  SoshalGroup? _current;
  List<String> _members = [];
  List<GroupMessage> _messages = [];
  List<GroupRole> _roles = [];
  List<GroupMemberWithRole> _memberRoles = [];

  List<SoshalGroup> get groups => _groups;
  SoshalGroup? get current => _current;
  List<String> get members => _members;
  List<GroupMessage> get messages => _messages;
  List<GroupRole> get roles => _roles;
  List<GroupMemberWithRole> get memberRoles => _memberRoles;

  Future<List<SoshalGroup>> fetchGroups(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiGroupsGroupsFetchGroups(
        userPubkey: userPubkey,
      );
      _groups = _decodeGroups(json);
      clearLastError();
      notifyDeferred();
      return _groups;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<SoshalGroup> getGroup(String groupId) async {
    try {
      final json = RustLib.instance.api.crateFfiGroupsGroupsGetGroupInfo(
        groupId: groupId,
      );
      _current = SoshalGroup.fromJson(jsonDecode(json));
      clearLastError();
      notifyDeferred();
      return _current!;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<String>> getMembers(String groupId) async {
    try {
      _members = RustLib.instance.api.crateFfiGroupsGroupsGetMembers(
        groupId: groupId,
      );
      clearLastError();
      notifyDeferred();
      return _members;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> join(String groupId, String userPubkey) async {
    try {
      final ok = RustLib.instance.api.crateFfiGroupsGroupsJoin(
        groupId: groupId,
        userPubkey: userPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> leave(String groupId, String userPubkey) async {
    try {
      final ok = RustLib.instance.api.crateFfiGroupsGroupsLeave(
        groupId: groupId,
        userPubkey: userPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<String> postMessage(String groupId, String content) async {
    try {
      final eventJson = RustLib.instance.api.crateFfiGroupsGroupsPostMessage(
        groupId: groupId,
        content: content,
      );
      clearLastError();
      return eventJson;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<GroupMessage>> fetchMessages(String groupId,
      {int limit = 100, int offset = 0}) async {
    try {
      final json = RustLib.instance.api.crateFfiGroupsGroupsFetchMessages(
        groupId: groupId,
        limit: limit,
        offset: offset,
      );
      final decoded = jsonDecode(json);
      final parsed = (decoded as List<dynamic>)
          .map((e) => GroupMessage.fromJson(e as Map<String, dynamic>))
          .toList();
      _messages = parsed.length > 200 ? parsed.sublist(0, 200) : parsed;
      clearLastError();
      notifyDeferred();
      return _messages;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<String> create(
    String groupId,
    String name,
    String description,
    String pictureUrl,
    String creatorPubkey,
  ) async {
    try {
      final id = RustLib.instance.api.crateFfiGroupsGroupsCreate(
        groupId: groupId,
        name: name,
        description: description,
        pictureUrl: pictureUrl,
        creatorPubkey: creatorPubkey,
      );
      clearLastError();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> setMemberRole(
    String groupId,
    String memberPubkey,
    String role,
    String adminPubkey,
  ) async {
    try {
      final ok = RustLib.instance.api.crateFfiGroupsGroupsSetMemberRole(
        groupId: groupId,
        memberPubkey: memberPubkey,
        role: role,
        adminPubkey: adminPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> removeMember(
    String groupId,
    String memberPubkey,
    String adminPubkey,
  ) async {
    try {
      final ok = RustLib.instance.api.crateFfiGroupsGroupsRemoveMember(
        groupId: groupId,
        memberPubkey: memberPubkey,
        adminPubkey: adminPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<GroupRole>> fetchRoles(String groupId) async {
    try {
      final json = RustLib.instance.api.crateFfiGroupsGroupsRolesList(
        groupId: groupId,
      );
      _roles = (jsonDecode(json) as List<dynamic>)
          .map((e) => GroupRole.fromJson(e as Map<String, dynamic>))
          .toList();
      clearLastError();
      notifyDeferred();
      return _roles;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Create (empty [roleId]) or update a custom role. [permissions] is the
  /// JSON array-of-keys string the backend stores.
  Future<bool> upsertRole(
    String groupId, {
    String roleId = '',
    required String name,
    required String color,
    required int position,
    required String permissions,
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiGroupsGroupsRoleUpsert(
        roleId: roleId,
        groupId: groupId,
        name: name,
        color: color,
        position: position,
        permissions: permissions,
      );
      clearLastError();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> deleteRole(String roleId) async {
    try {
      final ok = RustLib.instance.api.crateFfiGroupsGroupsRoleDelete(
        roleId: roleId,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Members with their assigned role ids ({pubkey, role} rows).
  Future<List<GroupMemberWithRole>> fetchMembersWithRoles(
      String groupId) async {
    try {
      final json = RustLib.instance.api.crateFfiGroupsGroupsMembersWithRoles(
        groupId: groupId,
      );
      _memberRoles = (jsonDecode(json) as List<dynamic>)
          .map((e) => GroupMemberWithRole.fromJson(e as Map<String, dynamic>))
          .toList();
      clearLastError();
      notifyDeferred();
      return _memberRoles;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  List<SoshalGroup> _decodeGroups(String json) {
    final decoded = jsonDecode(json);
    return (decoded as List<dynamic>)
        .map((e) => SoshalGroup.fromJson(e as Map<String, dynamic>))
        .toList();
  }
}

/// Group info.
class SoshalGroup {
  final String id;
  final String name;
  final String description;
  final String picture;
  final String owner;
  final int members;
  final bool isMember;
  final String role;
  final int createdAt;

  SoshalGroup({
    required this.id,
    required this.name,
    required this.description,
    required this.picture,
    required this.owner,
    required this.members,
    required this.isMember,
    required this.role,
    required this.createdAt,
  });

  factory SoshalGroup.fromJson(Map<String, dynamic> json) {
    final id = json['id'] as String? ?? '';
    final isMember = json['is_member'] as bool? ?? false;
    return SoshalGroup(
      id: id,
      name: json['name'] as String? ?? '',
      description: json['description'] as String? ?? '',
      picture: json['picture'] as String? ?? '',
      owner: json['owner'] as String? ?? '',
      members: (json['members'] as num?)?.toInt() ?? 0,
      isMember: isMember,
      role: json['role'] as String? ?? (isMember ? 'member' : ''),
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}

/// A group chat message row.
class GroupMessage {
  final String id;
  final String groupId;
  final String senderPubkey;
  final String content;
  final int createdAt;

  GroupMessage({
    required this.id,
    required this.groupId,
    required this.senderPubkey,
    required this.content,
    required this.createdAt,
  });

  factory GroupMessage.fromJson(Map<String, dynamic> json) {
    return GroupMessage(
      id: json['id'] as String? ?? '',
      groupId: json['group_id'] as String? ?? '',
      senderPubkey: json['sender_pubkey'] ?? json['pubkey'] ?? '',
      content: json['content'] as String? ?? '',
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}

/// A custom group role row.
class GroupRole {
  final String id;
  final String groupId;
  final String name;
  final String color;
  final int position;
  final String permissions;
  final int createdAt;

  GroupRole({
    required this.id,
    required this.groupId,
    required this.name,
    required this.color,
    required this.position,
    required this.permissions,
    required this.createdAt,
  });

  factory GroupRole.fromJson(Map<String, dynamic> json) {
    return GroupRole(
      id: json['id'] as String? ?? '',
      groupId: json['group_id'] as String? ?? '',
      name: json['name'] as String? ?? '',
      color: json['color'] as String? ?? '',
      position: (json['position'] as num?)?.toInt() ?? 0,
      permissions: json['permissions'] as String? ?? '[]',
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}

/// A group member with their assigned role id.
class GroupMemberWithRole {
  final String pubkey;
  final String role;

  GroupMemberWithRole({required this.pubkey, required this.role});

  factory GroupMemberWithRole.fromJson(Map<String, dynamic> json) {
    return GroupMemberWithRole(
      pubkey: json['pubkey'] as String? ?? '',
      role: json['role'] as String? ?? 'member',
    );
  }
}

/// System roles with fixed permissions (not editable).
const List<String> systemGroupRoles = ['Owner', 'Admin', 'Member'];

/// The 11 editable permissions for custom roles (key, label), in display
/// order (ported from the legacy group_roles permission matrix).
const List<(String, String)> groupPermissionKeys = [
  ('canPost', 'Post'),
  ('canChat', 'Chat'),
  ('canManageChannels', 'Manage Channels'),
  ('canKick', 'Kick Members'),
  ('canManageEvents', 'Manage Events'),
  ('canInvite', 'Invite Members'),
  ('canDeleteGroup', 'Delete Group'),
  ('pin_posts', 'Pin Posts'),
  ('delete_posts', 'Delete Posts'),
  ('ban_members', 'Ban Members'),
  ('manage_roles', 'Manage Roles'),
];

/// Parses the opaque `permissions` blob (JSON array of keys, or map of
/// booleans) into the active key set, reordered to match
/// [groupPermissionKeys]; unknown keys are preserved at the end.
List<String> groupPermsSet(String raw) {
  final present = <String>[];
  final decoded = jsonDecode(raw);
  if (decoded is List) {
    present.addAll(decoded.whereType<String>());
  } else if (decoded is Map) {
    decoded.forEach((key, value) {
      if (value == true) present.add(key.toString());
    });
  }
  final out = <String>[];
  for (final (key, _) in groupPermissionKeys) {
    final idx = present.indexOf(key);
    if (idx >= 0) out.add(present.removeAt(idx));
  }
  out.addAll(present);
  return out;
}

/// Serializes the active permission set back to the JSON array string the
/// backend stores.
String groupPermsToJson(List<String> active) => jsonEncode(active);

/// Display label for a permission key (falls back to the raw key).
String groupPermLabel(String key) {
  for (final (k, label) in groupPermissionKeys) {
    if (k == key) return label;
  }
  return key;
}

/// 12 preset swatch colors offered in the role edit form (legacy palette).
const List<String> groupRoleColors = [
  '#e94560',
  '#ff6b6b',
  '#feca57',
  '#48dbfb',
  '#1dd1a1',
  '#5f27cd',
  '#ff9ff3',
  '#54a0ff',
  '#ffd32a',
  '#01a3a4',
  '#341f97',
  '#ee5253',
];

/// Safe role color: falls back to neutral gray when not a hex literal.
String groupRoleColor(String color) {
  final trimmed = color.trim();
  final hex = RegExp(r'^#([0-9a-fA-F]{6})$');
  return hex.hasMatch(trimmed) ? trimmed : '#6b7280';
}
