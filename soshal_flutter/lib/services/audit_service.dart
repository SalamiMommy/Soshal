// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Audit Service
/// Read-only access to the local SQLite security event log.
class AuditService extends ChangeNotifier {
  List<AuditRow> _rows = [];
  String? _lastError;

  List<AuditRow> get rows => _rows;
  String? get lastError => _lastError;

  /// List audit log rows, newest first. Optional actor pubkey filter.
  Future<List<AuditRow>> list({int limit = 100, String? actor}) async {
    try {
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
      _lastError = null;
      notifyListeners();
      return _rows;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }
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
      id: json['id'] as String? ?? '',
      groupId: json['group_id'] as String?,
      actorPubkey: json['actor_pubkey'] as String? ?? '',
      action: json['action'] as String? ?? '',
      targetPubkey: json['target_pubkey'] as String?,
      details: json['details'] as String?,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}
