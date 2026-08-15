// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import 'ffi_bridge.dart';

/// Session Service
/// Handles multi-account management, session persistence, and keychain
class SessionService extends ChangeNotifier with LastErrorMixin {
  SessionData? _session;
  String? _activePubkey;
  Future<SessionData>? _loadFuture;

  SessionData? get session => _session;
  String? get activePubkey => _activePubkey;
  SessionAccount? get activeAccount => _session?.accounts.firstWhere(
        (a) => a.pubkey == _activePubkey,
        orElse: () => SessionAccount(
          pubkey: '',
          npub: '',
          lastUsed: 0,
          relayList: [],
        ),
      );

  /// Load session from storage
  Future<SessionData> loadSession() async {
    try {
      final dbPath = await FfiBridge.getDbPath();
      final sessionJson = RustLib.instance.api.crateFfiSessionSessionLoad(
        dbPath: dbPath,
      );
      _session = SessionData.fromJson(
        jsonDecode(sessionJson) as Map<String, dynamic>,
      );
      _activePubkey = _session!.activePubkey;

      clearLastError();
      notifyListeners();
      return _session!;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Save session to storage
  Future<bool> saveSession() async {
    try {
      if (_session == null) {
        throw Exception('No session to save');
      }

      final dbPath = await FfiBridge.getDbPath();
      return RustLib.instance.api.crateFfiSessionSessionSave(
        dbPath: dbPath,
        sessionData: jsonEncode(_session!.toJson()),
      );
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Ensure the session is loaded (from storage on first use) before
  /// mutating it. The splash screen loads it eagerly, but when that load
  /// fails the app still routes to /auth — this makes the auth flow
  /// self-sufficient instead of dead-ending with "Session not loaded".
  Future<SessionData> _ensureLoaded() async {
    if (_session == null) {
      _loadFuture ??= loadSession();
      try {
        await _loadFuture;
      } finally {
        _loadFuture = null;
      }
    }
    return _session!;
  }

  /// Add account to session
  Future<void> addAccount(
      String pubkey, String npub, List<String> relays) async {
    try {
      await _ensureLoaded();

      RustLib.instance.api.crateFfiSessionSessionAddAccount(
        pubkey: pubkey,
        npub: npub,
        relaysJson: jsonEncode(relays),
      );

      _session!.accounts.add(
        SessionAccount(
          pubkey: pubkey,
          npub: npub,
          lastUsed: DateTime.now().millisecondsSinceEpoch ~/ 1000,
          relayList: relays,
        ),
      );

      if (_session!.activePubkey == null) {
        _session = SessionData(
          activePubkey: pubkey,
          accounts: _session!.accounts,
        );
        _activePubkey = pubkey;
      }

      clearLastError();
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Seed a base profile row so the account is indexable/searchable
  /// (identity-core). Fire-and-forget friendly; never throws in the
  /// caller-facing flow since the store is best-effort.
  Future<bool> storeProfile(String profileJson) async {
    return RustLib.instance.api
        .crateFfiIdentityIdentityStoreProfile(profile: profileJson);
  }

  /// Persist an updated relay list for an account.
  Future<void> updateRelays(String pubkey, List<String> relays) async {
    if (_session == null) return;
    final index = _session!.accounts.indexWhere((a) => a.pubkey == pubkey);
    if (index < 0) return;
    _session!.accounts[index] = SessionAccount(
      pubkey: pubkey,
      npub: _session!.accounts[index].npub,
      lastUsed: _session!.accounts[index].lastUsed,
      relayList: relays,
    );
    await saveSession();
    notifyListeners();
  }

  /// Switch to a different account
  Future<void> switchAccount(String pubkey) async {
    try {
      await _ensureLoaded();

      if (!_session!.accounts.any((a) => a.pubkey == pubkey)) {
        throw Exception('Account not found');
      }

      RustLib.instance.api.crateFfiSessionSessionSwitchAccount(
        pubkey: pubkey,
      );
      _activePubkey = pubkey;
      _session = SessionData(
        activePubkey: pubkey,
        accounts: _session!.accounts,
      );

      clearLastError();
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Remove account from session (persisted via saveSession)
  Future<void> removeAccount(String pubkey) async {
    try {
      await _ensureLoaded();

      _session!.accounts.removeWhere((a) => a.pubkey == pubkey);

      if (_activePubkey == pubkey) {
        if (_session!.accounts.isNotEmpty) {
          _activePubkey = _session!.accounts.first.pubkey;
        } else {
          _activePubkey = null;
        }
        _session = SessionData(
          activePubkey: _activePubkey,
          accounts: _session!.accounts,
        );
      }

      clearLastError();
      notifyListeners();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Check if a session is active
  bool hasActiveSession() => _activePubkey != null;

  /// Get all accounts
  List<SessionAccount> getAccounts() => _session?.accounts ?? [];

  /// Active account JSON straight from the Rust session file.
  Future<Map<String, dynamic>> getActiveAccountJson() async {
    try {
      final json = RustLib.instance.api.crateFfiSessionSessionGetActive();
      final decoded = jsonDecode(json);
      clearLastError();
      return decoded is Map<String, dynamic> ? decoded : <String, dynamic>{};
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// All accounts JSON straight from the Rust session file.
  Future<List<dynamic>> listAccountsJson() async {
    try {
      final json = RustLib.instance.api.crateFfiSessionSessionListAccounts();
      final decoded = jsonDecode(json);
      clearLastError();
      return decoded is List<dynamic> ? decoded : const [];
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Register (or clear, when empty) the push token for the active account
  /// in the Rust session file.
  Future<bool> registerPushToken(String token) async {
    try {
      final ok = RustLib.instance.api
          .crateFfiSessionSessionRegisterPushToken(token: token);
      clearLastError();
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      return false;
    }
  }

  /// Re-sync the in-memory session from the Rust session file (used by the
  /// Accounts screen "refresh from Rust" action).
  Future<SessionData?> refreshFromRust() async {
    try {
      final active = await getActiveAccountJson();
      final accounts = await listAccountsJson();
      _session = SessionData(
        activePubkey: active['pubkey'] as String?,
        accounts: accounts
            .map((a) => SessionAccount.fromJson(a as Map<String, dynamic>))
            .toList(),
      );
      _activePubkey = _session!.activePubkey;
      clearLastError();
      notifyListeners();
      return _session;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }
}

/// Account entry in the persisted session file (mirrors session.json v2).
class SessionAccount {
  final String pubkey;
  final String npub;
  final int lastUsed;
  final List<String> relayList;

  SessionAccount({
    required this.pubkey,
    required this.npub,
    required this.lastUsed,
    required this.relayList,
  });

  factory SessionAccount.fromJson(Map<String, dynamic> json) {
    return SessionAccount(
      pubkey: json['pubkey'] as String? ?? '',
      npub: json['npub'] as String? ?? '',
      lastUsed: json['last_used'] as int? ?? 0,
      relayList: List<String>.from(json['relay_list'] ?? []),
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'pubkey': pubkey,
      'npub': npub,
      'last_used': lastUsed,
      'relay_list': relayList,
    };
  }
}

/// Session file payload: active account + account list.
class SessionData {
  final String? activePubkey;
  final List<SessionAccount> accounts;

  SessionData({
    this.activePubkey,
    required this.accounts,
  });

  factory SessionData.fromJson(Map<String, dynamic> json) {
    return SessionData(
      activePubkey: json['active_pubkey'] as String?,
      accounts: (json['accounts'] as List?)
              ?.map((a) => SessionAccount.fromJson(a))
              .toList() ??
          [],
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'active_pubkey': activePubkey,
      'accounts': accounts.map((a) => a.toJson()).toList(),
    };
  }
}
