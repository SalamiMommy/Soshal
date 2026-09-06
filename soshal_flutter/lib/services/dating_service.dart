// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Dating Service
/// Profile creation/browsing, likes, matches, filters and stats.
class DatingService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify, ServiceGuard {
  List<DatingCard> _cards = [];
  final List<DatingCard> _matches = [];
  final List<DatingCard> _likes = [];
  DatingCard? _ownProfile;

  List<DatingCard> get cards => _cards;
  List<DatingCard> get matches => _matches;
  List<DatingCard> get likes => _likes;
  DatingCard? get ownProfile => _ownProfile;

  /// Clear all account-scoped state on account switch so Account B never
  /// sees Account A's cached cards, matches, likes, or own profile.
  void resetForAccountSwitch() {
    _cards = [];
    _matches.clear();
    _likes.clear();
    _ownProfile = null;
    clearLastError();
    notifyListeners();
  }

  Future<List<DatingCard>> fetchProfiles(String userPubkey,
      {int limit = 50}) async {
    return _decode(
      () => RustLib.instance.api.crateFfiDatingDatingFetchProfiles(
        userPubkey: userPubkey,
        limit: limit,
        audience: 'public',
      ),
    );
  }

  Future<List<DatingCard>> filterProfiles(
    String userPubkey, {
    int minAge = 0,
    int maxAge = 0,
    int radiusKm = 0,
    int heightMinCm = 0,
    int heightMaxCm = 0,
    String bodyType = '',
    String smoking = '',
    String drinking = '',
    String relationshipIntent = '',
    String politics = '',
    String education = '',
    List<String> interests = const [],
  }) async {
    return _decode(
      () => RustLib.instance.api.crateFfiDatingDatingFilterProfiles(
        userPubkey: userPubkey,
        minAge: minAge,
        maxAge: maxAge,
        locationRadiusKm: radiusKm,
        heightMinCm: heightMinCm,
        heightMaxCm: heightMaxCm,
        bodyType: bodyType,
        smoking: smoking,
        drinking: drinking,
        relationshipIntent: relationshipIntent,
        politics: politics,
        education: education,
        interestsJson: jsonEncode(interests),
      ),
    );
  }

