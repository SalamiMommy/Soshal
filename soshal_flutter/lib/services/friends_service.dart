// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';
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
class FriendsService extends ChangeNotifier with LastErrorMixin, ServiceGuard {
  List<String> _suggestions = [];
  bool _suggestionsLoaded = false;
  final List<ProfileInfo> _contacts = [];
  Set<String> _friendPubkeys = {};
  Set<String> _fofPubkeys = {};
  String? _loadedForPubkey;

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
    notifyListeners();
  }

  /// Load friend suggestions from the bridge (pubkey list from the social
  /// contact graph). Empty when nothing to suggest yet.
  Future<List<String>> fetchSuggestions() => guard(() {
        _suggestions =
            RustLib.instance.api.crateFfiSocialSocialFriendSuggestions();
        _suggestionsLoaded = true;
        return _suggestions;
      });

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

  /// Add a profile to the in-memory contact list.
  void addContact(ProfileInfo profile) {
    if (_contacts.any((c) => c.pubkey == profile.pubkey)) return;
    _contacts.add(profile);
    _friendPubkeys.add(profile.pubkey);
    clearLastError();
    notifyListeners();
  }

  /// Remove a contact from the in-memory list.
  void removeContact(String pubkey) {
    _contacts.removeWhere((c) => c.pubkey == pubkey);
    _friendPubkeys.remove(pubkey);
    notifyListeners();
  }

  /// Load and cache the user's direct friends and friends-of-friends graph.
  Future<void> loadAudienceGraph(String myPubkey, {bool force = false}) =>
      guard(() async {
        if (!force &&
            _loadedForPubkey == myPubkey &&
            _friendPubkeys.isNotEmpty) {
          return;
        }
        final follows = <String>{};
        try {
          final raw = await fetchFollows(myPubkey);
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
          final sugg = await fetchSuggestions();
          fof.addAll(sugg);
        } catch (_) {}

        // Traverse first-degree follows to expand friends of friends
        for (final f in follows.take(25)) {
          try {
            final raw = await fetchFollows(f);
            final decoded = jsonDecode(raw);
            if (decoded is List) {
              fof.addAll(decoded.whereType<String>());
            }
          } catch (_) {}
        }
        fof.remove(myPubkey);
        _fofPubkeys = fof;
        _loadedForPubkey = myPubkey;
        notifyListeners();
      });

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
