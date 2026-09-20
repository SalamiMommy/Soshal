import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Scheduled Service
/// Draft posts with a future scheduled_at timestamp, persisted in the
/// posts table and broadcast later by the sync pipeline.
class ScheduledService extends ChangeNotifier
    with LastErrorMixin, ServiceGuard {
  List<ScheduledPost> _drafts = [];

  List<ScheduledPost> get drafts => _drafts;

  /// Clear draft posts on account switch so Account B never sees Account A's
  /// scheduled posts.
  void resetForAccountSwitch() {
    _drafts = [];
    clearLastError();
    notifyListeners();
  }

  /// Create a scheduled post draft. `scheduledAt` is a unix timestamp in
  /// the future. Returns the draft id.
  Future<String> create({
    required String pubkey,
    required String content,
    required int scheduledAt,
    List<String> hashtags = const [],
  }) =>
      guard(() {
        return RustLib.instance.api.crateFfiScheduledScheduledCreate(
          pubkey: pubkey,
          content: content,
          scheduledAt: scheduledAt,
          hashtags: hashtags,
        );
      }, notifyOnSuccess: false);

  /// List scheduled drafts for a pubkey, soonest first.
  Future<List<ScheduledPost>> list(String pubkey) => guard(() {
        final json = RustLib.instance.api.crateFfiScheduledScheduledList(
          pubkey: pubkey,
        );
        final decoded = jsonDecode(json);
        final items = decoded as List<dynamic>;
        _drafts = List<ScheduledPost>.generate(
          items.length,
          (i) => ScheduledPost.fromJson(items[i] as Map<String, dynamic>),
          growable: true,
        );
        return _drafts;
      });

  /// Delete a scheduled draft (soft delete).
  Future<bool> delete(String id) => guard(() {
        final ok =
            RustLib.instance.api.crateFfiScheduledScheduledDelete(id: id);
        if (ok) {
          _drafts.removeWhere((d) => d.id == id);
        }
        return ok;
      });
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
      id: json.strOf('id'),
      pubkey: json.strOf('pubkey'),
      content: json.strOf('content'),
      kind: (json['kind'] as num?)?.toInt() ?? 1,
      createdAt: json.intOf('created_at'),
      tagsJson: json.strOf('tags_json'),
      sig: json.strOrNull('sig'),
      replyTo: json.strOrNull('reply_to'),
      rootId: json.strOrNull('root_id'),
      mentionedPubkeys: _listFrom(json['mentioned_pubkeys']),
      mentionedHashtags: _listFrom(json['mentioned_hashtags']),
      subject: json.strOrNull('subject'),
      syncStatus: json.strOf('sync_status'),
      isDeleted: json.boolOf('is_deleted'),
      scheduledAt: (json['scheduled_at'] as num?)?.toInt(),
      freenetKey: json.strOrNull('freenet_key'),
      isFreenetNative: json.boolOf('is_freenet_native'),
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
      } catch (e) {
        debugPrint('scheduled tag parse: $e');
      }
    }
    return [];
  }
}
