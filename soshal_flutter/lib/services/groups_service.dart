// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Groups Service
/// NIP-29 group membership, info, messages and admin actions.
class GroupsService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify, ServiceGuard {
  List<SoshalGroup> _groups = [];
  bool _groupsLoading = false;
  SoshalGroup? _current;
  List<String> _members = [];
  List<GroupMessage> _messages = [];
  List<GroupRole> _roles = [];
  List<GroupMemberWithRole> _memberRoles = [];
  List<GroupRoom> _rooms = [];
  List<GroupThread> _threads = [];
  List<GroupThreadReply> _replies = [];
  final Map<String, List<ThreadReaction>> _reactionsByThread = {};
  List<GroupVoiceChannel> _voiceChannels = [];
  List<GroupVoicePresence> _presence = [];

  /// Thread feed sort mode.
  ThreadSort _threadSort = ThreadSort.popular;
  ThreadSort get threadSort => _threadSort;

  set threadSort(ThreadSort sort) {
    if (_threadSort == sort) return;
    _threadSort = sort;
    notifyDeferred();
  }

  List<SoshalGroup> get groups => _groups;
  bool get groupsLoading => _groupsLoading;
  SoshalGroup? get current => _current;
  List<String> get members => _members;
  List<GroupMessage> get messages => _messages;
  List<GroupRole> get roles => _roles;
  List<GroupMemberWithRole> get memberRoles => _memberRoles;
  List<GroupRoom> get rooms => _rooms;
  List<GroupThread> get threads => _threads;
  List<GroupThreadReply> get replies => _replies;
  List<ThreadReaction> get reactions =>
      _reactionsByThread.values.expand((list) => list).toList();
  List<GroupVoiceChannel> get voiceChannels => _voiceChannels;
  List<GroupVoicePresence> get presence => _presence;

  /// Clear all account-scoped state on account switch so Account B never
  /// sees Account A's cached groups, members, messages, roles, rooms,
  /// threads, reactions, or voice presences.
  void resetForAccountSwitch() {
    _groups = [];
    _groupsLoading = false;
    _current = null;
    _members = [];
    _messages = [];
    _roles = [];
    _memberRoles = [];
    _rooms = [];
    _threads = [];
    _replies = [];
    _reactionsByThread.clear();
    _voiceChannels = [];
    _presence = [];
    clearLastError();
    notifyListeners();
  }

  /// Emoji reactions for a target (thread or reply id).
  List<ThreadReaction> reactionsFor(String targetId) {
    final direct = _reactionsByThread[targetId];
    if (direct != null) return direct;
    final out = <ThreadReaction>[];
    for (final list in _reactionsByThread.values) {
      for (final r in list) {
        if (r.matches(targetId)) out.add(r);
      }
    }
    return out;
  }

  Future<List<SoshalGroup>> fetchGroups(String userPubkey) async {
    _groupsLoading = true;
    notifyDeferred();
    try {
      final json = RustLib.instance.api.crateFfiGroupsGroupsFetchGroups(
        userPubkey: userPubkey,
        audience: 'public',
      );
      _groups = _decodeGroups(json);
      _groupsLoading = false;
      clearLastError();
      notifyDeferred();
      return _groups;
    } catch (e, st) {
      _groupsLoading = false;
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<SoshalGroup> getGroup(String groupId) => guard(() {
        final json = RustLib.instance.api.crateFfiGroupsGroupsGetGroupInfo(
          groupId: groupId,
        );
        _current = SoshalGroup.fromJson(jsonDecode(json));
        return _current!;
      }, onNotify: notifyDeferred);

  Future<List<String>> getMembers(String groupId) => guard(() {
        _members = RustLib.instance.api.crateFfiGroupsGroupsGetMembers(
          groupId: groupId,
        );
        return _members;
      }, onNotify: notifyDeferred);

  Future<bool> join(String groupId, String userPubkey, {String? password}) =>
      guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsJoin(
          groupId: groupId,
          userPubkey: userPubkey,
          password: password,
        );
      }, onNotify: notifyDeferred);

