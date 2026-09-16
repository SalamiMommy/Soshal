import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/widgets.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'bookmarks_service.dart';
import 'calls_service.dart';
import 'chatrandom_service.dart';
import 'dating_service.dart';
import 'error_log.dart';
import 'events_service.dart';
import 'feed_service.dart';
import 'ffi_bridge.dart';
import 'friends_service.dart';
import 'groups_service.dart';
import 'marketplace_service.dart';
import 'messaging_service.dart';
import 'minis_service.dart';
import 'moderation_service.dart';
import 'music_service.dart';
import 'notifications_service.dart';
import 'p2p_service.dart';
import 'scheduled_service.dart';
import 'search_service.dart';
import 'shell_service.dart';
import 'stealth_service.dart';
import 'streaming_service.dart';
import 'sync_service.dart';
import 'turso_service.dart';
import 'zap_service.dart';
import '../utils/service_guard.dart';

/// Session Service
/// Handles multi-account management, session persistence, and keychain
class SessionService extends ChangeNotifier with LastErrorMixin, ServiceGuard {
  SessionData? _session;
  String? _activePubkey;
  Future<SessionData>? _loadFuture;
  SyncService? _sync;
  AppLifecycleListener? _lifecycle;

  SessionService() {
    _lifecycle = AppLifecycleListener(
      onHide: _saveIfActive,
      onPause: _saveIfActive,
    );
  }

  void _saveIfActive() {
    if (_session != null) {
      saveSession();
    }
  }

  @override
  void dispose() {
    _lifecycle?.dispose();
    super.dispose();
  }

  /// Attach the sync engine (wired from main.dart). Account switches stop
  /// the old engine before the new account takes over so its events don't
  /// route into the switched account's feed/DM surfaces.
  void attachSync(SyncService sync) => _sync = sync;

  FeedService? _feed;
  MessagingService? _messaging;
  NotificationService? _notifications;
  SearchService? _search;
  DatingService? _dating;
  MarketplaceService? _marketplace;
  EventsService? _events;
  GroupsService? _groups;
  BookmarksService? _bookmarks;
  ModerationService? _moderation;
  CallsService? _calls;
  FriendsService? _friends;
  MinisService? _minis;
  MusicService? _music;
  ZapService? _zap;
  StreamingService? _streaming;
  ScheduledService? _scheduled;
  StealthService? _stealth;
  ChatrandomService? _chatrandom;
  P2pService? _p2p;
  TursoService? _turso;
  ShellService? _shell;

  /// Attach the account-scoped services (wired from main.dart) so an account
  /// switch can clear their caches before the new account's data arrives.
  void attachAccountScopedServices({
    required FeedService feed,
    required MessagingService messaging,
    required NotificationService notifications,
    required SearchService search,
    required DatingService dating,
    required MarketplaceService marketplace,
    required EventsService events,
    required GroupsService groups,
    required BookmarksService bookmarks,
    required ModerationService moderation,
    required CallsService calls,
    FriendsService? friends,
    MinisService? minis,
    MusicService? music,
    ZapService? zap,
    StreamingService? streaming,
    ScheduledService? scheduled,
    StealthService? stealth,
    ChatrandomService? chatrandom,
    P2pService? p2p,
    TursoService? turso,
    ShellService? shell,
  }) {
    _feed = feed;
    _messaging = messaging;
    _notifications = notifications;
    _search = search;
    _dating = dating;
    _marketplace = marketplace;
    _events = events;
    _groups = groups;
    _bookmarks = bookmarks;
    _moderation = moderation;
    _calls = calls;
    _friends = friends;
    _minis = minis;
    _music = music;
    _zap = zap;
    _streaming = streaming;
    _scheduled = scheduled;
    _stealth = stealth;
    _chatrandom = chatrandom;
    _p2p = p2p;
    _turso = turso;
    _shell = shell;
    if (_activePubkey != null && _activePubkey!.isNotEmpty) {
      _stealth?.setActivePubkey(_activePubkey);
    }
  }

