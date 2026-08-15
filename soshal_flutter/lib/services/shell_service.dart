// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// A single sidebar navigation entry.
class NavItem {
  final String id;
  final String label;

  const NavItem(this.id, this.label);

  factory NavItem.fromJson(Map<String, dynamic> json) =>
      NavItem(json['id'] as String? ?? '', json['label'] as String? ?? '');

  Map<String, dynamic> toJson() => {'id': id, 'label': label};
}

/// App shell state: sidebar items (with drag-reorder persistence), offline
/// status, PIN lock, global audio bar, incoming call signals.
class ShellService extends ChangeNotifier {
  static const _sidebarOrderKey = 'sidebar_order';
  static const _themeKey = 'theme';
  static const _themeOptionsKey = 'theme_options';
  static const _biometricsKey = 'biometrics_enabled';

  /// Default nav items — mirrors the legacy rust-native sidebar exactly.
  static const defaultItems = <NavItem>[
    NavItem('feed', 'Feed'),
    NavItem('notifications', 'Notifications'),
    NavItem('inbox', 'Messages'),
    NavItem('groups', 'Groups'),
    NavItem('dating', 'Dating'),
    NavItem('marketplace', 'Marketplace'),
    NavItem('events', 'Events'),
    NavItem('minis', 'Minis'),
    NavItem('stories', 'Stories'),
    NavItem('live', 'Live'),
    NavItem('music', 'Musicloud'),
    NavItem('chat_random', 'Chat Random'),
    NavItem('friends', 'Friends'),
    NavItem('profile', 'Profile'),
    NavItem('bookmarks', 'Bookmarks'),
    NavItem('settings', 'Settings'),
  ];

  /// Conditional items the legacy sidebar could show on top of the defaults.
  static const extraItems = <NavItem>[
    NavItem('scheduled', 'Scheduled Posts'),
    NavItem('vouch', 'Vouch'),
    NavItem('stealth', 'Stealth'),
    NavItem('analytics', 'Analytics'),
    NavItem('audit', 'Audit Log'),
    NavItem('backup', 'Backup'),
    NavItem('network', 'Network'),
  ];

  static const routeForItem = <String, String>{
    'feed': '/feed',
    'notifications': '/notifications',
    'inbox': '/inbox',
    'groups': '/groups',
    'dating': '/dating',
    'marketplace': '/marketplace',
    'events': '/events',
    'minis': '/minis',
    'stories': '/stories',
    'live': '/live',
    'music': '/music',
    'chat_random': '/chat-random',
    'friends': '/friends',
    'profile': '/profile',
    'bookmarks': '/bookmarks',
    'settings': '/settings',
    'scheduled': '/scheduled',
    'vouch': '/vouch',
    'stealth': '/stealth',
    'analytics': '/analytics',
    'audit': '/audit',
    'backup': '/settings/backup',
    'network': '/network',
  };

  List<NavItem> _items = List.of(defaultItems);
  List<NavItem> get items => List.unmodifiable(_items);

  bool _rearranging = false;
  bool get rearranging => _rearranging;

  int? _draggedIndex;
  int? get draggedIndex => _draggedIndex;

  bool _offline = false;
  bool get offline => _offline;

  bool _locked = false;
  bool get locked => _locked;
  bool get unlockPending => _unlockPending;
  bool _unlockPending = false;
  String? _unlockError;
  String? get unlockError => _unlockError;
  int _lockAttempts = 0;
  int get lockAttempts => _lockAttempts;
  int _lockoutRemaining = 0;
  int get lockoutRemaining => _lockoutRemaining;
  bool _permanentLocked = false;
  bool get permanentLocked => _permanentLocked;
  final bool _biometricAvailable = false;
  bool get biometricAvailable => _biometricAvailable;

  bool _hasPin = false;
  bool get hasPin => _hasPin;

  // Global audio bar (Musicloud player).
  String _audioTitle = '';
  String get audioTitle => _audioTitle;
  String _audioUrl = '';
  String get audioUrl => _audioUrl;
  bool _audioPlaying = false;
  bool get audioPlaying => _audioPlaying;

  // Incoming call signal.
  Map<String, dynamic>? _incomingCall;
  Map<String, dynamic>? get incomingCall => _incomingCall;
  final List<String> _seenCalls = [];

  String? _theme;
  String? get theme => _theme;
  String? _themeOptionsJson;
  String? get themeOptionsJson => _themeOptionsJson;
  bool _biometricsEnabled = false;
  bool get biometricsEnabled => _biometricsEnabled;

