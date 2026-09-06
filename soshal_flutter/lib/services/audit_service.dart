import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Audit Service
/// Read-only access to the local SQLite security event log.
class AuditService extends ChangeNotifier with LastErrorMixin, ServiceGuard {
  List<AuditRow> _rows = [];

  List<AuditRow> get rows => _rows;

  /// List audit log rows, newest first. Optional actor pubkey filter.
  Future<List<AuditRow>> list({int limit = 100, String? actor}) =>
      guard(() {
        final json = RustLib.instance.api.crateFfiAuditAuditList(
          limit: limit,
          actorPubkey: actor,
        );
        final decoded = jsonDecode(json);
        _rows = decoded is List
            ? decoded
                .map((e) => AuditRow.fromJson(e as Map<String, dynamic>))
                .toList()
            : <AuditRow>[];
        return _rows;
      });
}

/// A single audit log entry.
class AuditRow {
  final String id;
  final String? groupId;
  final String actorPubkey;
  final String action;
  final String? targetPubkey;
  final String? details;
  final int createdAt;

  AuditRow({
    required this.id,
    this.groupId,
    required this.actorPubkey,
    required this.action,
    this.targetPubkey,
    this.details,
    required this.createdAt,
  });

  factory AuditRow.fromJson(Map<String, dynamic> json) {
    return AuditRow(
      id: json.strOf('id'),
      groupId: json.strOrNull('group_id'),
      actorPubkey: json.strOf('actor_pubkey'),
      action: json.strOf('action'),
      targetPubkey: json.strOrNull('target_pubkey'),
      details: json.strOrNull('details'),
      createdAt: json.intOf('created_at'),
    );
  }
}
