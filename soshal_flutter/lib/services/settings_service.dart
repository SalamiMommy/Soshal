// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Settings key/value persistence plus storage statistics and destructive
/// post purges. Thin wrapper over the db FFI — screens own their UI state.
class SettingsService extends ChangeNotifier {
  /// Read a persisted setting; empty string when unset.
  String getSetting(String key) =>
      RustLib.instance.api.crateFfiDbDbGetSetting(key: key) ?? '';

  /// Persist a setting key/value pair.
  Future<void> setSetting(String key, String value) async {
    RustLib.instance.api.crateFfiDbDbSetSetting(key: key, value: value);
  }

  /// Per-table row counts plus `__db_file__` size row.
  List<Map<String, dynamic>> storageStats() {
    final json = RustLib.instance.api.crateFfiDbDbStorageStats();
    final rows = jsonDecode(json) as List<dynamic>;
    return rows.map((e) => e as Map<String, dynamic>).toList();
  }

  /// Run an arbitrary DELETE/UPDATE statement. Returns false on error.
  Future<bool> safeDelete(String sql) async {
    try {
      RustLib.instance.api.crateFfiDbDbExecuteRaw(sql: sql);
      return true;
    } catch (e) {
      debugPrint('settings delete: $e');
      return false;
    }
  }

  /// Mark posts older than 30 days as deleted.
  Future<bool> purgeOldPosts() {
    final cutoff = DateTime.now()
            .subtract(const Duration(days: 30))
            .millisecondsSinceEpoch ~/
        1000;
    return safeDelete(
      'UPDATE posts SET is_deleted = 1 WHERE is_deleted = 0 AND created_at < $cutoff',
    );
  }

  /// Mark every locally cached post as deleted.
  Future<bool> purgeAllPosts() {
    return safeDelete('UPDATE posts SET is_deleted = 1 WHERE is_deleted = 0');
  }

  /// Absolute path of the active SQLite database file.
  String dbPath() => RustLib.instance.api.crateFfiDbDbPath();

  /// Storage engine mode string (e.g. `mmap` / `fsync`).
  String ioEngineMode() =>
      RustLib.instance.api.crateFfiStorageStorageGetIoEngineMode();

  /// Delete expired ephemeral media rows; returns the expired ids.
  List<String> cleanExpiredEphemeral() =>
      RustLib.instance.api.crateFfiEphemeralEphemeralCleanExpired();

  /// SHA-256 of `input` as lowercase hex.
  String sha256Hex(String input) =>
      RustLib.instance.api.crateFfiUtilUtilSha256Hex(input: input);

  /// Base64url (no padding) encode.
  String b64UrlEncode(String input) =>
      RustLib.instance.api.crateFfiUtilUtilBase64UrlEncode(input: input);

  /// Base64url (no padding) decode; empty string on invalid input.
  String b64UrlDecode(String input) =>
      RustLib.instance.api.crateFfiUtilUtilBase64UrlDecode(input: input);

  /// Purge geohash peer rows not seen within `cutoffSecsAgo`; rows removed.
  int purgeStaleGeohashPeers(int cutoffSecsAgo) =>
      RustLib.instance.api
          .crateFfiDbDbPurgeStaleGeohashPeers(cutoffSecsAgo: cutoffSecsAgo)
          .toInt();
}