  void _resetAccountScopedServices() {
    _feed?.resetForAccountSwitch();
    _messaging?.resetForAccountSwitch();
    _notifications?.resetForAccountSwitch();
    _search?.resetForAccountSwitch();
    _dating?.resetForAccountSwitch();
    _marketplace?.resetForAccountSwitch();
    _events?.resetForAccountSwitch();
    _groups?.resetForAccountSwitch();
    _bookmarks?.resetForAccountSwitch();
    _moderation?.resetForAccountSwitch();
    _calls?.resetForAccountSwitch();
    _friends?.resetForAccountSwitch();
    _minis?.resetForAccountSwitch();
    _music?.resetForAccountSwitch();
    _zap?.resetForAccountSwitch();
    _streaming?.resetForAccountSwitch();
    _scheduled?.resetForAccountSwitch();
    _stealth?.resetForAccountSwitch();
    _chatrandom?.resetForAccountSwitch();
    _p2p?.resetForAccountSwitch();
    _turso?.resetForAccountSwitch();
    _shell?.resetForAccountSwitch();
  }

  void reset() {
    _resetAccountScopedServices();
  }

  SessionData? get session => _session;
  String? get activePubkey => _activePubkey;
  // Null when no account matches: dummy pubkey '' would sign/publish as
  // empty. Callers must null-check instead of sending anonymous events.
  SessionAccount? get activeAccount {
    final s = _session;
    final pk = _activePubkey;
    if (s == null || pk == null || pk.isEmpty) return null;
    for (final a in s.accounts) {
      if (a.pubkey == pk) return a;
    }
    return null;
  }

  /// Returns the account with the highest last_used timestamp, or null if no accounts.
  SessionAccount? get lastUsedAccount {
    final s = _session;
    if (s == null || s.accounts.isEmpty) return null;
    SessionAccount? best;
    for (final a in s.accounts) {
      if (best == null || a.lastUsed > best.lastUsed) {
        best = a;
      }
    }
    return best;
  }

  /// Load session from storage
  Future<SessionData> loadSession() => guard(() async {
        final dbPath = await FfiBridge.getDbPath();
        final sessionJson = RustLib.instance.api.crateFfiSessionSessionLoad(
          dbPath: dbPath,
        );
        _session = SessionData.fromJson(
          jsonDecode(sessionJson) as Map<String, dynamic>,
        );
        _activePubkey = _session!.activePubkey;
        if (_activePubkey == null ||
            !_session!.accounts.any((a) => a.pubkey == _activePubkey)) {
          final lastUsed = lastUsedAccount;
          if (lastUsed != null) {
            _activePubkey = lastUsed.pubkey;
            _session = SessionData(
              activePubkey: _activePubkey,
              accounts: _session!.accounts,
            );
          }
        }
        return _session!;
      });

