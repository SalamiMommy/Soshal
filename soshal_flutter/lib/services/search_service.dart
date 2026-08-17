// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/offthread.dart';
import 'error_log.dart';

/// Search Service
/// Local FTS5 search across posts, profiles, hashtags and mentions.
class SearchService extends ChangeNotifier with LastErrorMixin {
  List<SearchResultItem> _results = [];
  List<SearchResultItem> _trendingProfiles = [];
  List<String> _hashtags = [];
  List<String> _trendingHashtags = [];

  List<SearchResultItem> get results => _results;
  List<SearchResultItem> get trendingProfilesList => _trendingProfiles;
  List<String> get hashtags => _hashtags;
  List<String> get trendingHashtagsList => _trendingHashtags;

  /// Search posts (returns post rows as raw JSON).
  Future<List<SearchResultItem>> searchPosts(String query,
      {int limit = 50}) async {
    return _run(
      () => RustLib.instance.api.crateFfiSearchSearchPosts(
        query: query,
        limit: limit,
      ),
    );
  }

  /// Search profiles.
  Future<List<SearchResultItem>> searchProfiles(String query,
      {int limit = 50}) async {
    return _run(
      () => RustLib.instance.api.crateFfiSearchSearchProfiles(
        query: query,
        limit: limit,
      ),
    );
  }

  /// Search hashtags.
  Future<List<String>> searchHashtags(String query, {int limit = 50}) async {
    try {
      _hashtags = RustLib.instance.api.crateFfiSearchSearchHashtags(
        query: query,
        limit: limit,
      );
      clearLastError();
      notifyListeners();
      return _hashtags;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Search mention rows (pubkey/name pairs from kind-3 contact lists).
  Future<List<SearchResultItem>> mentions(String query,
      {int limit = 50}) async {
    return _run(
      () => RustLib.instance.api.crateFfiSearchSearchMentions(
        query: query,
        limit: limit,
      ),
    );
  }

  /// Global search across all indexes (SearchResult rows).
  Future<List<SearchResultItem>> searchGlobal(String query,
      {int limit = 50}) async {
    return _run(
      () => RustLib.instance.api.crateFfiSearchSearchGlobal(
        query: query,
        limit: limit,
      ),
    );
  }

  /// Remote NIP-50 search across relays (verified text notes, raw JSON).
  Future<String> remoteGlobalSearch(
      String query, int limit, List<String> relays) async {
    try {
      final json = await RustLib.instance.api.crateFfiSearchSearchRemoteGlobal(
        query: query,
        limit: BigInt.from(limit),
        relaysJson: jsonEncode(relays),
      );
      clearLastError();
      notifyListeners();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Trending hashtags.
  Future<List<String>> trendingHashtags({int limit = 20}) async {
    try {
      _trendingHashtags =
          RustLib.instance.api.crateFfiSearchSearchTrendingHashtags(
        limit: limit,
      );
      clearLastError();
      notifyListeners();
      return _trendingHashtags;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Trending profiles.
  Future<List<SearchResultItem>> trendingProfiles({int limit = 20}) async {
    try {
      final json = RustLib.instance.api.crateFfiSearchSearchTrendingProfiles(
        limit: limit,
      );
      final decoded = jsonDecode(json);
      if (decoded is List) {
        _trendingProfiles = decoded
            .map((e) => SearchResultItem.fromJson(e as Map<String, dynamic>))
            .toList();
      } else {
        _trendingProfiles = [];
      }
      clearLastError();
      notifyListeners();
      return _trendingProfiles;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  Future<List<SearchResultItem>> _run(String Function() call) async {
    try {
      final json = call();
      final results = await runOffThread(() => _parseSearchResults(json));
      _results = results;
      clearLastError();
      notifyListeners();
      return _results;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Trending hashtags with usage counts straight from the local DB
  /// (rows: tag/pubkey/last_used_at/count) — richer than the search-core
  /// name-only list above.
  Future<List<Map<String, dynamic>>> dbTrendingHashtags(
      {int limit = 20}) async {
    try {
      final json =
          RustLib.instance.api.crateFfiDbDbGetTrendingHashtags(limit: limit);
      clearLastError();
      final list = jsonDecode(json) as List<dynamic>;
      return list.map((e) => Map<String, dynamic>.from(e as Map)).toList();
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }
}

/// JSON → [SearchResultItem] list, top-level so [compute] can run it on a
/// background isolate (search result decoding stays off the UI thread).
List<SearchResultItem> _parseSearchResults(String json) {
  final decoded = jsonDecode(json);
  if (decoded is! List) return const [];
  return decoded
      .map((e) => SearchResultItem.fromJson(e as Map<String, dynamic>))
      .toList();
}

/// A single search hit (post/profile/mention rows as raw JSON).
class SearchResultItem {
  final String id;
  final String title;
  final String description;
  final String? pubkey;
  final String kind;
  final int createdAt;

  SearchResultItem({
    required this.id,
    required this.title,
    required this.description,
    this.pubkey,
    required this.kind,
    required this.createdAt,
  });

  factory SearchResultItem.fromJson(Map<String, dynamic> json) {
    final rawKind = json['result_type'] as String? ?? '';
    String kind = rawKind;
    if (kind.isEmpty) {
      if (json.containsKey('event_id')) kind = 'post';
      if (json.containsKey('name')) kind = 'profile';
      if (json.containsKey('tag')) kind = 'hashtag';
      if (json.containsKey('mention')) kind = 'mention';
    }
    return SearchResultItem(
      id: (json['id'] ?? json['event_id'] ?? json['pubkey'] ?? '') as String,
      title: (json['title'] ?? json['name'] ?? json['content'] ?? '') as String,
      description: (json['description'] ??
          json['about'] ??
          json['content'] ??
          '') as String,
      pubkey: json['pubkey'] as String?,
      kind: kind,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
    );
  }
}
