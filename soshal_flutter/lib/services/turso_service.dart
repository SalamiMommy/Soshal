// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Turso Sync Service
/// Manages connection settings, replication credentials, and real-time
/// sync status for the Turso (libSQL) database engine.
class TursoService extends ChangeNotifier with LastErrorMixin {
  bool _isConfigured = false;
  String _status = 'idle';
  int? _lastSyncedAt;
  String _url = '';
  bool _isSyncing = false;

  bool get isConfigured => _isConfigured;
  String get status => _status;
  int? get lastSyncedAt => _lastSyncedAt;
  String get url => _url;
  bool get isSyncing => _isSyncing;

  /// Configure Turso database URL and auth token.
  Future<bool> configure(
      {required String url, required String authToken}) async {
    try {
      lastErrorValue = null;
      notifyListeners();

      RustLib.instance.api.crateFfiTursoDbTursoConfigure(
        url: url,
        authToken: authToken,
      );

      _url = url;
      _isConfigured = true;
      notifyListeners();
      await checkStatus();
      return true;
    } catch (e, st) {
      setLastError(e, st);
      _isConfigured = false;
      notifyListeners();
      return false;
    }
  }

  /// Trigger manual database replication sync with Turso Cloud.
  Future<bool> syncNow() async {
    if (_isSyncing) return false;
    _isSyncing = true;
    _status = 'syncing';
    lastErrorValue = null;
    notifyListeners();

    try {
      RustLib.instance.api.crateFfiTursoDbTursoSync();
      _isSyncing = false;
      _status = 'synced';
      _lastSyncedAt = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      notifyListeners();
      return true;
    } catch (e, st) {
      _isSyncing = false;
      _status = 'error';
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  /// Refresh current Turso sync status from Rust backend.
  Future<void> checkStatus() async {
    try {
      final statusJson = RustLib.instance.api.crateFfiTursoDbTursoStatus();
      final Map<String, dynamic> data =
          jsonDecode(statusJson) as Map<String, dynamic>;

      _isConfigured = data['configured'] as bool? ?? false;
      _status = data['status'] as String? ?? 'idle';
      _lastSyncedAt = (data['last_synced_at'] as num?)?.toInt();
      lastErrorValue = data["last_error"] as String?;
      if (_lastError != null) {
        debugPrint('turso last_error: $lastErrorValue');
        logRuntimeError('turso last_error: $lastErrorValue');
      }
      notifyListeners();
    } catch (e) {
      debugPrint('Error fetching Turso status: $e');
    }
  }
}