  Future<bool> setPassword(
    String groupId,
    String? newPassword,
    String actorPubkey,
  ) =>
      guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsSetPassword(
          groupId: groupId,
          newPassword: newPassword,
          actorPubkey: actorPubkey,
        );
      }, onNotify: notifyDeferred);

  Future<bool> leave(String groupId, String userPubkey) => guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsLeave(
          groupId: groupId,
          userPubkey: userPubkey,
        );
      }, onNotify: notifyDeferred);

  Future<String> postMessage(String groupId, String content,
          {String roomId = ''}) =>
      guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsPostMessage(
          groupId: groupId,
          roomId: roomId,
          content: content,
        );
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  Future<List<GroupMessage>> fetchMessages(String groupId,
          {int limit = 100, int offset = 0, String roomId = ''}) =>
      guard(() async {
        final json = RustLib.instance.api.crateFfiGroupsGroupsFetchMessages(
          groupId: groupId,
          roomId: roomId,
          limit: limit,
          offset: offset,
        );
        final parsed = await runOffThread(() => _parseGroupMessages(json));
        _messages = parsed.length > 200 ? parsed.sublist(0, 200) : parsed;
        return _messages;
      }, onNotify: notifyDeferred);

  Future<String> create(
    String groupId,
    String name,
    String description,
    String pictureUrl,
    String creatorPubkey, {
    bool isPrivate = false,
    String? password,
  }) =>
      guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsCreate(
          groupId: groupId,
          name: name,
          description: description,
          pictureUrl: pictureUrl,
          creatorPubkey: creatorPubkey,
          isPrivate: isPrivate,
          password: password,
        );
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  Future<bool> setMemberRole(
    String groupId,
    String memberPubkey,
    String role,
    String adminPubkey,
  ) =>
      guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsSetMemberRole(
          groupId: groupId,
          memberPubkey: memberPubkey,
          role: role,
          adminPubkey: adminPubkey,
        );
      }, onNotify: notifyDeferred);

  Future<bool> removeMember(
    String groupId,
    String memberPubkey,
    String adminPubkey,
  ) =>
      guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsRemoveMember(
          groupId: groupId,
          memberPubkey: memberPubkey,
          adminPubkey: adminPubkey,
        );
      }, onNotify: notifyDeferred);

  Future<List<GroupRole>> fetchRoles(String groupId) => guard(() {
        final json = RustLib.instance.api.crateFfiGroupsGroupsRolesList(
          groupId: groupId,
        );
        _roles = (jsonDecode(json) as List<dynamic>)
            .map((e) => GroupRole.fromJson(e as Map<String, dynamic>))
            .toList();
        return _roles;
      }, onNotify: notifyDeferred);

  /// Create (empty [roleId]) or update a custom role. [permissions] is the
  /// JSON array-of-keys string the backend stores.
  Future<bool> upsertRole(
    String groupId, {
    String roleId = '',
    required String name,
    required String color,
    required int position,
    required String permissions,
  }) =>
      guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsRoleUpsert(
          roleId: roleId,
          groupId: groupId,
          name: name,
          color: color,
          position: position,
          permissions: permissions,
        );
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  Future<bool> deleteRole(String roleId) => guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsRoleDelete(
          roleId: roleId,
        );
      }, onNotify: notifyDeferred);

  /// Members with their assigned role ids ({pubkey, role} rows).
  Future<List<GroupMemberWithRole>> fetchMembersWithRoles(String groupId) =>
      guard(() {
        final json = RustLib.instance.api.crateFfiGroupsGroupsMembersWithRoles(
          groupId: groupId,
        );
        _memberRoles = (jsonDecode(json) as List<dynamic>)
            .map((e) => GroupMemberWithRole.fromJson(e as Map<String, dynamic>))
            .toList();
        return _memberRoles;
      }, onNotify: notifyDeferred);

  Future<List<GroupRoom>> fetchRooms(String groupId) => guard(() {
        final json = RustLib.instance.api.crateFfiGroupsGroupsRoomsList(
          groupId: groupId,
        );
        _rooms = (jsonDecode(json) as List<dynamic>)
            .map((e) => GroupRoom.fromJson(e as Map<String, dynamic>))
            .toList();
        return _rooms;
      }, onNotify: notifyDeferred);

  Future<String> createRoom(
    String groupId,
    String name,
    String topic,
    String emoji,
    String color,
    String creator,
  ) =>
      guard(() async {
        final id = RustLib.instance.api.crateFfiGroupsGroupsRoomsCreate(
          groupId: groupId,
          name: name,
          topic: topic,
          emoji: emoji,
          color: color,
          creator: creator,
        );
        await fetchRooms(groupId);
        return id;
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  Future<bool> updateRoom(
    String roomId,
    String groupId,
    String name,
    String topic,
    String emoji,
    String color,
    String actor,
  ) =>
      guard(() async {
        final ok = RustLib.instance.api.crateFfiGroupsGroupsRoomsUpdate(
          roomId: roomId,
          groupId: groupId,
          name: name,
          topic: topic,
          emoji: emoji,
          color: color,
          actor: actor,
        );
        await fetchRooms(groupId);
        return ok;
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  Future<bool> deleteRoom(String roomId, String actor) => guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsRoomsDelete(
          roomId: roomId,
          actor: actor,
        );
      }, onNotify: notifyDeferred);

  Future<List<GroupThread>> fetchThreads(String groupId) => guard(() {
        final json = RustLib.instance.api.crateFfiGroupsGroupsThreadsList(
          groupId: groupId,
          sort: _threadSort.apiValue,
        );
        _threads = (jsonDecode(json) as List<dynamic>)
            .map((e) => GroupThread.fromJson(e as Map<String, dynamic>))
            .toList();
        return _threads;
      }, onNotify: notifyDeferred);

  /// Toggle an emoji reaction on a thread or reply; refetches threads
  /// (counts move) and, when reacting inside an open thread, its reactions.
  Future<bool> react(
          String threadId, String replyId, String emoji, String pubkey) =>
      guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsThreadsReact(
          threadId: threadId,
          replyId: replyId,
          pubkey: pubkey,
          emoji: emoji,
        );
      }, onNotify: notifyDeferred);

  /// Fetch emoji reaction summary for a thread (thread + reply targets).
  Future<List<ThreadReaction>> fetchReactions(
          String threadId, String viewerPubkey) =>
      guard(() {
        final json = RustLib.instance.api.crateFfiGroupsGroupsThreadsReactions(
          threadId: threadId,
          viewerPubkey: viewerPubkey,
        );
        final list = (jsonDecode(json) as List<dynamic>)
            .map((e) => ThreadReaction.fromJson(e as Map<String, dynamic>))
            .toList();
        _reactionsByThread[threadId] = list;
        return list;
      }, onNotify: notifyDeferred);

  Future<String> createThread(
    String groupId,
    String title,
    String body,
    String author,
  ) =>
      guard(() async {
        final id = RustLib.instance.api.crateFfiGroupsGroupsThreadsCreate(
          groupId: groupId,
          title: title,
          body: body,
          author: author,
        );
        await fetchThreads(groupId);
        return id;
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  Future<bool> deleteThread(String threadId, String actor) => guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsThreadsDelete(
          threadId: threadId,
          actor: actor,
        );
      }, onNotify: notifyDeferred);

  Future<bool> setThreadPinned(String threadId, bool pinned, String actor) =>
      guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsThreadsPin(
          threadId: threadId,
          pinned: pinned,
          actor: actor,
        );
      }, onNotify: notifyDeferred);

  Future<String> replyToThread(
    String threadId,
    String parentId,
    String content,
    String author,
  ) =>
      guard(() async {
        final id = RustLib.instance.api.crateFfiGroupsGroupsThreadsReply(
          threadId: threadId,
          parentId: parentId,
          content: content,
          author: author,
        );
        await fetchReplies(threadId);
        return id;
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  Future<List<GroupThreadReply>> fetchReplies(String threadId) => guard(() {
        final json = RustLib.instance.api.crateFfiGroupsGroupsThreadsReplies(
          threadId: threadId,
        );
        _replies = (jsonDecode(json) as List<dynamic>)
            .map((e) => GroupThreadReply.fromJson(e as Map<String, dynamic>))
            .toList();
        return _replies;
      }, onNotify: notifyDeferred);

  Future<List<GroupVoiceChannel>> fetchVoiceChannels(String groupId) =>
      guard(() {
        final json = RustLib.instance.api.crateFfiGroupsGroupsVoiceChannelsList(
          groupId: groupId,
        );
        _voiceChannels = (jsonDecode(json) as List<dynamic>)
            .map((e) => GroupVoiceChannel.fromJson(e as Map<String, dynamic>))
            .toList();
        return _voiceChannels;
      }, onNotify: notifyDeferred);

  Future<String> createVoiceChannel(
    String groupId,
    String name,
    String creator,
  ) =>
      guard(() async {
        final id = RustLib.instance.api.crateFfiGroupsGroupsVoiceChannelsCreate(
          groupId: groupId,
          name: name,
          creator: creator,
        );
        await fetchVoiceChannels(groupId);
        return id;
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  Future<bool> deleteVoiceChannel(String channelId, String actor) => guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsVoiceChannelsDelete(
          channelId: channelId,
          actor: actor,
        );
      }, onNotify: notifyDeferred);

  Future<bool> voiceJoin(String channelId, String pubkey) => guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsVoiceJoin(
          channelId: channelId,
          pubkey: pubkey,
        );
      }, onNotify: notifyDeferred);

  Future<bool> voiceLeave(String channelId, String pubkey) => guard(() {
        return RustLib.instance.api.crateFfiGroupsGroupsVoiceLeave(
          channelId: channelId,
          pubkey: pubkey,
        );
      }, onNotify: notifyDeferred);

  Future<List<GroupVoicePresence>> fetchPresence(String channelId) => guard(() {
        final json = RustLib.instance.api.crateFfiGroupsGroupsVoicePresence(
          channelId: channelId,
        );
        _presence = (jsonDecode(json) as List<dynamic>)
            .map((e) => GroupVoicePresence.fromJson(e as Map<String, dynamic>))
            .toList();
        return _presence;
      }, onNotify: notifyDeferred);

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
  final bool isPrivate;

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
    this.isPrivate = false,
  });

  factory SoshalGroup.fromJson(Map<String, dynamic> json) {
    final id = json.strOf('id');
    final isMember = json.boolOf('is_member');
    return SoshalGroup(
      id: id,
      name: json.strOf('name'),
      description: json.strOf('description'),
      picture: json.strOf('picture'),
      owner: json.strOf('owner'),
      members: json.intOf('members'),
      isMember: isMember,
      role: json.strOrNull('role') ?? (isMember ? 'member' : ''),
      createdAt: json.intOf('created_at'),
      isPrivate:
          json.boolOf('is_private') || json.strOf('access_type') == 'private',
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
      id: json.strOf('id'),
      groupId: json.strOf('group_id'),
      senderPubkey: json['sender_pubkey'] ?? json['pubkey'] ?? '',
      content: json.strOf('content'),
      createdAt: json.intOf('created_at'),
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
      id: json.strOf('id'),
      groupId: json.strOf('group_id'),
      name: json.strOf('name'),
      color: json.strOf('color'),
      position: json.intOf('position'),
      permissions: json.strOrNull('permissions') ?? '[]',
      createdAt: json.intOf('created_at'),
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
      pubkey: json.strOf('pubkey'),
      role: json.strOrNull('role') ?? 'member',
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

/// JSON → [GroupMessage] list, top-level so [runOffThread] can decode on a
/// background isolate.
List<GroupMessage> _parseGroupMessages(String json) {
  final decoded = jsonDecode(json);
  return (decoded as List<dynamic>)
      .map((e) => GroupMessage.fromJson(e as Map<String, dynamic>))
      .toList();
}

/// A themed chatroom row.
class GroupRoom {
  final String id;
  final String groupId;
  final String name;
  final String topic;
  final String emoji;
  final String color;
  final int position;
  final String createdBy;
  final int createdAt;

  GroupRoom({
    required this.id,
    required this.groupId,
    required this.name,
    required this.topic,
    required this.emoji,
    required this.color,
    required this.position,
    required this.createdBy,
    required this.createdAt,
  });

  factory GroupRoom.fromJson(Map<String, dynamic> json) {
    return GroupRoom(
      id: json.strOf('id'),
      groupId: json.strOf('group_id'),
      name: json.strOf('name'),
      topic: json.strOf('topic'),
      emoji: json.strOf('emoji'),
      color: json.strOrNull('color') ?? '#8b5cf6',
      position: json.intOf('position'),
      createdBy: json.strOf('created_by'),
      createdAt: json.intOf('created_at'),
    );
  }
}

/// Thread feed sort modes.
enum ThreadSort {
  /// Pinned first, then newest first.
  newest('newest'),

  /// Pinned first, then hot engagement (reactions + replies per hour).
  popular('popular');

  final String apiValue;

  const ThreadSort(this.apiValue);
}

/// A reddit-style thread row.
class GroupThread {
  final String id;
  final String groupId;
  final String title;
  final String body;
  final String author;
  final int createdAt;
  final bool isPinned;
  final int replyCount;
  final int reactionCount;

  GroupThread({
    required this.id,
    required this.groupId,
    required this.title,
    required this.body,
    required this.author,
    required this.createdAt,
    required this.isPinned,
    required this.replyCount,
    required this.reactionCount,
  });

  factory GroupThread.fromJson(Map<String, dynamic> json) {
    return GroupThread(
      id: json.strOf('id'),
      groupId: json.strOf('group_id'),
      title: json.strOf('title'),
      body: json.strOf('body'),
      author: json.strOf('author'),
      createdAt: json.intOf('created_at'),
      isPinned: json.boolOf('is_pinned'),
      replyCount: json.intOf('reply_count'),
      reactionCount: json.intOf('reaction_count'),
    );
  }
}

/// An emoji reaction summary row: per-target (thread or reply) counts with
/// the viewer's own reaction flag.
class ThreadReaction {
  final String threadId;
  final String replyId;
  final String emoji;
  final int count;
  final bool reacted;

  ThreadReaction({
    required this.threadId,
    required this.replyId,
    required this.emoji,
    required this.count,
    required this.reacted,
  });

  /// True when this summary targets [targetId] (thread id when [replyId]
  /// empty, reply id otherwise).
  bool matches(String targetId) =>
      replyId.isEmpty ? threadId == targetId : replyId == targetId;

  factory ThreadReaction.fromJson(Map<String, dynamic> json) {
    return ThreadReaction(
      threadId: json.strOf('thread_id'),
      replyId: json.strOf('reply_id'),
      emoji: json.strOf('emoji'),
      count: json.intOf('count'),
      reacted: json.boolOf('reacted'),
    );
  }
}

/// A thread reply row (flat or nested via [parentId]).
class GroupThreadReply {
  final String id;
  final String threadId;
  final String parentId;
  final String author;
  final String content;
  final int createdAt;

  GroupThreadReply({
    required this.id,
    required this.threadId,
    required this.parentId,
    required this.author,
    required this.content,
    required this.createdAt,
  });

  factory GroupThreadReply.fromJson(Map<String, dynamic> json) {
    return GroupThreadReply(
      id: json.strOf('id'),
      threadId: json.strOf('thread_id'),
      parentId: json.strOf('parent_id'),
      author: json.strOf('author'),
      content: json.strOf('content'),
      createdAt: json.intOf('created_at'),
    );
  }
}

/// A voice channel row.
class GroupVoiceChannel {
  final String id;
  final String groupId;
  final String name;
  final int position;
  final String createdBy;
  final int createdAt;

  GroupVoiceChannel({
    required this.id,
    required this.groupId,
    required this.name,
    required this.position,
    required this.createdBy,
    required this.createdAt,
  });

  factory GroupVoiceChannel.fromJson(Map<String, dynamic> json) {
    return GroupVoiceChannel(
      id: json.strOf('id'),
      groupId: json.strOf('group_id'),
      name: json.strOf('name'),
      position: json.intOf('position'),
      createdBy: json.strOf('created_by'),
      createdAt: json.intOf('created_at'),
    );
  }
}

/// A member present in a voice channel.
class GroupVoicePresence {
  final String channelId;
  final String pubkey;
  final int joinedAt;

  GroupVoicePresence({
    required this.channelId,
    required this.pubkey,
    required this.joinedAt,
  });

  factory GroupVoicePresence.fromJson(Map<String, dynamic> json) {
    return GroupVoicePresence(
      channelId: json.strOf('channel_id'),
      pubkey: json.strOf('pubkey'),
      joinedAt: json.intOf('joined_at'),
    );
  }
}
