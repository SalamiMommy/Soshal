// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Stealth Service
/// Local whitelist of pubkeys allowed to see you when stealth mode is
/// active, stored as a newline-separated setting via the local DB.
class StealthService extends ChangeNotifier {
  static const _whitelistKey = 'stealth_whitelist';

  List<String> _whitelist = [];
  String? _lastError;

  List<String> get whitelist => _whitelist;
  String? get lastError => _lastError;

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
      _lastError = null;
      notifyListeners();
      return _whitelist;
    } catch (e) {
      _lastError = e.toString();
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
        _lastError = null;
        notifyListeners();
      }
      return ok;
    } catch (e) {
      _lastError = e.toString();
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
      _lastError = null;
      return ok;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }
}
