// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Dating Service
/// Profile creation/browsing, likes, matches, filters and stats.
class DatingService extends ChangeNotifier with LastErrorMixin {
  List<DatingCard> _cards = [];
  final List<DatingCard> _matches = [];
  final List<DatingCard> _likes = [];
  DatingCard? _ownProfile;

  List<DatingCard> get cards => _cards;
  List<DatingCard> get matches => _matches;
  List<DatingCard> get likes => _likes;
  DatingCard? get ownProfile => _ownProfile;

  Future<List<DatingCard>> fetchProfiles(String userPubkey,
      {int limit = 50}) async {
    return _decode(
      () => RustLib.instance.api.crateFfiDatingDatingFetchProfiles(
        userPubkey: userPubkey,
        limit: limit,
      ),
    );
  }

  Future<List<DatingCard>> filterProfiles(
    String userPubkey, {
    int minAge = 0,
    int maxAge = 0,
    int radiusKm = 0,
    List<String> interests = const [],
  }) async {
    return _decode(
      () => RustLib.instance.api.crateFfiDatingDatingFilterProfiles(
        userPubkey: userPubkey,
        minAge: minAge,
        maxAge: maxAge,
        locationRadiusKm: radiusKm,
        interestsJson: jsonEncode(interests),
      ),
    );
  }

