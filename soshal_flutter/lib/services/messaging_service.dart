// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';
import 'moderation_service.dart';
import '../utils/service_guard.dart';

/// Messaging Service
/// Handles direct messages (NIP-44), group chats, and decryption
class MessagingService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify, ServiceGuard {
  static final _hexRegex = RegExp(r'^[0-9a-f]{64}$');
  final Map<String, List<DirectMessage>> _conversations = {};
  final Map<String, DateTime> _conversationsCacheTime = {};
  final Map<String, bool> _conversationExhausted = {};
  final Map<String, int> _readWatermarks = {};
  final List<EphemeralMedia> _pendingEphemeral = [];
  late List<EphemeralMedia> _cachedPendingEphemeral =
      List.unmodifiable(_pendingEphemeral);

  /// Live-stream DMs awaiting a batched persist (see [insertLiveDm]).
  final List<DirectMessage> _pendingStores = [];
  Timer? _storeFlushTimer;
  static const _storeFlushInterval = Duration(milliseconds: 250);
  static const _storeFlushBatchSize = 8;
  static const _maxConversations = 50;

  /// Bound the in-memory conversation cache. Each conversation is itself
  /// capped at 200 messages, but the number of keys was previously unbounded,
  /// leaking every conversation ever opened for the app's lifetime.
  void _evictConversationsIfNeeded() {
    if (_conversations.length <= _maxConversations) return;
    final sorted = _conversations.keys.toList()
      ..sort((a, b) {
        final ta = _conversationsCacheTime[a] ??
            DateTime.fromMillisecondsSinceEpoch(0);
        final tb = _conversationsCacheTime[b] ??
            DateTime.fromMillisecondsSinceEpoch(0);
        return ta.compareTo(tb);
      });
    while (_conversations.length > _maxConversations) {
      final victim = sorted.removeAt(0);
      _conversations.remove(victim);
      _conversationsCacheTime.remove(victim);
      _conversationExhausted.remove(victim);
    }
  }

  Map<String, List<DirectMessage>> get conversations => _conversations;
  Map<String, int> get readWatermarks => _readWatermarks;
  List<EphemeralMedia> get pendingEphemeral => _cachedPendingEphemeral;

  @override
  void dispose() {
    _storeFlushTimer?.cancel();
    _storeFlushTimer = null;
    super.dispose();
  }

  /// Clear all account-scoped state when switching accounts so Account B never
  /// sees Account A's cached conversations, watermarks, or pending ephemeral.
  void resetForAccountSwitch() {
    _storeFlushTimer?.cancel();
    _storeFlushTimer = null;
    _pendingStores.clear();
    _conversations.clear();
    _conversationsCacheTime.clear();
    _conversationExhausted.clear();
    _readWatermarks.clear();
    _pendingEphemeral.clear();
    _cachedPendingEphemeral = List.unmodifiable(_pendingEphemeral);
    clearLastError();
    notifyListeners();
  }

  /// Insert a DM arriving from the live sync stream (already decrypted by
  /// the bridge). Conversation is keyed by the peer pubkey. Persistence is
  /// batched: a message burst on the stream triggers ONE batch FFI call
  /// instead of one call per message.
  void insertLiveDm(DirectMessage message) {
    final peer = message.sender;
    if (peer.isEmpty) return;
    final list = _conversations.putIfAbsent(peer, () => []);
    _evictConversationsIfNeeded();
    if (list.any((m) => m.id == message.id)) return;
    list.insert(0, message);
    if (list.length > 200) {
      list.removeLast();
    }
    _pendingStores.add(message);
    _storeFlushTimer ??= Timer(_storeFlushInterval, _flushPendingStores);
    if (_pendingStores.length >= _storeFlushBatchSize) {
      unawaited(_flushPendingStores());
    }
    notifyDeferred();
  }

  Future<void> _flushPendingStores() async {
    _storeFlushTimer?.cancel();
    _storeFlushTimer = null;
    if (_pendingStores.isEmpty) return;
    final batch = List<DirectMessage>.from(_pendingStores);
    _pendingStores.clear();
    await _storeDmBatch(batch);
  }

