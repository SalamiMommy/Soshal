// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Chat Random Service
/// Interest-based random pairing: kind-20030 availability announcements and
/// kind-20031/20032 request/accept events over the shared relay client.
class ChatrandomService extends ChangeNotifier with LastErrorMixin {
  List<ChatrandomPeer> _peers = [];

  List<ChatrandomPeer> get peers => _peers;

  /// Build the availability content JSON for an announcement.
  String availableContent({
    required List<String> interests,
    required String mediaType,
    required String mode,
  }) {
    return RustLib.instance.api.crateFfiChatrandomChatrandomAvailableContent(
      interests: interests,
      mediaType: mediaType,
      mode: mode,
    );
  }

  /// Sign + publish a chatrandom event for `requestType` ('request' or
  /// 'accept') addressed to `peers`. Returns the event id.
  Future<String> send({
    required String requestType,
    required List<String> peers,
    required String contentJson,
  }) async {
    try {
      final id = await RustLib.instance.api.crateFfiChatrandomChatrandomSend(
        requestType: requestType,
        peers: peers,
        contentJson: contentJson,
      );
      clearLastError();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Fetch chatrandom events (kinds 20030/20031/20032). With no `author`
  /// these are events addressed to `myPubkey`; pass `author` for a peer's
  /// own announcements.
  Future<List<ChatrandomPeer>> fetch(
    String myPubkey, {
    String? author,
    int limit = 20,
  }) async {
    try {
      final json = await RustLib.instance.api.crateFfiChatrandomChatrandomFetch(
        myPubkey: myPubkey,
        author: author,
        limit: BigInt.from(limit),
      );
      final decoded = jsonDecode(json);
      _peers = (decoded as List<dynamic>)
          .map((e) => ChatrandomPeer.fromJson(e as Map<String, dynamic>))
          .toList();
      clearLastError();
      notifyListeners();
      return _peers;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }
}

/// A chatrandom peer event (availability, request or accept).
class ChatrandomPeer {
  final String id;
  final String pubkey;
  final String content;
  final int createdAt;

  ChatrandomPeer({
    required this.id,
    required this.pubkey,
    required this.content,
    required this.createdAt,
  });

  factory ChatrandomPeer.fromJson(Map<String, dynamic> json) {
    return ChatrandomPeer(
      id: json['id'] as String? ?? '',
      pubkey: json['pubkey'] as String? ?? '',
      content: json['content'] as String? ?? '',
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}
