// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

/// Feed Service
/// Handles feed operations and post publishing
class FeedService extends ChangeNotifier {
  List<FeedPost> _posts = [];
  bool _isLoading = false;
  String? _lastError;
  int _currentOffset = 0;
  static const String _pinnedKey = 'pinned_posts';
  final Set<String> _pinned = {};
  bool _pinnedLoaded = false;

  List<FeedPost> get posts => _posts;
  bool get isLoading => _isLoading;
  String? get lastError => _lastError;
  List<String> get pinnedPosts => List.unmodifiable(_pinned.toList());

  /// Fetch feed events with pagination
  Future<List<FeedPost>> fetchFeed({int limit = 20, int offset = 0}) async {
    try {
      _isLoading = true;

      final options = jsonEncode({
        'limit': limit,
        'offset': offset,
        'filter_type': 'all',
      });
      final json = RustLib.instance.api
          .crateFfiFeedFeedFetchEvents(optionsJson: options);
      final newPosts = _decodePosts(json);
      if (offset == 0) {
        _posts = newPosts;
      } else {
        _posts.addAll(newPosts);
        if (_posts.length > 100) {
          _posts = _posts.sublist(_posts.length - 100);
        }
      }
      _lastError = null;
      _currentOffset = offset;
    } catch (e) {
      _lastError = e.toString();
      rethrow;
    } finally {
      _isLoading = false;
      notifyListeners();
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
      _posts = _decodePosts(json);
      _lastError = null;
      _currentOffset = startIndex;
    } catch (e) {
      _lastError = e.toString();
      rethrow;
    } finally {
      _isLoading = false;
      notifyListeners();
    }

    return _posts;
  }

  /// Enqueue post into Rust offline outbox queue for optimistic posting
  Future<String> enqueueOutboxPost(String content, {String? mediaPath}) async {
    try {
      final payload = jsonEncode({'content': content, 'kind': 1});
      final id = RustLib.instance.api.crateFfiSyncSyncEnqueueOutbox(
        actionType: 'post',
        payloadJson: payload,
        mediaPath: mediaPath,
      );
      notifyListeners();
      return id;
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Load more posts for infinite scroll
  Future<void> loadMore({int limit = 20}) async {
    await fetchFeed(limit: limit, offset: _currentOffset + limit);
  }

  /// Load pinned post ids from the `pinned_posts` settings key (JSON list).
  Future<List<String>> loadPinnedPosts() async {
    try {
      final raw = RustLib.instance.api.crateFfiDbDbGetSetting(key: _pinnedKey);
      _pinned.clear();
      if (raw != null && raw.isNotEmpty) {
        _pinned.addAll((jsonDecode(raw) as List<dynamic>).whereType<String>());
      }
      _pinnedLoaded = true;
      _lastError = null;
    } catch (e) {
      _lastError = e.toString();
      _pinned.clear();
      _pinnedLoaded = true;
    }
    notifyListeners();
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
      RustLib.instance.api.crateFfiDbDbSetSetting(
        key: _pinnedKey,
        value: jsonEncode(_pinned.toList()),
      );
      _lastError = null;
      notifyListeners();
      return _pinned.contains(eventId);
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
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
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
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
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  /// Fetch a thread (root post + replies)
  Future<List<FeedPost>> fetchThread(String eventId) async {
    try {
      final json = RustLib.instance.api.crateFfiFeedFeedFetchThread(
        eventId: eventId,
      );
      return _decodePosts(json);
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
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
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
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
    } catch (e) {
      _lastError = e.toString();
      notifyListeners();
      rethrow;
    }
  }

  static List<FeedPost> _decodePosts(String json) {
    final list = jsonDecode(json) as List<dynamic>;
    return list
        .map((e) => FeedPost.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  /// Insert a post arriving from the live sync stream (dedup by event id).
  void insertLivePost(FeedPost post) {
    if (post.eventId.isEmpty) return;
    if (_posts.any((p) => p.eventId == post.eventId)) return;
    _posts.insert(0, post);
    notifyListeners();
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
    notifyListeners();
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
      pubkey: json['pubkey'] as String? ?? '',
      content: json['content'] as String? ?? '',
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
      reactions: (json['reactions'] as num?)?.toInt() ?? 0,
      replies: (json['replies'] as num?)?.toInt() ?? 0,
      reposts: (json['reposts'] as num?)?.toInt() ?? 0,
      liked: json['liked'] as bool? ?? false,
      profileName: json['profile_name'] as String?,
      profilePicture: json['profile_picture'] as String?,
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
      url: json['url'] as String? ?? '',
      type: json['type'] as String? ?? 'image',
      blobHash: json['blob_hash'] as String? ?? '',
      size: (json['size'] as num?)?.toInt() ?? 0,
    );
  }
}
