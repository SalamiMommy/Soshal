// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Legacy Service
/// Dead-legacy RN domain storage wired into Turso for future features:
/// huddle posts, guestbook, stream chat, link previews, friend backups,
/// geohash peers, custom profile nodes, do-not-refetch markers and
/// diagnostic logs.
class LegacyService extends ChangeNotifier with LastErrorMixin {

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
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e, st) {
      setLastError(e, st);
      return [];
    }
  }

  Future<bool> huddlePostDelete(String postId) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyHuddlePostDelete(
        postId: postId,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e, st) {
      setLastError(e, st);
      return [];
    }
  }

  Future<bool> guestbookSetApproved(String entryId, bool approved) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyGuestbookSetApproved(
        entryId: entryId,
        approved: approved,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
  }

  Future<bool> guestbookDelete(String entryId) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyGuestbookDelete(
        entryId: entryId,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e, st) {
      setLastError(e, st);
      return [];
    }
  }

  Future<bool> streamChatClear(String streamId) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyStreamChatClear(
        streamId: streamId,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
  }

  Future<Map<String, dynamic>?> linkPreviewGet(String url) async {
    try {
      final json = RustLib.instance.api.crateFfiLegacyLegacyLinkPreviewGet(
        url: url,
      );
      clearLastError();
      return json == null ? null : jsonDecode(json) as Map<String, dynamic>;
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
  }

  Future<String?> friendBackupGet(String userPubkey) async {
    try {
      final data = RustLib.instance.api.crateFfiLegacyLegacyFriendBackupGet(
        userPubkey: userPubkey,
      );
      clearLastError();
      return data;
    } catch (e, st) {
      setLastError(e, st);
      return null;
    }
  }

  Future<bool> friendBackupDelete(String userPubkey) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyFriendBackupDelete(
        userPubkey: userPubkey,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
  }

  Future<List<Map<String, dynamic>>> geohashPeersByCell(String geohash) async {
    try {
      final json = RustLib.instance.api.crateFfiLegacyLegacyGeohashPeersByCell(
        geohash: geohash,
      );
      clearLastError();
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e, st) {
      setLastError(e, st);
      return [];
    }
  }

  Future<BigInt> geohashPeersPurge(int staleSecs) async {
    try {
      return RustLib.instance.api.crateFfiLegacyLegacyGeohashPeersPurge(
        staleSecs: staleSecs,
      );
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
  }

  Future<List<Map<String, dynamic>>> profileNodes(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiLegacyLegacyProfileNodes(
        userPubkey: userPubkey,
      );
      clearLastError();
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e, st) {
      setLastError(e, st);
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
    } catch (e, st) {
      setLastError(e, st);
      return BigInt.zero;
    }
  }

  Future<bool> refetchBlock(String id, String reason) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyRefetchBlock(
        id: id,
        reason: reason,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
  }

  Future<bool> refetchBlocked(String id) async {
    try {
      return RustLib.instance.api.crateFfiLegacyLegacyRefetchBlocked(id: id);
    } catch (e, st) {
      setLastError(e, st);
      return false;
    }
  }

  Future<bool> refetchUnblock(String id) async {
    try {
      final ok = RustLib.instance.api.crateFfiLegacyLegacyRefetchUnblock(
        id: id,
      );
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
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
      clearLastError();
      return (jsonDecode(json) as List).cast<Map<String, dynamic>>();
    } catch (e, st) {
      setLastError(e, st);
      return [];
    }
  }

  Future<BigInt> diagnosticPurge(int olderThanSecs) async {
    try {
      return RustLib.instance.api.crateFfiLegacyLegacyDiagnosticPurge(
        olderThanSecs: olderThanSecs,
      );
    } catch (e, st) {
      setLastError(e, st);
      return BigInt.zero;
    }
  }
}
