// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Turso Sync Service
/// Manages connection settings, replication credentials, and real-time
/// sync status for the Turso (libSQL) database engine.
class TursoService extends ChangeNotifier {
  bool _isConfigured = false;
  String _status = 'idle';
  int? _lastSyncedAt;
  String? _lastError;
  String _url = '';
  bool _isSyncing = false;

  bool get isConfigured => _isConfigured;
  String get status => _status;
  int? get lastSyncedAt => _lastSyncedAt;
  String? get lastError => _lastError;
  String get url => _url;
  bool get isSyncing => _isSyncing;

  /// Configure Turso database URL and auth token.
  Future<bool> configure(
      {required String url, required String authToken}) async {
    try {
      _lastError = null;
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
    } catch (e) {
      _lastError = e.toString();
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
    _lastError = null;
    notifyListeners();

    try {
      RustLib.instance.api.crateFfiTursoDbTursoSync();
      _isSyncing = false;
      _status = 'synced';
      _lastSyncedAt = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      notifyListeners();
      return true;
    } catch (e) {
      _isSyncing = false;
      _status = 'error';
      _lastError = e.toString();
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
      _lastError = data['last_error'] as String?;
      notifyListeners();
    } catch (e) {
      debugPrint('Error fetching Turso status: $e');
    }
  }
}
