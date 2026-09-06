// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

import 'messaging_service.dart';

/// Friends Service
/// Friend suggestions, friend requests, and an in-memory local contact list.
/// The contact list is kept in memory only (no persistence) until the
/// backend-gated contact store lands.
class FriendsService extends ChangeNotifier with LastErrorMixin, ServiceGuard {
  List<String> _suggestions = [];
  bool _suggestionsLoaded = false;
  final List<ProfileInfo> _contacts = [];

  List<String> get suggestions => _suggestions;
  bool get suggestionsLoaded => _suggestionsLoaded;
  List<ProfileInfo> get contacts => _contacts;

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
        return RustLib.instance.api
            .crateFfiRelationsRelationsSendFriendRequest(
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
    clearLastError();
    notifyListeners();
  }

  /// Remove a contact from the in-memory list.
  void removeContact(String pubkey) {
    _contacts.removeWhere((c) => c.pubkey == pubkey);
    notifyListeners();
  }
}
