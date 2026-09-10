// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Search Service
/// Local FTS5 search across posts, profiles, hashtags and mentions.
class SearchService extends ChangeNotifier with LastErrorMixin, ServiceGuard {
  List<SearchResultItem> _results = [];
  List<SearchResultItem> _trendingProfiles = [];
  DateTime? _trendingProfilesFetchedAt;
  List<String> _hashtags = [];
  List<String> _trendingHashtags = [];
  DateTime? _trendingHashtagsFetchedAt;
  List<Map<String, dynamic>> _dbTrendingHashtags = [];
  DateTime? _dbTrendingHashtagsFetchedAt;

  static const Duration _trendingCacheTtl = Duration(seconds: 30);

  List<SearchResultItem> get results => _results;
  List<SearchResultItem> get trendingProfilesList => _trendingProfiles;
  List<String> get hashtags => _hashtags;
  List<String> get trendingHashtagsList => _trendingHashtags;
  List<Map<String, dynamic>> get dbTrendingHashtagsList => _dbTrendingHashtags;

  /// Clear all account-scoped search state on account switch so Account B
  /// never sees Account A's cached results.
  void resetForAccountSwitch() {
    _results = [];
    _trendingProfiles = [];
    _trendingProfilesFetchedAt = null;
    _hashtags = [];
    _trendingHashtags = [];
    _trendingHashtagsFetchedAt = null;
    _dbTrendingHashtags = [];
    _dbTrendingHashtagsFetchedAt = null;
    clearLastError();
    notifyListeners();
  }

  /// Search posts (returns post rows as raw JSON).
  Future<List<SearchResultItem>> searchPosts(String query,
      {int limit = 50}) async {
    return _run(
      () => RustLib.instance.api.crateFfiSearchSearchPosts(
        query: query,
        limit: limit,
        audience: 'public',
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
  Future<List<String>> searchHashtags(String query, {int limit = 50}) =>
      guard(() {
        _hashtags = RustLib.instance.api.crateFfiSearchSearchHashtags(
          query: query,
          limit: limit,
        );
        return _hashtags;
      });

  /// Search mention rows (pubkey/name pairs from kind-3 contact lists).
  /// Kept separate from _results so typing @mentions does not wipe or dirty
  /// the main search screen results.
  Future<List<SearchResultItem>> mentions(String query, {int limit = 50}) =>
      guard(() async {
        final json = RustLib.instance.api.crateFfiSearchSearchMentions(
          query: query,
          limit: limit,
        );
        return await runOffThreadCompute(_parseSearchResults, json);
      }, clearOnSuccess: false, notifyOnSuccess: false, notifyOnError: false);

  /// Global search across all indexes (SearchResult rows).
  Future<List<SearchResultItem>> searchGlobal(String query,
      {int limit = 50}) async {
    return _run(
      () => RustLib.instance.api.crateFfiSearchSearchGlobal(
        query: query,
        limit: limit,
        audience: 'public',
      ),
    );
  }

  /// Remote NIP-50 search across relays (verified text notes, raw JSON).
  Future<String> remoteGlobalSearch(
      String query, int limit, List<String> relays) async {
    return guard(() async {
      return await RustLib.instance.api.crateFfiSearchSearchRemoteGlobal(
        query: query,
        limit: BigInt.from(limit),
        relaysJson: jsonEncode(relays),
      );
    });
  }

  /// Trending hashtags.
  Future<List<String>> trendingHashtags({int limit = 20}) => guard(() {
        if (_trendingHashtagsFetchedAt != null &&
            DateTime.now().difference(_trendingHashtagsFetchedAt!) <
                _trendingCacheTtl &&
            _trendingHashtags.isNotEmpty) {
          return _trendingHashtags;
        }
        _trendingHashtags =
            RustLib.instance.api.crateFfiSearchSearchTrendingHashtags(
          limit: limit,
        );
        _trendingHashtagsFetchedAt = DateTime.now();
        return _trendingHashtags;
      });

  /// Trending profiles.
  Future<List<SearchResultItem>> trendingProfiles({int limit = 20}) =>
      guard(() async {
        if (_trendingProfilesFetchedAt != null &&
            DateTime.now().difference(_trendingProfilesFetchedAt!) <
                _trendingCacheTtl &&
            _trendingProfiles.isNotEmpty) {
          return _trendingProfiles;
        }
        final json = RustLib.instance.api.crateFfiSearchSearchTrendingProfiles(
          limit: limit,
        );
        _trendingProfiles = await runOffThreadCompute(_parseSearchResults, json);
        _trendingProfilesFetchedAt = DateTime.now();
        return _trendingProfiles;
      });

  Future<List<SearchResultItem>> _run(String Function() call) async {
    return guard(() async {
      final json = call();
      final results = await runOffThreadCompute(_parseSearchResults, json);
      _results = results;
      return _results;
    });
  }

  /// Trending hashtags with usage counts straight from the local DB
  /// (rows: tag/pubkey/last_used_at/count) — richer than the search-core
  /// name-only list above.
  Future<List<Map<String, dynamic>>> dbTrendingHashtags({int limit = 20}) =>
      guard(() async {
        if (_dbTrendingHashtagsFetchedAt != null &&
            DateTime.now().difference(_dbTrendingHashtagsFetchedAt!) <
                _trendingCacheTtl &&
            _dbTrendingHashtags.isNotEmpty) {
          return _dbTrendingHashtags;
        }
        final json =
            RustLib.instance.api.crateFfiDbDbGetTrendingHashtags(limit: limit);
        _dbTrendingHashtags =
            await runOffThreadCompute(_parseTrendingHashtags, json);
        _dbTrendingHashtagsFetchedAt = DateTime.now();
        return _dbTrendingHashtags;
      });
}

/// JSON → [SearchResultItem] list, top-level so [compute] can run it on a
/// JSON → hashtag row maps, top-level so [runOffThread] can decode on a
/// background isolate.
List<Map<String, dynamic>> _parseTrendingHashtags(String json) {
  final decoded = jsonDecode(json);
  if (decoded is! List) return const [];
  return List<Map<String, dynamic>>.generate(
    decoded.length,
    (i) => Map<String, dynamic>.from(decoded[i] as Map),
    growable: true,
  );
}

/// JSON → [SearchResultItem] list, top-level so [runOffThread] can decode on a
/// background isolate (search result decoding stays off the UI thread).
List<SearchResultItem> _parseSearchResults(String json) {
  final decoded = jsonDecode(json);
  if (decoded is! List) return const [];
  return List<SearchResultItem>.generate(
    decoded.length,
    (i) => SearchResultItem.fromJson(decoded[i] as Map<String, dynamic>),
    growable: true,
  );
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
    final rawKind = json.strOf('result_type');
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
      pubkey: json.strOrNull('pubkey'),
      kind: kind,
      createdAt: json.intOf('created_at'),
    );
  }
}
