// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

import 'messaging_service.dart';

/// Friends Service
/// Friend suggestions, friend requests, and an in-memory local contact list.
/// The contact list is kept in memory only (no persistence) until the
/// backend-gated contact store lands.
class FriendsService extends ChangeNotifier with LastErrorMixin {
  List<String> _suggestions = [];
  bool _suggestionsLoaded = false;
  final List<ProfileInfo> _contacts = [];

  List<String> get suggestions => _suggestions;
  bool get suggestionsLoaded => _suggestionsLoaded;
  List<ProfileInfo> get contacts => _contacts;

  /// Load friend suggestions from the bridge (pubkey list from the social
  /// contact graph). Empty when nothing to suggest yet.
  Future<List<String>> fetchSuggestions() async {
    try {
      _suggestions =
          RustLib.instance.api.crateFfiSocialSocialFriendSuggestions();
      _suggestionsLoaded = true;
      _lastError = null;
      notifyListeners();
      return _suggestions;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Send a friend request to `pubkey`. Returns true when accepted.
  Future<bool> sendFriendRequest(String pubkey) async {
    try {
      final ok =
          RustLib.instance.api.crateFfiRelationsRelationsSendFriendRequest(
        pubkey: pubkey,
      );
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Add a profile to the in-memory contact list.
  void addContact(ProfileInfo profile) {
    if (_contacts.any((c) => c.pubkey == profile.pubkey)) return;
    _contacts.add(profile);
    _lastError = null;
    notifyListeners();
  }

  /// Remove a contact from the in-memory list.
  void removeContact(String pubkey) {
    _contacts.removeWhere((c) => c.pubkey == pubkey);
    notifyListeners();
  }

  /// Clear error.
  void clearError() {
    _lastError = null;
    notifyListeners();
  }
}