  bool _initialized = false;
  bool get initialized => _initialized;

  /// Load persisted nav order + lock/theme state. Called once after the
  /// bridge is up and the user is authenticated.
  Future<void> initialize() async {
    if (_initialized) return;
    _initialized = true;
    try {
      final saved = RustLib.instance.api.crateFfiDbDbGetSetting(
        key: _sidebarOrderKey,
      );
      if (saved != null && saved.isNotEmpty) {
        try {
          final parsed = jsonDecode(saved) as List<dynamic>;
          final loaded = parsed
              .map((e) => NavItem.fromJson(e as Map<String, dynamic>))
              .toList();
          if (loaded.isNotEmpty) {
            _items = loaded;
          }
        } catch (_) {}
      }
      final theme = RustLib.instance.api.crateFfiDbDbGetSetting(key: _themeKey);
      if (theme != null && theme.isNotEmpty) {
        _theme = theme;
      }
      final options = RustLib.instance.api.crateFfiDbDbGetSetting(
        key: _themeOptionsKey,
      );
      if (options != null && options.isNotEmpty) {
        _themeOptionsJson = options;
      }
      final bio = RustLib.instance.api.crateFfiDbDbGetSetting(
        key: _biometricsKey,
      );
      _biometricsEnabled = bio != null && bio.toLowerCase() == 'true';
      _hasPin = RustLib.instance.api.crateFfiPinPinHas();
      _locked = _hasPin || _biometricsEnabled;
      if (_locked) {
        await refreshLockout(notify: false);
      }
      notifyListeners();
    } catch (e) {
      debugPrint('shell init: $e');
    }
  }

  /// Persist the current nav item order to the settings table.
  void saveOrder() {
    try {
      RustLib.instance.api.crateFfiDbDbSetSetting(
        key: _sidebarOrderKey,
        value: jsonEncode(_items.map((i) => i.toJson()).toList()),
      );
    } catch (e) {
      debugPrint('save sidebar order: $e');
    }
  }

  /// Show/hide the conditional nav items (toggled from Settings).
  void setExtraItemsVisible(List<String> ids, {bool persist = true}) {
    final base = List.of(defaultItems);
    for (final id in ids) {
      for (final extra in extraItems) {
        if (extra.id == id) {
          base.add(extra);
        }
      }
    }
    _items = base;
    if (persist) saveOrder();
    notifyListeners();
  }

  bool get itemVisible => _items.isNotEmpty;

  void beginRearrange() {
    _rearranging = true;
    notifyListeners();
  }

  void endRearrange() {
    _rearranging = false;
    _draggedIndex = null;
    notifyListeners();
  }

  void setDraggedIndex(int? index) {
    _draggedIndex = index;
  }

  /// Move item from `from` to `to` and persist.
  void moveItem(int from, int to) {
    if (from < 0 || to < 0 || from >= _items.length || to >= _items.length) {
      return;
    }
    final moved = _items.removeAt(from);
    _items.insert(to, moved);
    saveOrder();
    notifyListeners();
  }

  /// Poll relay connectivity (drives the offline banner).
  Future<void> refreshRelayStatus() async {
    try {
      final json = await RustLib.instance.api
          .crateFfiNetworkNetworkRelayConnectionStatus();
      final v = jsonDecode(json) as Map<String, dynamic>;
      final connected = v['connected'] as int? ?? 0;
      final offline = connected == 0;
      if (offline != _offline) {
        _offline = offline;
        notifyListeners();
      }
    } catch (_) {
      // Relay client not initialized: treat as offline.
      if (!_offline) {
        _offline = true;
        notifyListeners();
      }
    }
  }

  // ─── PIN lock ────────────────────────────────────────────────────────────

  Future<void> refreshLockout({bool notify = true}) async {
    try {
      final json = RustLib.instance.api.crateFfiPinPinLockoutState();
      final v = jsonDecode(json) as Map<String, dynamic>;
      _lockAttempts = v['attemptCount'] as int? ?? 0;
      final until = v['lockoutUntil'] as int? ?? 0;
      _lockoutRemaining =
          (until - DateTime.now().millisecondsSinceEpoch).clamp(0, 1 << 62);
      _permanentLocked = v['permanentLocked'] as bool? ?? false;
      if (notify) notifyListeners();
    } catch (e) {
      debugPrint('lockout state: $e');
    }
  }

