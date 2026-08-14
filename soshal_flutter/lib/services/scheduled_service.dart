// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Scheduled Service
/// Draft posts with a future scheduled_at timestamp, persisted in the
/// posts table and broadcast later by the sync pipeline.
class ScheduledService extends ChangeNotifier {
  List<ScheduledPost> _drafts = [];
  String? _lastError;

  List<ScheduledPost> get drafts => _drafts;
  String? get lastError => _lastError;

  /// Create a scheduled post draft. `scheduledAt` is a unix timestamp in
  /// the future. Returns the draft id.
  Future<String> create({
    required String pubkey,
    required String content,
    required int scheduledAt,
    List<String> hashtags = const [],
  }) async {
    try {
      final id = RustLib.instance.api.crateFfiScheduledScheduledCreate(
        pubkey: pubkey,
        content: content,
        scheduledAt: scheduledAt,
        hashtags: hashtags,
      );
      _lastError = null;
      return id;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// List scheduled drafts for a pubkey, soonest first.
  Future<List<ScheduledPost>> list(String pubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiScheduledScheduledList(
        pubkey: pubkey,
      );
      final decoded = jsonDecode(json);
      _drafts = (decoded as List<dynamic>)
          .map((e) => ScheduledPost.fromJson(e as Map<String, dynamic>))
          .toList();
      _lastError = null;
      notifyListeners();
      return _drafts;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Delete a scheduled draft (soft delete).
  Future<bool> delete(String id) async {
    try {
      final ok = RustLib.instance.api.crateFfiScheduledScheduledDelete(id: id);
      _lastError = null;
      return ok;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }
}

/// A scheduled post draft row from the posts table.
class ScheduledPost {
  final String id;
  final String pubkey;
  final String content;
  final int kind;
  final int createdAt;
  final String tagsJson;
  final String? sig;
  final String? replyTo;
  final String? rootId;
  final List<String> mentionedPubkeys;
  final List<String> mentionedHashtags;
  final String? subject;
  final String syncStatus;
  final bool isDeleted;
  final int? scheduledAt;
  final String? freenetKey;
  final bool isFreenetNative;

  ScheduledPost({
    required this.id,
    required this.pubkey,
    required this.content,
    required this.kind,
    required this.createdAt,
    required this.tagsJson,
    this.sig,
    this.replyTo,
    this.rootId,
    required this.mentionedPubkeys,
    required this.mentionedHashtags,
    this.subject,
    required this.syncStatus,
    required this.isDeleted,
    this.scheduledAt,
    this.freenetKey,
    required this.isFreenetNative,
  });

  factory ScheduledPost.fromJson(Map<String, dynamic> json) {
    return ScheduledPost(
      id: json['id'] as String? ?? '',
      pubkey: json['pubkey'] as String? ?? '',
      content: json['content'] as String? ?? '',
      kind: (json['kind'] as num?)?.toInt() ?? 1,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
      tagsJson: json['tags_json'] as String? ?? '',
      sig: json['sig'] as String?,
      replyTo: json['reply_to'] as String?,
      rootId: json['root_id'] as String?,
      mentionedPubkeys: _listFrom(json['mentioned_pubkeys']),
      mentionedHashtags: _listFrom(json['mentioned_hashtags']),
      subject: json['subject'] as String?,
      syncStatus: json['sync_status'] as String? ?? '',
      isDeleted: json['is_deleted'] as bool? ?? false,
      scheduledAt: (json['scheduled_at'] as num?)?.toInt(),
      freenetKey: json['freenet_key'] as String?,
      isFreenetNative: json['is_freenet_native'] as bool? ?? false,
    );
  }

  static List<String> _listFrom(dynamic value) {
    if (value is List) {
      return value.map((e) => e.toString()).toList();
    }
    if (value is String && value.isNotEmpty) {
      try {
        final decoded = jsonDecode(value);
        if (decoded is List) {
          return decoded.map((e) => e.toString()).toList();
        }
      } catch (_) {}
    }
    return [];
  }
}
