// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Stealth Service
/// Local whitelist of pubkeys allowed to see you when stealth mode is
/// active, stored as a newline-separated setting via the local DB.
class StealthService extends ChangeNotifier with LastErrorMixin {
  static const _whitelistKey = 'stealth_whitelist';

  List<String> _whitelist = [];

  List<String> get whitelist => _whitelist;

  /// Load the whitelist from the local DB setting.
  Future<List<String>> load() async {
    try {
      final raw = RustLib.instance.api.crateFfiDbDbGetSetting(
        key: _whitelistKey,
      );
      _whitelist = (raw ?? '')
          .split('\n')
          .map((s) => s.trim())
          .where((s) => s.isNotEmpty)
          .toList();
      clearLastError();
      notifyListeners();
      return _whitelist;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Persist the whitelist as a newline-separated setting value.
  Future<bool> save(List<String> items) async {
    try {
      final ok = RustLib.instance.api.crateFfiDbDbSetSetting(
        key: _whitelistKey,
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
  Future<bool> clear() async {
    try {
      final ok = RustLib.instance.api.crateFfiDbDbDeleteSetting(
        key: _whitelistKey,
      );
      clearLastError();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }
}