  Future<bool> unlock(String pin) async {
    if (pin.isEmpty || _unlockPending) return false;
    _unlockPending = true;
    _unlockError = null;
    notifyListeners();
    try {
      final ok = RustLib.instance.api.crateFfiPinPinVerify(pin: pin);
      if (ok) {
        _locked = false;
        _lockAttempts = 0;
        _lockoutRemaining = 0;
        _unlockError = null;
      } else {
        _unlockError = 'Wrong PIN';
        _locked = true;
        await refreshLockout();
      }
      return ok;
    } catch (e) {
      _unlockError = 'error: $e';
      await refreshLockout();
      return false;
    } finally {
      _unlockPending = false;
      notifyListeners();
    }
  }

  Future<bool> setPin(String pin) async {
    try {
      RustLib.instance.api.crateFfiPinPinSet(pin: pin);
      _hasPin = true;
      notifyListeners();
      return true;
    } catch (e) {
      debugPrint('set pin: $e');
      return false;
    }
  }

  Future<bool> clearPin(String pin) async {
    try {
      RustLib.instance.api.crateFfiPinPinClear(pin: pin);
      _hasPin = RustLib.instance.api.crateFfiPinPinHas();
      notifyListeners();
      return true;
    } catch (e) {
      debugPrint('clear pin: $e');
      return false;
    }
  }

  /// Re-check whether the app should be locked (e.g. after settings change).
  Future<void> reevaluateLock() async {
    try {
      _hasPin = RustLib.instance.api.crateFfiPinPinHas();
      final bio = RustLib.instance.api.crateFfiDbDbGetSetting(
        key: _biometricsKey,
      );
      _biometricsEnabled = bio != null && bio.toLowerCase() == 'true';
      _locked = _hasPin || _biometricsEnabled;
      if (_locked) await refreshLockout();
      notifyListeners();
    } catch (e) {
      debugPrint('reevaluate lock: $e');
    }
  }

  void lockNow() {
    if (_hasPin || _biometricsEnabled) {
      _locked = true;
      notifyListeners();
    }
  }

  // ─── Global audio bar ────────────────────────────────────────────────────

  void playAudio(String url, String title) {
    _audioUrl = url;
    _audioTitle = title;
    _audioPlaying = true;
    notifyListeners();
  }

  void stopAudio() {
    _audioUrl = '';
    _audioTitle = '';
    _audioPlaying = false;
    notifyListeners();
  }

  // ─── Incoming calls ──────────────────────────────────────────────────────

  /// Poll relay kind-20001..20004 signals addressed to me.
  Future<void> pollCallSignals(String myPubkey) async {
    if (myPubkey.isEmpty) return;
    try {
      final json = await RustLib.instance.api.crateFfiCallsCallsFetchSignals(
        myPubkey: myPubkey,
      );
      final signals = jsonDecode(json) as List<dynamic>;
      final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
      for (final s in signals) {
        final v = s as Map<String, dynamic>;
        if (v['kind'] != 20001) continue;
        final created = v['created_at'] as int? ?? 0;
        if (now - created > 60) continue;
        final callId = v['call_id'] as String? ?? '';
        if (callId.isEmpty || _seenCalls.contains(callId)) continue;
        _seenCalls.add(callId);
        _incomingCall = v;
        notifyListeners();
        break;
      }
    } catch (e) {
      debugPrint('poll call signals: $e');
    }
  }

  void acceptCall() {
    _incomingCall = null;
    notifyListeners();
  }

  void declineCall() {
    final call = _incomingCall;
    _incomingCall = null;
    if (call != null) {
      final peer = call['pubkey'] as String? ?? '';
      final callId = call['call_id'] as String? ?? '';
      final v = call['content'] is String
          ? jsonDecode(call['content'] as String)
          : null;
      final mediaType =
          (v is Map<String, dynamic> ? v['media_type'] as String? : null) ??
              'voice';
      if (peer.isNotEmpty && callId.isNotEmpty) {
        // Fire-and-forget: tell the peer the call ended.
        RustLib.instance.api
            .crateFfiCallsCallsSendSignal(
              signalType: 'end',
              targetPubkey: peer,
              callId: callId,
              sdp: null,
              candidate: null,
              mediaType: mediaType,
            )
            .then((_) {})
            .catchError((_) {});
      }
    }
    notifyListeners();
  }
}
