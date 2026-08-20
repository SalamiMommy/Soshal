// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';

/// Feed Service
/// Handles feed operations and post publishing
class FeedService extends ChangeNotifier with LastErrorMixin, DeferredNotify {
  List<FeedPost> _posts = [];
  List<FeedPost> _rankedPosts = [];
  bool _ranked = false;
  bool _isLoading = false;
  bool _loadingMore = false;
  int _currentOffset = 0;
  static const String _pinnedKey = 'pinned_posts';
  final Set<String> _pinned = {};
  bool _pinnedLoaded = false;
  // Cached unmodifiable view: the getter must return the SAME instance
  // between mutations, otherwise `context.select((s) => s.pinnedPosts)` sees
  // a fresh identity every call and rebuilds on every FeedService notify.
  List<String> _pinnedView = const [];
  final List<FeedPost> _pendingIndex = [];
  bool _indexFlushScheduled = false;

  /// Best-effort hook invoked after a full feed refresh (offset 0). Set by
  /// [SyncService.attach] to trigger peer reconciliation; failures are silent.
  void Function()? onRefreshed;

  List<FeedPost> get posts => _posts;
  List<FeedPost> get displayPosts => _ranked ? _rankedPosts : _posts;
  bool get isRanked => _ranked;
  bool get isLoading => _isLoading;
  List<String> get pinnedPosts => _pinnedView;

  /// Rebuild the cached pinned view after [Set] mutation.
  void _rebuildPinnedView() {
    _pinnedView = List.unmodifiable(_pinned.toList());
  }

  /// Fetch feed events with pagination (supports cursor or offset)
  Future<List<FeedPost>> fetchFeed(
      {int limit = 20, int offset = 0, int? cursorCreatedAt}) async {
    try {
      _isLoading = true;

      final options = jsonEncode({
        'limit': limit,
        'offset': offset,
        'filter_type': 'all',
        if (cursorCreatedAt != null) 'cursor_created_at': cursorCreatedAt,
      });
      final json = RustLib.instance.api
          .crateFfiFeedFeedFetchEvents(optionsJson: options);
      final newPosts = await _decodePosts(json);
      if (offset == 0 && cursorCreatedAt == null) {
        _posts = newPosts;
        _ranked = false;
        _rankedPosts = [];
        _reconcileAfterRefresh();
      } else {
        _posts.addAll(newPosts);
        if (_posts.length > 100) {
          _posts = _posts.sublist(_posts.length - 100);
        }
      }
      clearLastError();
      _currentOffset = offset;
    } catch (e, st) {
      setLastError(e, st);
      rethrow;
    } finally {
      _isLoading = false;
      notifyDeferred();
    }

    return _posts;
  }

  /// Fetch a windowed slice of feed items directly from Rust
  Future<List<FeedPost>> fetchWindow(
      {int startIndex = 0, int limit = 20}) async {
    try {
      _isLoading = true;

      final json = RustLib.instance.api.crateFfiFeedFeedFetchWindow(
        startIndex: startIndex,
        limit: limit,
      );
      _posts = await _decodePosts(json);
      _ranked = false;
      _rankedPosts = [];
      clearLastError();
      _currentOffset = startIndex;
    } catch (e, st) {
      setLastError(e, st);
      rethrow;
    } finally {
      _isLoading = false;
      notifyDeferred();
    }

    return _posts;
  }