  Future<List<DatingCard>> fetchMatches(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiDatingDatingFetchMatches(
        userPubkey: userPubkey,
      );
      _matches.clear();
      _matches.addAll(_parseCards(json));
      _lastError = null;
      notifyListeners();
      return _matches;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<List<DatingCard>> fetchLikes(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiDatingDatingFetchLikes(
        userPubkey: userPubkey,
      );
      _likes.clear();
      _likes.addAll(_parseCards(json));
      _lastError = null;
      notifyListeners();
      return _likes;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<List<DatingCard>> fetchLikesForMatch(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiDatingDatingFetchLikes(
        userPubkey: userPubkey,
      );
      _lastError = null;
      return _parseCards(json);
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<double> calculateScore(String userPubkey, String targetPubkey) async {
    try {
      final score = RustLib.instance.api.crateFfiDatingDatingCalculateScore(
        userPubkey: userPubkey,
        targetPubkey: targetPubkey,
        preferencesJson: '{}',
      );
      _lastError = null;
      return score;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<DatingCard> getOwnProfile(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiDatingDatingGetOwnProfile(
        userPubkey: userPubkey,
      );
      _ownProfile = DatingCard.fromJson(jsonDecode(json));
      _lastError = null;
      notifyListeners();
      return _ownProfile!;
    } catch (e, st) {
      setLastError(e, st);
      _ownProfile = null;
      notifyListeners();
      rethrow;
    }
  }

  Future<String> createProfile(
    String userPubkey,
    String name,
    int age,
    String location,
    String bio,
    List<String> images,
    List<String> interests,
  ) async {
    try {
      final eventId = RustLib.instance.api.crateFfiDatingDatingCreateProfile(
        userPubkey: userPubkey,
        name: name,
        age: age,
        location: location,
        bio: bio,
        imagesJson: jsonEncode(images),
        interestsJson: jsonEncode(interests),
      );
      _lastError = null;
      await getOwnProfile(userPubkey);
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<bool> updateProfile(
    String userPubkey,
    String bio,
    List<String> images,
    List<String> interests,
  ) async {
    try {
      final ok = RustLib.instance.api.crateFfiDatingDatingUpdateProfile(
        userPubkey: userPubkey,
        bio: bio,
        imagesJson: jsonEncode(images),
        interestsJson: jsonEncode(interests),
      );
      _lastError = null;
      await getOwnProfile(userPubkey);
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<bool> deleteProfile(String userPubkey) async {
    try {
      final ok = RustLib.instance.api.crateFfiDatingDatingDeleteProfile(
        userPubkey: userPubkey,
      );
      _ownProfile = null;
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<bool> like(String userPubkey, String profileId) async {
    _cards.removeWhere((c) => c.pubkey == profileId);
    return _bool(
      () => RustLib.instance.api.crateFfiDatingDatingLike(
          userPubkey: userPubkey, profileId: profileId),
    );
  }

  Future<bool> unlike(String userPubkey, String profileId) async {
    return _bool(
      () => RustLib.instance.api.crateFfiDatingDatingUnlike(
          userPubkey: userPubkey, profileId: profileId),
    );
  }

  Future<bool> superlike(String userPubkey, String profileId) async {
    _cards.removeWhere((c) => c.pubkey == profileId);
    return _bool(
      () => RustLib.instance.api.crateFfiDatingDatingSuperlike(
          userPubkey: userPubkey, profileId: profileId),
    );
  }

  Future<bool> pass(String userPubkey, String profileId) async {
    _cards.removeWhere((c) => c.pubkey == profileId);
    return _bool(
      () => RustLib.instance.api.crateFfiDatingDatingPass(
          userPubkey: userPubkey, profileId: profileId),
    );
  }

  Future<bool> block(String userPubkey, String targetPubkey) async {
    return _bool(
      () => RustLib.instance.api.crateFfiDatingDatingBlockProfile(
          userPubkey: userPubkey, targetPubkey: targetPubkey),
    );
  }

  Future<bool> unblock(String userPubkey, String targetPubkey) async {
    return _bool(
      () => RustLib.instance.api.crateFfiDatingDatingUnblockProfile(
          userPubkey: userPubkey, targetPubkey: targetPubkey),
    );
  }

  Future<bool> report(
      String reporterPubkey, String targetPubkey, String reason) async {
    return _bool(
      () => RustLib.instance.api.crateFfiDatingDatingReportProfile(
          reporterPubkey: reporterPubkey,
          targetPubkey: targetPubkey,
          reason: reason),
    );
  }

  Future<DatingStats> getStats(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiDatingDatingGetStats(
        userPubkey: userPubkey,
      );
      final decoded = jsonDecode(json);
      final stats = DatingStats.fromJson(
          decoded is Map<String, dynamic> ? decoded : <String, dynamic>{});
      _lastError = null;
      return stats;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<bool> _bool(bool Function() call) async {
    try {
      final ok = call();
      _lastError = null;
      notifyListeners();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<List<DatingCard>> _decode(String Function() call) async {
    try {
      final json = call();
      _cards = _parseCards(json);
      _lastError = null;
      notifyListeners();
      return _cards;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  static List<DatingCard> _parseCards(String json) =>
      (jsonDecode(json) as List<dynamic>)
          .map((e) => DatingCard.fromJson(e as Map<String, dynamic>))
          .toList();
}

/// A dating card as surfaced by the bridge.
class DatingCard {
  final String pubkey;
  final String name;
  final int age;
  final String location;
  final String bio;
  final List<String> images;
  final List<String> interests;
  final double compatibilityScore;
  final int lastSeen;

  DatingCard({
    required this.pubkey,
    required this.name,
    required this.age,
    required this.location,
    required this.bio,
    required this.images,
    required this.interests,
    required this.compatibilityScore,
    required this.lastSeen,
  });

  factory DatingCard.fromJson(Map<String, dynamic> json) {
    return DatingCard(
      pubkey: json['pubkey'] as String? ?? '',
      name: json['name'] as String? ?? '',
      age: (json['age'] as num?)?.toInt() ?? 0,
      location: json['location'] as String? ?? '',
      bio: json['bio'] as String? ?? '',
      images: (json['images'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      interests: (json['interests'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      compatibilityScore:
          (json['compatibility_score'] as num?)?.toDouble() ?? 0,
      lastSeen: (json['last_seen'] as num?)?.toInt() ?? 0,
    );
  }
}

/// Dating account statistics.
class DatingStats {
  final int profileViews;
  final int likesReceived;
  final int superlikeReceived;
  final int matches;
  final bool profileComplete;
  final int photoCount;

  DatingStats({
    required this.profileViews,
    required this.likesReceived,
    required this.superlikeReceived,
    required this.matches,
    required this.profileComplete,
    required this.photoCount,
  });

  factory DatingStats.fromJson(Map<String, dynamic> json) {
    return DatingStats(
      profileViews: (json['profile_views'] as num?)?.toInt() ?? 0,
      likesReceived: (json['likes_received'] as num?)?.toInt() ?? 0,
      superlikeReceived: (json['superlike_received'] as num?)?.toInt() ?? 0,
      matches: (json['matches'] as num?)?.toInt() ?? 0,
      profileComplete: json['profile_complete'] as bool? ?? false,
      photoCount: (json['photo_count'] as num?)?.toInt() ?? 0,
    );
  }
}
