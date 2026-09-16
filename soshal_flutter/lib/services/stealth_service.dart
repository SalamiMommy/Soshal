// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Stealth Service
/// Local whitelist of pubkeys allowed to see you when stealth mode is
/// active, stored as a newline-separated setting via the local DB.
class StealthService extends ChangeNotifier with LastErrorMixin, ServiceGuard {
  String? _activePubkey;
  List<String> _whitelist = [];

  List<String> get whitelist => _whitelist;

  String _keyFor([String? pubkey]) {
    final pk = pubkey ?? _activePubkey;
    if (pk != null && pk.isNotEmpty) {
      return 'stealth_whitelist_$pk';
    }
    return 'stealth_whitelist';
  }

  /// Sets the active account pubkey for scoped whitelist storage.
  void setActivePubkey(String? pubkey) {
    if (_activePubkey != pubkey) {
      _activePubkey = pubkey;
      _whitelist = [];
      clearLastError();
      notifyListeners();
    }
  }

  /// Reset in-memory state on account switch or logout.
  void resetForAccountSwitch() {
    _activePubkey = null;
    _whitelist = [];
    clearLastError();
    notifyListeners();
  }

  /// Load the whitelist from the local DB setting.
  Future<List<String>> load([String? pubkey]) => guard(() {
        final raw = RustLib.instance.api.crateFfiDbDbGetSetting(
          key: _keyFor(pubkey),
        );
        _whitelist = (raw ?? '')
            .split('\n')
            .map((s) => s.trim())
            .where((s) => s.isNotEmpty)
            .toList();
        return _whitelist;
      });

  /// Persist the whitelist as a newline-separated setting value.
  Future<bool> save(List<String> items, [String? pubkey]) async {
    try {
      final ok = RustLib.instance.api.crateFfiDbDbSetSetting(
        key: _keyFor(pubkey),
        value: items.join('\n'),
      );
      if (ok) {
        _whitelist = List.from(items);
        clearLastError();
        notifyListeners();
      }
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Delete the stored whitelist setting.
  Future<bool> clear([String? pubkey]) => guard(() {
        final ok = RustLib.instance.api.crateFfiDbDbDeleteSetting(
          key: _keyFor(pubkey),
        );
        _whitelist = [];
        return ok;
      });
}
