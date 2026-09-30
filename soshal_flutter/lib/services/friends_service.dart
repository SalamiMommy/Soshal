// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
import 'package:flutter/foundation.dart' show listEquals;
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';
import 'messaging_service.dart';
import 'session_service.dart';

/// Target audience tiers for content filtering.
enum AudienceFilter {
  all,
  friendsOfFriends,
  friends;

  String get label => switch (this) {
        AudienceFilter.all => 'All',
        AudienceFilter.friendsOfFriends => 'Friends of Friends',
        AudienceFilter.friends => 'Friends',
      };

  String get shortLabel => switch (this) {
        AudienceFilter.all => 'All',
        AudienceFilter.friendsOfFriends => 'Friends of Friends',
        AudienceFilter.friends => 'Friends',
      };

  IconData get icon => switch (this) {
        AudienceFilter.all => Icons.public,
        AudienceFilter.friendsOfFriends => Icons.groups_outlined,
        AudienceFilter.friends => Icons.people_outline,
      };
}

/// Friends Service
/// Friend suggestions, friend requests, and an in-memory local contact list.
/// The contact list is kept in memory only (no persistence) until the
/// backend-gated contact store lands.
class FriendsService extends ChangeNotifier
    with LastErrorMixin, ServiceGuard, DeferredNotify {
  List<String> _suggestions = [];
  bool _suggestionsLoaded = false;
  final List<ProfileInfo> _contacts = [];
  Set<String> _friendPubkeys = {};
  Set<String> _fofPubkeys = {};
  String? _loadedForPubkey;

  /// Bumped whenever the audience graph's *contents* change.
  ///
  /// This is the service's own record of "a notification means something changed",
  /// which it otherwise does not: `ServiceGuard.guard` notifies on completion of
  /// every guarded call — success, failure, or a no-op — so a single
  /// `loadAudienceGraph` used to fire four notifications (the guarded
  /// `fetchFollows`, the guarded `fetchSuggestions`, the guarded
  /// `fetchFollowsUnion`, and its own), and every cache hit fired a fifth that
  /// changed nothing at all. With twelve screens watching through `filterList`,
  /// each of those rebuilt twelve whole subtrees.
  ///
  /// Those notifications are now suppressed at the source instead (see
  /// [loadAudienceGraph]), which is the actual fix — the screens still `watch`
  /// the service, because `select` cannot be used here. It is worth keeping the
  /// counter as an observable assertion of that contract rather than as a
  /// selection key: a caller or test can check that a notification-worthy
  /// operation really did change the graph, independently of whether anything
  /// rebuilt.
  int _audienceRevision = 0;
  int get audienceRevision => _audienceRevision;

  List<String> get suggestions => _suggestions;
  bool get suggestionsLoaded => _suggestionsLoaded;
  List<ProfileInfo> get contacts => _contacts;
  Set<String> get friendPubkeys => _friendPubkeys;
  Set<String> get fofPubkeys => _fofPubkeys;

  /// Clear in-memory audience graph on account switch.
  void resetForAccountSwitch() {
    _friendPubkeys.clear();
    _fofPubkeys.clear();
    _loadedForPubkey = null;
    _suggestions.clear();
    _suggestionsLoaded = false;
    _contacts.clear();
    clearLastError();
    _audienceRevision++;
    notifyListeners();
  }

  /// Load friend suggestions from the bridge (pubkey list from the social
  /// contact graph). Empty when nothing to suggest yet.
  Future<List<String>> fetchSuggestions() =>
      guard(_fetchSuggestionsUnguarded);

  /// Body of [fetchSuggestions], without the [guard] wrapper.
  ///
  /// Split out because [loadAudienceGraph] calls this as one step of building the
  /// graph, and a `guard` notification at that point is pure noise: it rebuilds
  /// every audience-filtered screen against a `_fofPubkeys` that has not been
  /// assigned yet. The wrapper is right for a caller asking for suggestions; it
  /// is wrong for an internal step that swallows its own result.
  List<String> _fetchSuggestionsUnguarded() {
    final next = RustLib.instance.api.crateFfiSocialSocialFriendSuggestions();
    // Only a real content change is a graph change. The bridge returns a
    // fresh list each call, so an identity check would always say "changed";
    // the suggestions are small, and an exact comparison is the only way to
    // keep a repeated no-op refresh from rebuilding every audience-filtered
    // screen.
    if (!listEquals(next, _suggestions)) {
      _suggestions = next;
      _audienceRevision++;
    }
    _suggestionsLoaded = true;
    return _suggestions;
  }

  /// Send a friend request to `pubkey`. Returns true when accepted.
  Future<bool> sendFriendRequest(String pubkey) => guard(() {
        return RustLib.instance.api.crateFfiRelationsRelationsSendFriendRequest(
          pubkey: pubkey,
        );
      });

  /// Refreshes the follow list for `pubkey` straight from relays; returns
  /// the JSON array of followed pubkeys (kind-3 contacts).
  Future<String> fetchFollows(String pubkey) => guard(() {
        return RustLib.instance.api.crateFfiIdentityIdentityFetchFollows(
          pubkey: pubkey,
        );
      });

  /// The deduped union of the follow lists of many accounts, as a `List<String>`.
  ///
  /// `fetchFollows` is a `#[frb(sync)]` call, so walking a list of accounts with
  /// `await` in a loop does 25 blocking FFI round-trips on the UI isolate —
  /// `Future.wait` cannot help, because the calls already ran by the time the
  /// list literal was built. This collapses the whole traversal into one bridge
  /// call and one `WHERE pubkey IN (...)` query on the Rust side.
  Future<List<String>> fetchFollowsUnion(List<String> pubkeys) => guard(() {
        final raw = RustLib.instance.api
            .crateFfiIdentityIdentityFetchFollowsUnion(pubkeys: pubkeys);
        final decoded = jsonDecode(raw);
        return decoded is List
            ? decoded.whereType<String>().toList()
            : <String>[];
      });

  /// Unguarded bodies of [fetchFollows] and [fetchFollowsUnion], for
  /// [loadAudienceGraph]'s internal use. See
  /// [_fetchSuggestionsUnguarded] for why the wrapper is bypassed there.
  String _fetchFollowsUnguarded(String pubkey) =>
      RustLib.instance.api.crateFfiIdentityIdentityFetchFollows(pubkey: pubkey);

  List<String> _fetchFollowsUnionUnguarded(List<String> pubkeys) {
    final raw = RustLib.instance.api
        .crateFfiIdentityIdentityFetchFollowsUnion(pubkeys: pubkeys);
    final decoded = jsonDecode(raw);
    return decoded is List
        ? decoded.whereType<String>().toList()
        : <String>[];
  }

  /// Add a profile to the in-memory contact list.
  void addContact(ProfileInfo profile) {
    if (_contacts.any((c) => c.pubkey == profile.pubkey)) return;
    _contacts.add(profile);
    _friendPubkeys.add(profile.pubkey);
    _audienceRevision++;
    clearLastError();
    notifyListeners();
  }

  /// Remove a contact from the in-memory list.
  void removeContact(String pubkey) {
    final before = _contacts.length;
    _contacts.removeWhere((c) => c.pubkey == pubkey);
    _friendPubkeys.remove(pubkey);
    if (_contacts.length != before) _audienceRevision++;
    notifyListeners();
  }

  /// Load and cache the user's direct friends and friends-of-friends graph.
  ///
  /// This fires exactly one notification, and only when the graph it builds
  /// differs from the one it already had. It used to fire four: the guarded
  /// `fetchFollows`, the guarded `fetchSuggestions`, the guarded
  /// `fetchFollowsUnion`, and its own — three of them before `_fofPubkeys` was
  /// even assigned, so each rebuilt the twelve audience-filtered screens against
  /// half-updated state. The nested calls therefore go through the unguarded
  /// bodies, and `guard` is told not to notify on success. The error path still
  /// notifies, so the error banner still appears.
  ///
  /// A nested failure no longer sets `lastError` either. That is the better
  /// behaviour, not a regression: the result is swallowed here, so a graph load
  /// that overall succeeded should not raise an error banner for a step that
  /// failed and was handled.
  Future<void> loadAudienceGraph(String myPubkey, {bool force = false}) =>
      guard(() async {
        if (!force &&
            _loadedForPubkey == myPubkey &&
            _friendPubkeys.isNotEmpty) {
          // Cache hit. The body returns without notifying, but `guard` would
          // notify on the way out, and eight screens call this from `initState`
          // — so every navigation to one of them rebuilt the whole screen to be
          // handed the identical graph it already had.
          return;
        }
        final follows = <String>{};
        try {
          final raw = _fetchFollowsUnguarded(myPubkey);
          final decoded = jsonDecode(raw);
          if (decoded is List) {
            follows.addAll(decoded.whereType<String>());
          }
        } catch (_) {}

        for (final c in _contacts) {
          follows.add(c.pubkey);
        }
        _friendPubkeys = follows;

        final fof = <String>{};
        try {
          fof.addAll(_fetchSuggestionsUnguarded());
        } catch (_) {}

        // Traverse first-degree follows to expand friends of friends. One
        // batched call instead of 25 sequential `fetchFollows` round-trips:
        // `fetchFollows` is a sync FFI fn, so each `await` in that loop cost a
        // full UI-isolate block. The batch merges the lists in the same
        // first-occurrence order, so `fof` ends up identical either way.
        try {
          fof.addAll(_fetchFollowsUnionUnguarded(follows.take(25).toList()));
        } catch (_) {}
        fof.remove(myPubkey);
        _fofPubkeys = fof;
        _loadedForPubkey = myPubkey;
        _audienceRevision++;
        // `notifyDeferred`, not `notifyListeners`. All three bridge calls here
        // are `#[frb(sync)]`, so this body no longer has a single `await` and
        // runs to completion synchronously — eight screens reach it from
        // `initState`, so a direct notify would fire mid-build and trip the
        // "setState() or markNeedsBuild() called during build" assertion. The old
        // version was accidentally safe only because `await fetchFollows`
        // suspended before the notify; removing the awaits removed that.
        notifyDeferred();
      }, notifyOnSuccess: false);

  /// Check whether an author matches the given audience filter.
  bool matchesAudience(
    String? pubkey,
    AudienceFilter filter, {
    String? myPubkey,
  }) {
    if (filter == AudienceFilter.all) return true;
    if (pubkey == null || pubkey.isEmpty) return false;
    if (myPubkey != null && pubkey == myPubkey) return true;

    final isFriend = _friendPubkeys.contains(pubkey) ||
        _contacts.any((c) => c.pubkey == pubkey);
    if (filter == AudienceFilter.friends) {
      return isFriend;
    }
    if (filter == AudienceFilter.friendsOfFriends) {
      return isFriend ||
          _fofPubkeys.contains(pubkey) ||
          _suggestions.contains(pubkey);
    }
    return true;
  }

  /// Filter a list of items according to an audience tier.
  List<T> filterList<T>(
    List<T> items,
    AudienceFilter filter,
    String Function(T item) getPubkey, {
    String? myPubkey,
  }) {
    if (filter == AudienceFilter.all) return items;
    return items
        .where((item) =>
            matchesAudience(getPubkey(item), filter, myPubkey: myPubkey))
        .toList();
  }
}

/// Convenience extension for optional FriendsService access in screens.
extension FriendsServiceContext on BuildContext {
  /// Safely watch [FriendsService], returning null if not provided in the widget tree.
  FriendsService? get friendsServiceOrNull {
    try {
      return watch<FriendsService>();
    } catch (_) {
      return null;
    }
  }

  /// Safely read [FriendsService], returning null if not provided in the widget tree.
  FriendsService? get friendsServiceReadOrNull {
    try {
      return read<FriendsService>();
    } catch (_) {
      return null;
    }
  }

  /// Safely read active pubkey from [SessionService], returning null if not provided.
  String? get activePubkeyOrNull {
    try {
      return read<SessionService>().activePubkey;
    } catch (_) {
      return null;
    }
  }
}