  Future<void> _storeDmBatch(List<DirectMessage> batch) async {
    try {
      final payload = jsonEncode([
        for (final m in batch)
          {
            'id': m.id,
            'sender': m.sender,
            'recipient': m.recipient,
            'content': m.content,
            'created_at': m.createdAt,
            if (m.tagsJson.isNotEmpty) 'tags': m.tagsJson,
          },
      ]);
      RustLib.instance.api.crateFfiMessagingMessagingStoreDms(
        dmsJson: payload,
      );
      clearLastError();
    } catch (e, st) {
      // Best-effort persist; drop the batch on failure rather than retry
      // storm the store from the live path.
      debugPrint('dm batch store: $e');
      setLastError(e, st);
      notifyDeferred();
    }
  }

  /// Fetch DMs with a specific contact
  Future<List<DirectMessage>> fetchDMs(String otherPubkey,
      {int limit = 100}) async {
    // Snapshot the cached conversation so a transient FFI failure below does
    // not blank a working conversation after its TTL eviction.
    final staleSnapshot = _conversations[otherPubkey];
    // End-of-history reached: repeated "load older" calls must no-op.
    if (_conversationExhausted[otherPubkey] == true &&
        _conversations.containsKey(otherPubkey)) {
      return _conversations[otherPubkey]!;
    }
    try {
      final cached = _conversationsCacheTime[otherPubkey];
      if (cached != null && DateTime.now().difference(cached).inSeconds > 30) {
        _conversations.remove(otherPubkey);
        _conversationsCacheTime.remove(otherPubkey);
      }
      final existing = _conversations[otherPubkey];
      if (existing != null &&
          (existing.length >= limit ||
              _conversationExhausted[otherPubkey] == true ||
              !_conversationsCacheTime.containsKey(otherPubkey))) {
        return existing;
      }

      await _flushPendingStores();
      final json = RustLib.instance.api.crateFfiMessagingMessagingFetchDms(
        withPubkey: otherPubkey,
        limit: limit,
      );
      final messages = await runOffThread(() => _parseDmsJson(json));

      if (messages.length > 200) {
        _conversations[otherPubkey] = messages.sublist(messages.length - 200);
      } else {
        _conversations[otherPubkey] = messages;
      }
      _conversationsCacheTime[otherPubkey] = DateTime.now();
      _conversationExhausted[otherPubkey] = messages.length < limit;
      _evictConversationsIfNeeded();

      clearLastError();
      notifyDeferred();
      return _conversations[otherPubkey]!;
    } catch (e, st) {
      // Restore the TTL-evicted cache on failure so a working conversation is
      // not lost to a transient store round-trip error.
      if (staleSnapshot != null && !_conversations.containsKey(otherPubkey)) {
        _conversations[otherPubkey] = staleSnapshot;
      }
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Send a direct message (NIP-44 v2)
  Future<String> sendDM(
    String content,
    String recipientPubkey,
    String senderPubkey,
  ) async {
    try {
      final rawResult =
          await RustLib.instance.api.crateFfiMessagingMessagingSendDm(
        content: content,
        recipientPubkey: recipientPubkey,
      );
      var eventId = rawResult;
      try {
        if (rawResult.trim().startsWith('{')) {
          final decoded = jsonDecode(rawResult) as Map<String, dynamic>;
          if (decoded['id'] is String) {
            eventId = decoded['id'] as String;
          }
        }
      } catch (e) {
        debugPrint('dm send decode: $e');
      }

      // Add to local conversation
      final message = DirectMessage(
        id: eventId,
        sender: senderPubkey,
        recipient: recipientPubkey,
        content: content,
        createdAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
        decrypted: true,
        isOwn: true,
      );

      if (!_conversations.containsKey(recipientPubkey)) {
        _conversations[recipientPubkey] = [];
      }
      _evictConversationsIfNeeded();
      final convo = _conversations[recipientPubkey]!;
      convo.insert(0, message);
      if (convo.length > 200) {
        convo.removeLast();
      }

      clearLastError();
      notifyDeferred();
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Resolve a recipient input (npub or hex) to a hex pubkey.
  /// Throws if the input is neither a valid npub nor 64-char hex pubkey.
  String resolvePubkey(String input) {
    final trimmed = input.trim();
    if (_hexRegex.hasMatch(trimmed.toLowerCase())) {
      return trimmed.toLowerCase();
    }
    if (trimmed.startsWith('npub1')) {
      try {
        return RustLib.instance.api.crateFfiAuthAuthNpubDecode(npub: trimmed);
      } catch (e, st) {
        setLastError(e, st);
        notifyDeferred();
        throw Exception('Invalid npub: $trimmed');
      }
    }
    throw Exception('Invalid recipient: expected npub or 64-char hex pubkey');
  }

  /// Send a group DM (kind-1059). Group id is derived deterministically from
  /// the sorted participant list; the backend resolves the conversation.
  Future<String> sendGroupDm({
    required String content,
    required List<String> participantPubkeys,
    String senderPubkey = '',
  }) =>
      guard(() {
        final sorted = [...participantPubkeys]..sort();
        final eventId =
            RustLib.instance.api.crateFfiMessagingMessagingSendGroupDm(
          content: content,
          groupId: sorted.join(','),
          participantPubkeysJson: jsonEncode(sorted),
        );

        // Cache sent message locally, keyed by groupId.
        final groupId = sorted.join(',');
        final message = DirectMessage(
          id: eventId,
          sender: senderPubkey,
          recipient: groupId,
          content: content,
          createdAt: DateTime.now().millisecondsSinceEpoch ~/ 1000,
          decrypted: true,
          isOwn: true,
        );
        _conversations.putIfAbsent(groupId, () => []).add(message);

        return eventId;
      }, onNotify: notifyDeferred);

  /// Decrypt a direct message
  Future<String> decryptDM(
    String encryptedContent,
    String senderPubkey,
    String recipientSk,
  ) =>
      guard(() {
        return RustLib.instance.api.crateFfiSignerSignerNip44Decrypt(
          payload: encryptedContent,
          senderPubkey: senderPubkey,
        );
      }, onNotify: notifyDeferred, clearOnSuccess: false);

  /// Fetch all conversation partner pubkeys for the active account.
  Future<List<String>> fetchConversations(String pubkey) => guard(() {
        return RustLib.instance.api
            .crateFfiMessagingMessagingFetchConversations(
          pubkey: pubkey,
        );
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// Replace a message's content in the local conversation cache. After a
  /// decrypt, the plaintext is also persisted to the DM store.
  void updateMessageContent(
    String? otherPubkey,
    DirectMessage message,
    String decrypted,
  ) {
    if (otherPubkey == null) return;
    final list = _conversations[otherPubkey];
    if (list == null) return;
    final index = list.indexWhere((m) => identical(m, message));
    if (index >= 0) {
      final updated = message.deepCopy(content: decrypted, decrypted: true);
      list[index] = updated;
      unawaited(storeDm(updated));
      notifyDeferred();
    }
  }

  /// Persist a DM row into the local store (sync FFI). Best-effort: returns
  /// false on failure instead of throwing, so the live ingest path never
  /// breaks on a store hiccup.
  Future<bool> storeDm(DirectMessage message) async {
    try {
      final ok = RustLib.instance.api.crateFfiMessagingMessagingStoreDm(
        id: message.id,
        sender: message.sender,
        recipient: message.recipient,
        content: message.content,
        createdAt: BigInt.from(message.createdAt),
        tagsJson:
            message.tagsJson.isEmpty ? jsonEncode(const []) : message.tagsJson,
      );
      clearLastError();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return false;
    }
  }

  /// Fetch a single ephemeral (burn DM) row by id, fresh from the store.
  /// Returns null when the row no longer exists.
  Future<EphemeralMedia?> ephemeralById(String id) => guard(() {
        final json = RustLib.instance.api.crateFfiEphemeralEphemeralGet(id: id);
        if (json.isEmpty) return null;
        return EphemeralMedia.fromJson(
          jsonDecode(json) as Map<String, dynamic>,
        );
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// Mark conversation as read
  Future<void> markAsRead(String otherPubkey) async {
    _readWatermarks[otherPubkey] =
        DateTime.now().millisecondsSinceEpoch ~/ 1000;
    notifyDeferred();
  }

  /// Register a burn DM (disappearing media) against a sent message.
  /// Sync FFI; returns the ephemeral row id.
  Future<String> saveEphemeral({
    required String messageId,
    required String conversationId,
    required String conversationType,
    required String mediaUrl,
    required String mediaType,
    required String senderPubkey,
    required String recipientPubkey,
    required int maxViews,
    int expiresAt = 0,
  }) =>
      guard(() {
        return RustLib.instance.api.crateFfiEphemeralEphemeralSave(
          messageId: messageId,
          conversationId: conversationId,
          conversationType: conversationType,
          mediaUrl: mediaUrl,
          mediaType: mediaType,
          senderPubkey: senderPubkey,
          recipientPubkey: recipientPubkey,
          maxViews: maxViews,
          expiresAt: expiresAt,
        );
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// Fetch pending disappearing media addressed to [pubkey] (the active
  /// account). Sync FFI returning a JSON array.
  Future<List<EphemeralMedia>> fetchPendingEphemeral(String pubkey) =>
      guard(() {
        final json = RustLib.instance.api
            .crateFfiEphemeralEphemeralListPending(pubkey: pubkey);
        final list = jsonDecode(json) as List<dynamic>;
        _pendingEphemeral
          ..clear()
          ..addAll(list
              .map((e) => EphemeralMedia.fromJson(e as Map<String, dynamic>)));
        _cachedPendingEphemeral = List.unmodifiable(_pendingEphemeral);
        return pendingEphemeral;
      }, onNotify: notifyDeferred);

  /// Consume one view of a burn DM (increments view count; row expires at
  /// max_views). Returns the fresh row.
  Future<EphemeralMedia> viewEphemeral(String id) async {
    try {
      final json = RustLib.instance.api.crateFfiEphemeralEphemeralView(id: id);
      final media = EphemeralMedia.fromJson(jsonDecode(json));
      final index = _pendingEphemeral.indexWhere((m) => m.id == id);
      if (index >= 0) {
        _pendingEphemeral[index] = media;
        _cachedPendingEphemeral = List.unmodifiable(_pendingEphemeral);
        notifyDeferred();
      }
      clearLastError();
      return media;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Delete a burn DM row (also removes it from the pending list).
  Future<bool> deleteEphemeral(String id) => guard(() {
        final ok =
            RustLib.instance.api.crateFfiEphemeralEphemeralDelete(id: id);
        _pendingEphemeral.removeWhere((m) => m.id == id);
        _cachedPendingEphemeral = List.unmodifiable(_pendingEphemeral);
        return ok;
      }, onNotify: notifyDeferred);
}

/// Identity Service
/// Handles profiles, WoT status, and NIP-05 verification
class IdentityService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify {
  final Map<String, ProfileInfo> _profiles = {};
  late final ModerationService _moderation = ModerationService();

  Map<String, ProfileInfo> get profiles => _profiles;

  /// Merge a freshly-fetched profile over the cached one, keeping cached
  /// display fields when the incoming value is empty/zero (sparse parse must
  /// not blank the UI profile until the next complete fetch).
  ProfileInfo _mergeProfile(ProfileInfo incoming, ProfileInfo? cached) {
    if (cached == null) return incoming;
    return ProfileInfo(
      pubkey: incoming.pubkey,
      name: incoming.name.isNotEmpty ? incoming.name : cached.name,
      displayName: incoming.displayName.isNotEmpty
          ? incoming.displayName
          : cached.displayName,
      picture: incoming.picture.isNotEmpty ? incoming.picture : cached.picture,
      banner: incoming.banner.isNotEmpty ? incoming.banner : cached.banner,
      about: incoming.about.isNotEmpty ? incoming.about : cached.about,
      nip05: incoming.nip05,
      nip05Valid: incoming.nip05Valid,
      createdAt:
          incoming.createdAt != 0 ? incoming.createdAt : cached.createdAt,
      followers:
          incoming.followers != 0 ? incoming.followers : cached.followers,
      following:
          incoming.following != 0 ? incoming.following : cached.following,
      isFollowing: incoming.isFollowing,
      wotStatus: incoming.wotStatus,
    );
  }

  /// Clear the cached profile map on account switch so Account B doesn't see
  /// Account A's cached profiles.
  void resetForAccountSwitch() {
    _profiles.clear();
    clearLastError();
    notifyListeners();
  }

  /// Get user profile
  Future<ProfileInfo> getProfile(String pubkey, {bool refresh = false}) async {
    try {
      if (!refresh && _profiles.containsKey(pubkey)) {
        return _profiles[pubkey]!;
      }

      final json = RustLib.instance.api
          .crateFfiIdentityIdentityGetProfile(pubkey: pubkey);
      final profile =
          ProfileInfo.fromJson(jsonDecode(json) as Map<String, dynamic>);
      _profiles[pubkey] =
          refresh ? _mergeProfile(profile, _profiles[pubkey]) : profile;

      clearLastError();
      notifyDeferred();
      return profile;
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }

  /// Get current user's profile
  Future<ProfileInfo> getSelfProfile(String pubkey) async {
    try {
      final json = RustLib.instance.api
          .crateFfiIdentityIdentityGetProfile(pubkey: pubkey);
      final profile =
          ProfileInfo.fromJson(jsonDecode(json) as Map<String, dynamic>);
      _profiles[pubkey] = _mergeProfile(profile, _profiles[pubkey]);
      clearLastError();
      notifyDeferred();
      return profile;
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }

  /// Search users by name/pubkey
  Future<List<ProfileInfo>> searchUsers(String query, {int limit = 50}) async {
    try {
      final json = RustLib.instance.api.crateFfiIdentityIdentitySearchUsers(
        query: query,
        limit: limit,
      );
      return await runOffThread(() => _parseProfiles(json));
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }

  /// Update user profile
  Future<String> updateProfile(
    String pubkey,
    String name,
    String displayName,
    String picture,
    String banner,
    String about,
    String nip05,
  ) async {
    try {
      final eventId =
          RustLib.instance.api.crateFfiIdentityIdentityUpdateProfile(
        pubkey: pubkey,
        name: name,
        displayName: displayName,
        picture: picture,
        banner: banner,
        about: about,
        nip05: nip05,
      );

      try {
        RustLib.instance.api.crateFfiSearchSearchIndexProfile(
          pubkey: pubkey,
          name: displayName,
          about: about,
        );
      } catch (e) {
        debugPrint('index profile: $e');
      }

      // Update local cache
      _profiles[pubkey] = ProfileInfo(
        pubkey: pubkey,
        name: name,
        displayName: displayName,
        picture: picture,
        banner: banner,
        about: about,
        nip05: nip05,
        nip05Valid: false,
        createdAt: 0,
        followers: 0,
        following: 0,
        isFollowing: false,
        wotStatus: 'unknown',
      );

      clearLastError();
      notifyDeferred();
      return eventId;
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }

  /// Publish NIP-09 deletion tombstones for this account's identity events
  /// (kind 0 metadata, kind 3 contacts, kind 10002 relay list). Queues to
  /// the outbox when offline.
  Future<String> deleteProfile(String pubkey) async {
    try {
      final eventId =
          await RustLib.instance.api.crateFfiIdentityIdentityDeleteProfile(
        pubkey: pubkey,
      );
      clearLastError();
      notifyDeferred();
      return eventId;
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }

  /// Publish custom profile to relays (Nostr kind 30085)
  Future<String> publishCustomProfile(String pubkey, String profileJson) async {
    try {
      final eventId =
          RustLib.instance.api.crateFfiIdentityIdentityPublishCustomProfile(
        pubkey: pubkey,
        profileJson: profileJson,
      );
      clearLastError();
      notifyDeferred();
      return eventId;
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }

  /// Verify NIP-05 identifier
  Future<bool> verifyNip05(String nip05) async {
    try {
      return await RustLib.instance.api
          .crateFfiIdentityIdentityVerifyNip05(nip05: nip05);
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }

  /// Get Web of Trust status
  Future<int> getTrustScore(String source, String target) async {
    try {
      final score = RustLib.instance.api.crateFfiIdentityIdentityGetTrustScore(
        sourcePubkey: source,
        targetPubkey: target,
      );
      return (score * 100).round();
    } catch (e) {
      setLastError(e);
      return 0;
    }
  }

  /// Check whether one account has blocked another.
  Future<bool> isBlocked(String checkerPubkey, String targetPubkey) async {
    try {
      return RustLib.instance.api.crateFfiIdentityIdentityIsBlocked(
        checkerPubkey: checkerPubkey,
        targetPubkey: targetPubkey,
      );
    } catch (e) {
      setLastError(e);
      return false;
    }
  }

  /// Block a user (blocker must be the active account).
  Future<bool> blockUser(String blockerPubkey, String targetPubkey) {
    return _moderation.block(blockerPubkey, targetPubkey);
  }

  /// Unblock a user.
  Future<bool> unblockUser(String blockerPubkey, String targetPubkey) {
    return _moderation.unblock(blockerPubkey, targetPubkey);
  }

  /// List users blocked by the given account.
  Future<List<String>> getBlockedUsers(String pubkey) async {
    try {
      final list = RustLib.instance.api
          .crateFfiIdentityIdentityGetBlockedUsers(pubkey: pubkey);
      clearLastError();
      return list;
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }

  Future<String> getWotStatus(String targetPubkey, String viewerPubkey) async {
    try {
      return RustLib.instance.api.crateFfiIdentityIdentityGetWotStatus(
        targetPubkey: targetPubkey,
        viewerPubkey: viewerPubkey,
      );
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }

  /// Follow a user
  Future<String> followUser(String targetPubkey, String myPubkey) async {
    try {
      return RustLib.instance.api.crateFfiIdentityIdentityFollowUser(
        pubkey: targetPubkey,
      );
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }

  /// Unfollow a user (depends on full contact-list republish; Rust returns
  /// a guidance error until the caller republishes kind 3)
  Future<String> unfollowUser(String targetPubkey, String myPubkey) async {
    try {
      final ok = RustLib.instance.api.crateFfiIdentityIdentityUnfollowUser(
        pubkey: targetPubkey,
      );
      return ok ? 'unfollowed' : 'not followed';
    } catch (e) {
      setLastError(e);
      notifyDeferred();
      rethrow;
    }
  }
}

/// Direct message (NIP-44 v2) as served by the bridge.
class DirectMessage {
  final String id;
  final String sender;
  final String recipient;
  final String content;
  final int createdAt;
  final bool decrypted;
  final bool isOwn;
  final String tagsJson;

  DirectMessage({
    required this.id,
    required this.sender,
    this.recipient = '',
    required this.content,
    required this.createdAt,
    required this.decrypted,
    required this.isOwn,
    this.tagsJson = '',
  });

  factory DirectMessage.fromJson(Map<String, dynamic> json) {
    return DirectMessage(
      id: json.strOf('id'),
      sender: json.strOf('sender'),
      recipient: json.strOf('recipient'),
      content: json.strOf('content'),
      createdAt: json.intOf('created_at'),
      decrypted: json.boolOf('decrypted'),
      isOwn: json.boolOf('is_own'),
      tagsJson: json.strOf('tags'),
    );
  }

  DirectMessage deepCopy({String? content, bool? decrypted}) {
    return DirectMessage(
      id: id,
      sender: sender,
      recipient: recipient,
      content: content ?? this.content,
      createdAt: createdAt,
      decrypted: decrypted ?? this.decrypted,
      isOwn: isOwn,
      tagsJson: tagsJson,
    );
  }
}

/// JSON → [DirectMessage] list, top-level so [compute] can run it on a
/// background isolate (fetchDMs decodes off the UI thread).
List<DirectMessage> _parseDmsJson(String json) {
  final list = jsonDecode(json) as List<dynamic>;
  return list
      .map((e) => DirectMessage.fromJson(e as Map<String, dynamic>))
      .toList();
}

/// User profile info as served by the identity module.
class ProfileInfo {
  final String pubkey;
  final String name;
  final String displayName;
  final String picture;
  final String banner;
  final String about;
  final String nip05;
  final bool nip05Valid;
  final int createdAt;
  final int followers;
  final int following;
  final bool isFollowing;
  final String wotStatus;

  ProfileInfo({
    required this.pubkey,
    required this.name,
    required this.displayName,
    required this.picture,
    required this.banner,
    required this.about,
    required this.nip05,
    required this.nip05Valid,
    required this.createdAt,
    required this.followers,
    required this.following,
    required this.isFollowing,
    required this.wotStatus,
  });

  factory ProfileInfo.fromJson(Map<String, dynamic> json) {
    return ProfileInfo(
      pubkey: json.strOf('pubkey'),
      name: json.strOf('name'),
      displayName: json.strOf('display_name'),
      picture: json.strOf('picture'),
      banner: json.strOf('banner'),
      about: json.strOf('about'),
      nip05: json.strOf('nip05'),
      nip05Valid: json.boolOf('nip05_valid'),
      createdAt: json.intOf('created_at'),
      followers: json.intOf('followers'),
      following: json.intOf('following'),
      isFollowing: json.boolOf('is_following'),
      wotStatus: json.strOrNull('wot_status') ?? 'unknown',
    );
  }
}

/// Disappearing media (burn DM) row as served by the ephemeral module.
class EphemeralMedia {
  final String id;
  final String messageId;
  final String conversationId;
  final String conversationType;
  final String mediaUrl;
  final String mediaType;
  final String senderPubkey;
  final String recipientPubkey;
  final int maxViews;
  final int currentViews;
  final String state;
  final int expiresAt;
  final int createdAt;
  final int viewedAt;

  EphemeralMedia({
    required this.id,
    required this.messageId,
    required this.conversationId,
    required this.conversationType,
    required this.mediaUrl,
    required this.mediaType,
    required this.senderPubkey,
    required this.recipientPubkey,
    required this.maxViews,
    required this.currentViews,
    required this.state,
    required this.expiresAt,
    required this.createdAt,
    required this.viewedAt,
  });

  factory EphemeralMedia.fromJson(Map<String, dynamic> json) {
    return EphemeralMedia(
      id: json.strOf('id'),
      messageId: json.strOf('message_id'),
      conversationId: json.strOf('conversation_id'),
      conversationType: json.strOf('conversation_type'),
      mediaUrl: json.strOf('media_url'),
      mediaType: json.strOrNull('media_type') ?? 'image',
      senderPubkey: json.strOf('sender_pubkey'),
      recipientPubkey: json.strOf('recipient_pubkey'),
      maxViews: (json['max_views'] as num?)?.toInt() ?? 1,
      currentViews: json.intOf('current_views'),
      state: json.strOf('state'),
      expiresAt: json.intOf('expires_at'),
      createdAt: json.intOf('created_at'),
      viewedAt: json.intOf('viewed_at'),
    );
  }
}

/// JSON → [ProfileInfo] list, top-level so [runOffThread] can decode on a
/// background isolate.
List<ProfileInfo> _parseProfiles(String json) {
  final list = jsonDecode(json) as List<dynamic>;
  return list
      .map((e) => ProfileInfo.fromJson(e as Map<String, dynamic>))
      .toList();
}