  /// Save session to storage
  Future<bool> saveSession() async {
    try {
      if (_session == null) {
        throw Exception('No session to save');
      }

      final dbPath = await FfiBridge.getDbPath();
      final ok = RustLib.instance.api.crateFfiSessionSessionSave(
        dbPath: dbPath,
        sessionData: jsonEncode(_session!.toJson()),
      );
      return ok;
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
    await guard(() async {
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
    });
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
    final updated = SessionAccount(
      pubkey: pubkey,
      npub: _session!.accounts[index].npub,
      lastUsed: _session!.accounts[index].lastUsed,
      relayList: relays,
    );
    final previous = _session!.accounts[index];
    _session!.accounts[index] = updated;
    try {
      await saveSession();
    } catch (e) {
      _session!.accounts[index] = previous;
      rethrow;
    }
    notifyListeners();
  }

  /// Switch to a different account
  Future<void> switchAccount(String pubkey) async {
    await guard(() async {
      await _ensureLoaded();

      if (!_session!.accounts.any((a) => a.pubkey == pubkey)) {
        throw Exception('Account not found');
      }

      // Stop the old account's sync engine before switching so its events
      // stop routing into the switched account's feed/DM.
      await _sync?.stop();

      final nowSec = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      final accIndex = _session!.accounts.indexWhere((a) => a.pubkey == pubkey);
      if (accIndex >= 0) {
        final current = _session!.accounts[accIndex];
        _session!.accounts[accIndex] = SessionAccount(
          pubkey: current.pubkey,
          npub: current.npub,
          lastUsed: nowSec,
          relayList: current.relayList,
        );
      }

      RustLib.instance.api.crateFfiSessionSessionSwitchAccount(
        pubkey: pubkey,
      );
      _activePubkey = pubkey;
      _session = SessionData(
        activePubkey: pubkey,
        accounts: _session!.accounts,
      );

      // Clear every account-scoped service cache BEFORE the new account's
      // sync engine starts, so Account B never renders Account A's posts,
      // DMs, notifications, or search results during the switch gap.
      _resetAccountScopedServices();
      _stealth?.setActivePubkey(pubkey);

      // Restart the sync engine for the newly selected account. The earlier
      // stop() killed the old account's ingest; without this restart the
      // switched account's feed/DM updates stay silent until app relaunch.
      final accountRelays = _session!.accounts
          .firstWhere((a) => a.pubkey == pubkey,
              orElse: () => _session!.accounts.first)
          .relayList;
      final relays = accountRelays.isNotEmpty
          ? accountRelays
          : const ['wss://relay.nostr.band', 'wss://nos.lol'];
      await _sync?.start(relays: relays);
    });
  }

  /// Remove account from session (persisted via saveSession)
  Future<void> removeAccount(String pubkey) async {
    await guard(() async {
      await _ensureLoaded();

      _session!.accounts.removeWhere((a) => a.pubkey == pubkey);

      if (_activePubkey == pubkey) {
        // Active account removed — stop its sync engine and clear its
        // account-scoped caches before the survivor's data arrives.
        await _sync?.stop();
        _resetAccountScopedServices();
        if (_session!.accounts.isNotEmpty) {
          _activePubkey = _session!.accounts.first.pubkey;
          _stealth?.setActivePubkey(_activePubkey);
          // Restart the sync engine for the surviving account; the stop()
          // above killed the old ingest and without this the survivor's
          // feed/DM updates stay silent until app relaunch.
          final accountRelays = _session!.accounts
              .firstWhere((a) => a.pubkey == _activePubkey,
                  orElse: () => _session!.accounts.first)
              .relayList;
          final relays = accountRelays.isNotEmpty
              ? accountRelays
              : const ['wss://relay.nostr.band', 'wss://nos.lol'];
          await _sync?.start(relays: relays);
        } else {
          _activePubkey = null;
        }
        _session = SessionData(
          activePubkey: _activePubkey,
          accounts: _session!.accounts,
        );
      }

      await saveSession();
    });
  }

  /// Check if a session is active
  bool hasActiveSession() => _activePubkey != null;

  /// Get all accounts
  List<SessionAccount> getAccounts() => _session?.accounts ?? [];

  /// Active account JSON straight from the Rust session file.
  Future<Map<String, dynamic>> getActiveAccountJson() => guard(() {
        final json = RustLib.instance.api.crateFfiSessionSessionGetActive();
        final decoded = jsonDecode(json);
        return decoded is Map<String, dynamic> ? decoded : <String, dynamic>{};
      }, notifyOnSuccess: false);

  /// All accounts JSON straight from the Rust session file.
  Future<List<dynamic>> listAccountsJson() => guard(() {
        final json = RustLib.instance.api.crateFfiSessionSessionListAccounts();
        final decoded = jsonDecode(json);
        return decoded is List<dynamic> ? decoded : const [];
      }, notifyOnSuccess: false);

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
  Future<SessionData?> refreshFromRust() => guard(() async {
        final active = await getActiveAccountJson();
        final accounts = await listAccountsJson();
        _session = SessionData(
          activePubkey: active['pubkey'] as String?,
          accounts: accounts
              .map((a) => SessionAccount.fromJson(a as Map<String, dynamic>))
              .toList(),
        );
        _activePubkey = _session!.activePubkey;
        return _session;
      });
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
      pubkey: json.strOf('pubkey'),
      npub: json.strOf('npub'),
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
      activePubkey: json.strOrNull('active_pubkey'),
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
