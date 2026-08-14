// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Legacy Service
/// Dead-legacy RN domain storage wired into Turso for future features:
/// huddle posts, guestbook, stream chat, link previews, friend backups,
/// geohash peers, custom profile nodes, do-not-refetch markers and
/// diagnostic logs.
class LegacyService extends ChangeNotifier {
  String? _lastError;

  String? get lastError => _lastError;

  Future<bool> huddlePostStore({
    required String huddleId,
    required String pubkey,
    required String content,
    int expiresInSecs = 3600,
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyHuddlePostStore(
        huddleId: huddleId,
        pubkey: pubkey,
        content: content,
        expiresInSecs: expiresInSecs,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<List<Map<String, dynamic>>> huddlePosts(
    String huddleId, {
    int limit = 50,
  }) async {
    try {
      final json = RustLib.instance.api.crateFfiLegacyLegacyHuddlePosts(
        huddleId: huddleId,
        limit: limit,
      );
      _lastError = null;
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e) {
      _lastError = e.toString();
      return [];
    }
  }

  Future<bool> huddlePostDelete(String postId) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyHuddlePostDelete(
        postId: postId,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<bool> guestbookAdd({
    required String profilePubkey,
    required String senderPubkey,
    String? senderName,
    required String content,
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyGuestbookAdd(
        profilePubkey: profilePubkey,
        senderPubkey: senderPubkey,
        senderName: senderName,
        content: content,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<List<Map<String, dynamic>>> guestbookEntries(
    String profilePubkey, {
    int limit = 50,
    bool onlyApproved = true,
  }) async {
    try {
      final json = RustLib.instance.api.crateFfiLegacyLegacyGuestbookEntries(
        profilePubkey: profilePubkey,
        limit: limit,
        onlyApproved: onlyApproved,
      );
      _lastError = null;
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e) {
      _lastError = e.toString();
      return [];
    }
  }

  Future<bool> guestbookSetApproved(String entryId, bool approved) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyGuestbookSetApproved(
        entryId: entryId,
        approved: approved,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<bool> guestbookDelete(String entryId) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyGuestbookDelete(
        entryId: entryId,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<bool> streamChatSend({
    required String streamId,
    required String pubkey,
    required String text,
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyStreamChatSend(
        streamId: streamId,
        pubkey: pubkey,
        text: text,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<List<Map<String, dynamic>>> streamChatMessages(
    String streamId, {
    int limit = 100,
  }) async {
    try {
      final json = RustLib.instance.api.crateFfiLegacyLegacyStreamChatMessages(
        streamId: streamId,
        limit: limit,
      );
      _lastError = null;
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e) {
      _lastError = e.toString();
      return [];
    }
  }

  Future<bool> streamChatClear(String streamId) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyStreamChatClear(
        streamId: streamId,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<bool> linkPreviewStore({
    required String url,
    required String title,
    String description = '',
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyLinkPreviewStore(
        url: url,
        title: title,
        description: description,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<Map<String, dynamic>?> linkPreviewGet(String url) async {
    try {
      final json = RustLib.instance.api.crateFfiLegacyLegacyLinkPreviewGet(
        url: url,
      );
      _lastError = null;
      return json == null ? null : jsonDecode(json) as Map<String, dynamic>;
    } catch (e) {
      _lastError = e.toString();
      return null;
    }
  }

  Future<bool> friendBackupStore(
    String userPubkey,
    String encryptedData,
  ) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyFriendBackupStore(
        userPubkey: userPubkey,
        encryptedData: encryptedData,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<String?> friendBackupGet(String userPubkey) async {
    try {
      final data = RustLib.instance.api.crateFfiLegacyLegacyFriendBackupGet(
        userPubkey: userPubkey,
      );
      _lastError = null;
      return data;
    } catch (e) {
      _lastError = e.toString();
      return null;
    }
  }

  Future<bool> friendBackupDelete(String userPubkey) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyFriendBackupDelete(
        userPubkey: userPubkey,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<bool> geohashPeerUpsert({
    required String pubkey,
    required String geohash,
    String purpose = 'both',
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyGeohashPeerUpsert(
        pubkey: pubkey,
        geohash: geohash,
        purpose: purpose,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<List<Map<String, dynamic>>> geohashPeersByCell(String geohash) async {
    try {
      final json = RustLib.instance.api.crateFfiLegacyLegacyGeohashPeersByCell(
        geohash: geohash,
      );
      _lastError = null;
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e) {
      _lastError = e.toString();
      return [];
    }
  }

  Future<BigInt> geohashPeersPurge(int staleSecs) async {
    try {
      return RustLib.instance.api.crateFfiLegacyLegacyGeohashPeersPurge(
        staleSecs: staleSecs,
      );
    } catch (e) {
      _lastError = e.toString();
      return BigInt.zero;
    }
  }

  Future<bool> profileNodeUpsert({
    required String id,
    required String userPubkey,
    required String nodeType,
    String styles = '{}',
    String properties = '{}',
    String layout = '{"row":0,"col":0,"sort":0}',
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyProfileNodeUpsert(
        id: id,
        userPubkey: userPubkey,
        nodeType: nodeType,
        styles: styles,
        properties: properties,
        layout: layout,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<List<Map<String, dynamic>>> profileNodes(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiLegacyLegacyProfileNodes(
        userPubkey: userPubkey,
      );
      _lastError = null;
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e) {
      _lastError = e.toString();
      return [];
    }
  }

  Future<BigInt> profileNodeDelete(
    String id,
    String userPubkey,
  ) async {
    try {
      return RustLib.instance.api.crateFfiLegacyLegacyProfileNodeDelete(
        id: id,
        userPubkey: userPubkey,
      );
    } catch (e) {
      _lastError = e.toString();
      return BigInt.zero;
    }
  }

  Future<bool> refetchBlock(String id, String reason) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyRefetchBlock(
        id: id,
        reason: reason,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<bool> refetchBlocked(String id) async {
    try {
      return RustLib.instance.api.crateFfiLegacyLegacyRefetchBlocked(id: id);
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<bool> refetchUnblock(String id) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyRefetchUnblock(
        id: id,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<bool> diagnosticLog({
    required String level,
    required String service,
    required String method,
    required String message,
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyDiagnosticLog(
        level: level,
        service: service,
        method: method,
        message: message,
      );
      _lastError = null;
      return ok;
    } catch (e) {
      _lastError = e.toString();
      return false;
    }
  }

  Future<List<Map<String, dynamic>>> diagnosticLogs({
    int limit = 100,
    String? level,
  }) async {
    try {
      final json = RustLib.instance.api.crateFfiLegacyLegacyDiagnosticLogs(
        limit: limit,
        level: level,
      );
      _lastError = null;
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e) {
      _lastError = e.toString();
      return [];
    }
  }

  Future<BigInt> diagnosticPurge(int olderThanSecs) async {
    try {
      return RustLib.instance.api.crateFfiLegacyLegacyDiagnosticPurge(
        olderThanSecs: olderThanSecs,
      );
    } catch (e) {
      _lastError = e.toString();
      return BigInt.zero;
    }
  }
}