  /// Enqueue post into Rust offline outbox queue for optimistic posting.
  /// The payload content is zstd-dict compressed before enqueue.
  Future<String> enqueueOutboxPost(String content, {String? mediaPath}) async {
    try {
      final payload = jsonEncode({'content': compressJson(content), 'kind': 1});
      final id = RustLib.instance.api.crateFfiSyncSyncEnqueueOutbox(
        actionType: 'post',
        payloadJson: payload,
        mediaPath: mediaPath,
      );
      notifyDeferred();
      return id;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Load more posts for infinite scroll
  Future<void> loadMore({int limit = 20}) async {
    if (_loadingMore) return;
    _loadingMore = true;
    try {
      final lastCreatedAt = _posts.isNotEmpty ? _posts.last.createdAt : null;
      final offset = _currentOffset + limit;
      await fetchFeed(
          limit: limit, offset: offset, cursorCreatedAt: lastCreatedAt);
    } finally {
      _loadingMore = false;
    }
  }

  /// Load pinned post ids from the `pinned_posts` settings key (JSON list).
  Future<List<String>> loadPinnedPosts() async {
    try {
      final raw = RustLib.instance.api.crateFfiDbDbGetSetting(key: _pinnedKey);
      _pinned.clear();
      _rebuildPinnedView();
      if (raw != null && raw.isNotEmpty) {
        _pinned.addAll((jsonDecode(raw) as List<dynamic>).whereType<String>());
        _rebuildPinnedView();
      }
      _pinnedLoaded = true;
      clearLastError();
    } catch (e, st) {
      setLastError(e, st);
      _pinned.clear();
      _pinnedLoaded = true;
    }
    notifyDeferred();
    return pinnedPosts;
  }

  /// Whether the given event id is currently pinned.
  bool isPinned(String eventId) => _pinned.contains(eventId);

  /// Add/remove an event id in the settings-backed pinned list.
  Future<bool> togglePin(String eventId) async {
    try {
      if (!_pinnedLoaded) {
        await loadPinnedPosts();
      }
      if (_pinned.contains(eventId)) {
        _pinned.remove(eventId);
      } else {
        _pinned.add(eventId);
      }
      _rebuildPinnedView();
      RustLib.instance.api.crateFfiDbDbSetSetting(
        key: _pinnedKey,
        value: jsonEncode(_pinned.toList()),
      );
      clearLastError();
      notifyDeferred();
      return _pinned.contains(eventId);
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Publish a text note (kind 1)
  Future<String> publishTextNote(
    String content,
    List<List<String>> tags, [
    String? signerPubkey,
  ]) async {
    try {
      final eventId =
          await RustLib.instance.api.crateFfiFeedFeedPublishTextNote(
        content: content,
        tagsJson: jsonEncode(tags),
      );
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Validate note content before publish (sync FFI).
  bool validateNote(String content) {
    try {
      return RustLib.instance.api
          .crateFfiFeedFeedValidateNote(content: content);
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return false;
    }
  }

  /// Extract hashtags from draft content (sync FFI).
  List<String> extractHashtags(String text) {
    try {
      return RustLib.instance.api.crateFfiUtilUtilExtractHashtags(text: text);
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return const [];
    }
  }

  /// Publish a reply to an event
  Future<String> publishReply(
    String content,
    String rootEventId,
    String replyToEventId, [
    String? signerPubkey,
  ]) async {
    try {
      final eventId = await RustLib.instance.api.crateFfiFeedFeedPublishReply(
        content: content,
        rootEventId: rootEventId,
        replyToEventId: replyToEventId,
      );
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Fetch a thread (root post + replies)
  Future<List<FeedPost>> fetchThread(String eventId) async {
    try {
      final json = RustLib.instance.api.crateFfiFeedFeedFetchThread(
        eventId: eventId,
      );
      return await _decodePosts(json);
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Create a reaction (like/repost/zap)
  Future<String> createReaction(
    String eventId,
    String reactionType,
    String signerPubkey,
  ) async {
    try {
      return await RustLib.instance.api.crateFfiFeedFeedCreateReaction(
        eventId: eventId,
        reactionType: reactionType,
      );
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Delete a post
  Future<String> deletePost(String eventId, String signerPubkey) async {
    try {
      final result = await RustLib.instance.api
          .crateFfiFeedFeedDeletePost(eventId: eventId);
      try {
        RustLib.instance.api.crateFfiSearchSearchRemoveIndexed(
          id: eventId,
        );
      } catch (e) {
        debugPrint('unindex: $e');
      }
      return result;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Compress a JSON payload into a base64 zstd-dict frame (sync FFI).
  String compressJson(String payload) {
    try {
      return RustLib.instance.api
          .crateFfiContentContentCompressJsonDict(data: payload);
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return payload;
    }
  }

  /// True when [payload] looks like a base64-encoded compressed frame.
  /// zstd-dict frames start with magic bytes `5A 48 01 …` → base64 `WkgB`;
  /// plain deflate starts with `78 9C/01/DA/5E` → `eJw/eAE/eNo/eF4`. The
  /// cheap prefix check avoids a base64 decode + decompress FFI round-trip
  /// per post on the decode path.
  static bool _looksCompressed(String payload) {
    if (payload.length < 12) return false;
    return payload.startsWith('WkgB') ||
        payload.startsWith('eJw') ||
        payload.startsWith('eAE') ||
        payload.startsWith('eNo') ||
        payload.startsWith('eF4');
  }

  /// Decompress a base64 zstd-dict frame produced by [compressJson]; returns
  /// the input unchanged when it isn't a valid compressed payload.
  String decompressJson(String payload) {
    if (payload.isEmpty || !_looksCompressed(payload)) return payload;
    try {
      final decoded = RustLib.instance.api
          .crateFfiContentContentDecompressJsonDict(encoded: payload);
      if (decoded.isEmpty || decoded == payload) return payload;
      return decoded;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return payload;
    }
  }

  /// Rank local posts via feed-core and swap the displayed order.
  /// [postsJson] is an array of `{stats: {...}, hashtags: []}` entries.
  Future<List<FeedPost>> rankPosts(String postsJson) async {
    try {
      final json = await RustLib.instance.api
          .crateFfiFeedFeedRankPosts(eventsJson: postsJson);
      final indices = (jsonDecode(json) as List<dynamic>)
          .map((e) => (e as num).toInt())
          .toList();
      _rankedPosts = [
        for (final i in indices)
          if (i >= 0 && i < _posts.length) _posts[i],
      ];
      _ranked = true;
      clearLastError();
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
    notifyDeferred();
    return _rankedPosts;
  }

  /// Toggle ranked vs chronological feed ordering.
  Future<void> toggleRanking() async {
    if (_ranked) {
      _ranked = false;
      _rankedPosts = [];
      notifyDeferred();
      return;
    }
    await rankPosts(jsonEncode([
      for (final p in _posts)
        {
          'stats': {
            'created_at_secs': p.createdAt.toDouble(),
            'likes_count': p.reactions,
            'replies_count': p.replies,
            'zaps_count': 0,
            'reposts_count': p.reposts,
            'wot_distance': 0,
          },
          'hashtags': extractHashtags(p.content),
        },
    ]));
  }

  /// Aggregate per-emoji reaction counts for the given thread posts
  /// (feed-core aggregator; sync FFI).
  List<ReactionSummary> aggregateChatReactions(
      List<FeedPost> threadPosts, String selfPubkey) {
    final reactions = <Map<String, dynamic>>[];
    for (final p in threadPosts) {
      for (var i = 0; i < p.reactions; i++) {
        reactions.add({
          'emoji': '+',
          'reactorPubkey': p.liked ? selfPubkey : '',
        });
      }
    }
    final input =
        jsonEncode({'reactions': reactions, 'selfPubkey': selfPubkey});
    try {
      final json = RustLib.instance.api
          .crateFfiFeedFeedAggregateChatReactions(input: input);
      final list = jsonDecode(json) as List<dynamic>;
      return list
          .map((e) => ReactionSummary.fromJson(e as Map<String, dynamic>))
          .toList();
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      return const [];
    }
  }

  /// Queue a stored post for FTS indexing; flush runs off the load path.
  void _indexPost(FeedPost post) {
    if (post.eventId.isEmpty) return;
    _pendingIndex.add(post);
    if (_indexFlushScheduled) return;
    _indexFlushScheduled = true;
    Future.microtask(_flushIndexQueue);
  }

  void _flushIndexQueue() {
    _indexFlushScheduled = false;
    final pending = List<FeedPost>.from(_pendingIndex);
    _pendingIndex.clear();
    if (pending.isEmpty) return;
    try {
      final rowsJson = jsonEncode([
        for (final post in pending)
          {
            'id': post.eventId,
            'pubkey': post.pubkey,
            'content': post.content,
            'kind': 1,
          }
      ]);
      RustLib.instance.api.crateFfiSearchSearchIndexPosts(rowsJson: rowsJson);
    } catch (_) {
      // Indexing is best-effort; a failed upsert must not break ingest.
    }
  }

  /// Decode feed rows: JSON parsing happens on a background isolate
  /// ([_parseFeedRowsStatic]); decompress (FFI, main-isolate only) + [FeedPost]
  /// construction happen here.
  Future<List<FeedPost>> _decodePosts(String json) async {
    // Don't capture 'this' in the isolate - use a static function instead
    final rows = await runOffThread(() => _parseFeedRowsStatic(json));
    final posts = <FeedPost>[];
    for (final m in rows) {
      final content = m['content'] as String? ?? '';
      if (content.isNotEmpty) {
        final decoded = decompressJson(content);
        if (decoded != content) m['content'] = decoded;
      }
      posts.add(FeedPost.fromJson(m));
    }
    return posts;
  }

  /// JSON → raw feed rows, top-level so [compute] can run it on a background
  /// isolate (keeps the per-post [FeedPost] build + FFI decompress on main).
  /// Static version to avoid capturing 'this' (which contains onRefreshed)
  /// when running on an isolate via runOffThread/compute.
  static List<Map<String, dynamic>> _parseFeedRowsStatic(String json) {
    return (jsonDecode(json) as List<dynamic>)
        .map((e) => Map<String, dynamic>.from(e as Map))
        .toList();
  }

  /// Fire the post-refresh reconcile hook; sync is best-effort so failures
  /// are swallowed here.
  void _reconcileAfterRefresh() {
    try {
      onRefreshed?.call();
    } catch (_) {
      // Best-effort; a failed reconcile must not break the refresh path.
    }
  }

  /// Insert a post arriving from the live sync stream (dedup by event id and filter moderated content).
  void insertLivePost(FeedPost post) {
    if (post.eventId.isEmpty) return;
    if (!validateNote(post.content)) return;
    if (_posts.any((p) => p.eventId == post.eventId)) return;
    _posts.insert(0, post);
    _indexPost(post);
    notifyDeferred();
  }

  /// Apply a live reaction to a cached post (bump the counter if known).
  void applyLiveReaction(String eventId, String pubkey, String content) {
    final index = _posts.indexWhere((p) => p.eventId == eventId);
    if (index < 0) return;
    final p = _posts[index];
    final liked = p.liked || content == '+';
    _posts[index] = FeedPost(
      eventId: p.eventId,
      pubkey: p.pubkey,
      content: p.content,
      createdAt: p.createdAt,
      reactions: p.reactions + 1,
      replies: p.replies,
      reposts: p.reposts,
      liked: liked,
      profileName: p.profileName,
      profilePicture: p.profilePicture,
      media: p.media,
    );
    notifyDeferred();
  }

  /// Deletes locally stored posts older than `cutoffSecs`; rows removed.
  Future<int> dbDeleteOlderThan(int cutoffSecs) async {
    try {
      final n = RustLib.instance.api
          .crateFfiDbDbDeleteOlderThan(cutoffSecs: cutoffSecs);
      clearLastError();
      return n.toInt();
    } catch (e, st) {
      setLastError(e, st);
      rethrow;
    }
  }

  /// Deletes every locally stored post; rows removed.
  Future<int> dbDeleteAllPosts() async {
    try {
      final n = RustLib.instance.api.crateFfiDbDbDeleteAllPosts();
      clearLastError();
      return n.toInt();
    } catch (e, st) {
      setLastError(e, st);
      rethrow;
    }
  }
}

/// Feed post (kind 1 / reply) as stored locally.
class FeedPost {
  final String eventId;
  final String pubkey;
  final String content;
  final int createdAt;
  final int reactions;
  final int replies;
  final int reposts;
  final bool liked;
  final String? profileName;
  final String? profilePicture;
  final PostMedia? media;

  FeedPost({
    required this.eventId,
    required this.pubkey,
    required this.content,
    required this.createdAt,
    required this.reactions,
    required this.replies,
    required this.reposts,
    required this.liked,
    this.profileName,
    this.profilePicture,
    this.media,
  });

  factory FeedPost.fromJson(Map<String, dynamic> json) {
    return FeedPost(
      eventId: (json['event_id'] ?? json['id']) as String? ?? '',
      pubkey: json.strOf('pubkey'),
      content: json.strOf('content'),
      createdAt: json.intOf('created_at'),
      reactions: json.intOf('reactions'),
      replies: json.intOf('replies'),
      reposts: json.intOf('reposts'),
      liked: json.boolOf('liked'),
      profileName: json.strOrNull('profile_name'),
      profilePicture: json.strOrNull('profile_picture'),
      media: _parseMedia(json),
    );
  }

  /// `media_json` (LAN blob-sharing pipeline) or legacy `media` object.
  static PostMedia? _parseMedia(Map<String, dynamic> json) {
    final raw = json['media_json'];
    if (raw is String && raw.isNotEmpty) {
      try {
        return PostMedia.fromJson(jsonDecode(raw) as Map<String, dynamic>);
      } catch (_) {
        return null;
      }
    }
    return json['media'] != null
        ? PostMedia.fromJson(json['media'] as Map<String, dynamic>)
        : null;
  }
}

/// Media attachment for a feed post (parsed from content tags).
class PostMedia {
  final String url;
  final String type; // 'video', 'image', 'audio'
  final String blobHash;
  final int size;

  PostMedia({
    required this.url,
    required this.type,
    required this.blobHash,
    required this.size,
  });

  factory PostMedia.fromJson(Map<String, dynamic> json) {
    return PostMedia(
      url: json.strOf('url'),
      type: json.strOrNull('type') ?? 'image',
      blobHash: json.strOf('blob_hash'),
      size: json.intOf('size'),
    );
  }
}

/// Per-emoji reaction aggregate produced by `aggregate_chat_reactions`.
class ReactionSummary {
  final String emoji;
  final int count;
  final bool hasReacted;

  ReactionSummary({
    required this.emoji,
    required this.count,
    required this.hasReacted,
  });

  factory ReactionSummary.fromJson(Map<String, dynamic> json) {
    return ReactionSummary(
      emoji: json.strOrNull('emoji') ?? '+',
      count: json.intOf('count'),
      hasReacted: json.boolOf('hasReacted'),
    );
  }
}