  Future<List<DatingCard>> fetchMatches(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiDatingDatingFetchMatches(
        userPubkey: userPubkey,
      );
      final cards = await runOffThread(() => _parseCards(json));
      _matches.clear();
      _matches.addAll(cards);
      clearLastError();
      notifyDeferred();
      return _matches;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<DatingCard>> fetchLikes(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiDatingDatingFetchLikes(
        userPubkey: userPubkey,
      );
      final cards = await runOffThread(() => _parseCards(json));
      _likes.clear();
      _likes.addAll(cards);
      clearLastError();
      notifyDeferred();
      return _likes;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<DatingCard>> fetchLikesForMatch(String userPubkey) => guard(
      () async {
        final json = RustLib.instance.api.crateFfiDatingDatingFetchLikes(
          userPubkey: userPubkey,
        );
        return await runOffThread(() => _parseCards(json));
      },
      onNotify: notifyDeferred,
      notifyOnSuccess: false,
    );

  Future<double> calculateScore(String userPubkey, String targetPubkey) => guard(
      () async {
        final prefs = <String, dynamic>{};
        final own = _ownProfile;
        if (own != null) {
          if (own.preferenceWeights.isNotEmpty) {
            prefs['preferenceWeights'] = own.preferenceWeights;
          }
          if (own.dealbreakers.isNotEmpty) {
            prefs['dealbreakers'] = own.dealbreakers;
          }
        }
        return RustLib.instance.api.crateFfiDatingDatingCalculateScore(
          userPubkey: userPubkey,
          targetPubkey: targetPubkey,
          preferencesJson: prefs.isEmpty ? '{}' : jsonEncode(prefs),
        );
      },
      onNotify: notifyDeferred,
      notifyOnSuccess: false,
    );

  Future<DatingCard> getOwnProfile(String userPubkey) async {
    try {
      final json = RustLib.instance.api.crateFfiDatingDatingGetOwnProfile(
        userPubkey: userPubkey,
      );
      _ownProfile = DatingCard.fromJson(jsonDecode(json));
      clearLastError();
      notifyDeferred();
      return _ownProfile!;
    } catch (e, st) {
      // "No dating profile yet" is the legitimate pre-profile empty state,
      // not an error: don't spam the SVC ERROR surface while onboarding
      // screens poll it.
      if (!'$e'.contains('No dating profile yet')) {
        setLastError(e, st);
      }
      _ownProfile = null;
      notifyDeferred();
      rethrow;
    }
  }

  /// Fetch a single dating profile by profile event id (fresh from the
  /// store, not the swipe deck).
  Future<DatingCard> getProfile(String profileId) => guard(() {
        final json = RustLib.instance.api.crateFfiDatingDatingGetProfile(
          profileId: profileId,
        );
        return DatingCard.fromJson(
          jsonDecode(json) as Map<String, dynamic>,
        );
      }, onNotify: notifyDeferred);

  Future<String> createProfile(
    String userPubkey,
    String name,
    int age,
    String location, {
    String gender = '',
    String seeking = '',
    int heightCm = 0,
    String bodyType = '',
    String smoking = '',
    String drinking = '',
    String relationshipIntent = '',
    String politics = '',
    String ethnicity = '',
    String education = '',
    List<String> language = const [],
    int maxDistanceKm = 0,
    String bio = '',
    List<String> images = const [],
    List<String> interests = const [],
  }) async {
    try {
      final eventId = RustLib.instance.api.crateFfiDatingDatingCreateProfile(
        userPubkey: userPubkey,
        name: name,
        age: age,
        location: location,
        gender: gender,
        seeking: seeking,
        heightCm: heightCm,
        bodyType: bodyType,
        smoking: smoking,
        drinking: drinking,
        relationshipIntent: relationshipIntent,
        politics: politics,
        ethnicity: ethnicity,
        education: education,
        languageJson: jsonEncode(language),
        maxDistanceKm: maxDistanceKm,
        bio: bio,
        imagesJson: jsonEncode(images),
        interestsJson: jsonEncode(interests),
      );
      clearLastError();
      await getOwnProfile(userPubkey);
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> updateProfile(
    String userPubkey,
    String bio,
    List<String> images,
    List<String> interests, {
    String location = '',
    String gender = '',
    String seeking = '',
    int heightCm = 0,
    String bodyType = '',
    String smoking = '',
    String drinking = '',
    String relationshipIntent = '',
    String politics = '',
    String ethnicity = '',
    String education = '',
    List<String> language = const [],
    int maxDistanceKm = 0,
  }) async {
    try {
      final ok = RustLib.instance.api.crateFfiDatingDatingUpdateProfile(
        userPubkey: userPubkey,
        location: location,
        gender: gender,
        seeking: seeking,
        heightCm: heightCm,
        bodyType: bodyType,
        smoking: smoking,
        drinking: drinking,
        relationshipIntent: relationshipIntent,
        politics: politics,
        ethnicity: ethnicity,
        education: education,
        languageJson: jsonEncode(language),
        maxDistanceKm: maxDistanceKm,
        bio: bio,
        imagesJson: jsonEncode(images),
        interestsJson: jsonEncode(interests),
      );
      clearLastError();
      await getOwnProfile(userPubkey);
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> deleteProfile(String userPubkey) => guard(() {
        final ok = RustLib.instance.api.crateFfiDatingDatingDeleteProfile(
          userPubkey: userPubkey,
        );
        _ownProfile = null;
        return ok;
      }, onNotify: notifyDeferred);

  Future<bool> like(String userPubkey, String profileId) async {
    final ok = await _bool(
      () => RustLib.instance.api.crateFfiDatingDatingLike(
          userPubkey: userPubkey, profileId: profileId),
    );
    if (ok) {
      _cards.removeWhere((c) => c.pubkey == profileId);
      notifyDeferred();
    }
    return ok;
  }

  Future<bool> unlike(String userPubkey, String profileId) async {
    return _bool(
      () => RustLib.instance.api.crateFfiDatingDatingUnlike(
          userPubkey: userPubkey, profileId: profileId),
    );
  }

  Future<bool> superlike(String userPubkey, String profileId) async {
    final ok = await _bool(
      () => RustLib.instance.api.crateFfiDatingDatingSuperlike(
          userPubkey: userPubkey, profileId: profileId),
    );
    if (ok) {
      _cards.removeWhere((c) => c.pubkey == profileId);
      notifyDeferred();
    }
    return ok;
  }

  Future<bool> pass(String userPubkey, String profileId) async {
    final ok = await _bool(
      () => RustLib.instance.api.crateFfiDatingDatingPass(
          userPubkey: userPubkey, profileId: profileId),
    );
    if (ok) {
      _cards.removeWhere((c) => c.pubkey == profileId);
      notifyDeferred();
    }
    return ok;
  }

  Future<bool> block(String userPubkey, String targetPubkey) async {
    return _bool(
      () => RustLib.instance.api.crateFfiDatingDatingBlockProfile(
          userPubkey: userPubkey, targetPubkey: targetPubkey),
    );
  }

  /// Reset profiles the user swiped "no" on: deletes local `pass` records so
  /// they re-enter the discover deck. Returns how many were reset.
  Future<int> resetPasses(String userPubkey) => guard(() {
        final n = RustLib.instance.api.crateFfiDatingDatingResetPasses(
          userPubkey: userPubkey,
        );
        return n;
      }, onNotify: notifyDeferred);

  Future<bool> unblock(String userPubkey, String targetPubkey) async {
    return _bool(
      () => RustLib.instance.api.crateFfiDatingDatingUnblockProfile(
          userPubkey: userPubkey, targetPubkey: targetPubkey),
    );
  }

  Future<bool> unmatch(String userPubkey, String profileId) async {
    try {
      final ok = RustLib.instance.api.crateFfiDatingDatingUnmatch(
        userPubkey: userPubkey,
        profileId: profileId,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return false;
    }
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

  Future<DatingStats> getStats(String userPubkey) => guard(
      () async {
        final json = RustLib.instance.api.crateFfiDatingDatingGetStats(
          userPubkey: userPubkey,
        );
        final decoded = jsonDecode(json);
        return DatingStats.fromJson(
            decoded is Map<String, dynamic> ? decoded : <String, dynamic>{});
      },
      onNotify: notifyDeferred,
      notifyOnSuccess: false,
    );

  Future<bool> _bool(bool Function() call) async {
    return guard(() {
      return call();
    });
  }

  Future<List<DatingCard>> _decode(String Function() call) async {
    return guard(() async {
      final json = call();
      _cards = await runOffThread(() => _parseCards(json));
      return _cards;
    });
  }
}

/// A dating card as surfaced by the bridge.
class DatingCard {
  final String pubkey;
  final String name;
  final int age;
  final String location;
  final String gender;
  final String seeking;
  final double height;
  final String bodyType;
  final String smoking;
  final String drinking;
  final String relationshipIntent;
  final String politics;
  final String ethnicity;
  final String education;
  final List<String> language;
  final double maxDistanceKm;
  final String bio;
  final List<String> images;
  final List<String> interests;
  final double compatibilityScore;
  final int lastSeen;
  final Map<String, double> preferenceWeights;
  final List<String> dealbreakers;

  DatingCard({
    required this.pubkey,
    required this.name,
    required this.age,
    required this.location,
    this.gender = '',
    this.seeking = '',
    this.height = 0,
    this.bodyType = '',
    this.smoking = '',
    this.drinking = '',
    this.relationshipIntent = '',
    this.politics = '',
    this.ethnicity = '',
    this.education = '',
    this.language = const [],
    this.maxDistanceKm = 0,
    required this.bio,
    required this.images,
    required this.interests,
    required this.compatibilityScore,
    required this.lastSeen,
    this.preferenceWeights = const {},
    this.dealbreakers = const [],
  });

  factory DatingCard.fromJson(Map<String, dynamic> json) {
    return DatingCard(
      pubkey: json.strOf('pubkey'),
      name: json.strOf('name'),
      age: json.intOf('age'),
      location: json.strOf('location'),
      gender: json.strOf('gender'),
      seeking: json.strOf('seeking'),
      height: (json['height'] as num?)?.toDouble() ?? 0,
      bodyType: json.strOf('body_type'),
      smoking: json.strOf('smoking'),
      drinking: json.strOf('drinking'),
      relationshipIntent: json.strOf('relationship_intent'),
      politics: json.strOf('politics'),
      ethnicity: json.strOf('ethnicity'),
      education: json.strOf('education'),
      language: (json['language'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      maxDistanceKm: (json['max_distance_km'] as num?)?.toDouble() ?? 0,
      bio: json.strOf('bio'),
      images: (json['images'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      interests: (json['interests'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
      compatibilityScore:
          (json['compatibility_score'] as num?)?.toDouble() ?? 0,
      lastSeen: json.intOf('last_seen'),
      preferenceWeights: (json['preferenceWeights'] as Map<String, dynamic>?)
              ?.map((k, v) => MapEntry(k, (v as num?)?.toDouble() ?? 0)) ??
          const {},
      dealbreakers: (json['dealbreakers'] as List<dynamic>? ?? [])
          .map((e) => e.toString())
          .toList(),
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
      profileViews: json.intOf('profile_views'),
      likesReceived: json.intOf('likes_received'),
      superlikeReceived: json.intOf('superlike_received'),
      matches: json.intOf('matches'),
      profileComplete: json.boolOf('profile_complete'),
      photoCount: json.intOf('photo_count'),
    );
  }
}

/// JSON → [DatingCard] list, top-level so [runOffThread] can decode on a
/// background isolate.
List<DatingCard> _parseCards(String json) => (jsonDecode(json) as List<dynamic>)
    .map((e) => DatingCard.fromJson(e as Map<String, dynamic>))
    .toList();
