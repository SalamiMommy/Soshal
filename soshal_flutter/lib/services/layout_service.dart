// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:soshal_flutter/frb_generated.dart';

import '../ffi/raster.dart';
import 'feed_service.dart';

/// Per-post layout metadata computed in Rust, used by the feed list's
/// [itemExtentBuilder] to keep scroll metrics stable and deterministic.
class LayoutService extends ChangeNotifier {
  static const _fontSizePx = 14.0;
  static const _lineHeightFactor = 1.4;
  static const _headerPx = 48.0;
  static const _actionPx = 40.0;
  static const _paddingPx = 16.0;
  static const _gapPx = 8.0;

  bool _ready = false;
  int _screenWidth = 360;
  double _textScale = 1.0;

  /// event_id -> total card height in px (single source of truth)
  final Map<String, double> _heights = {};

  /// event_id -> media block height in px
  final Map<String, double> _mediaHeights = {};

  /// Posts from the last [refresh] — reused by [updateViewMetrics].
  List<FeedPost>? _lastPosts;

  bool get ready => _ready;
  Map<String, double> get heights => Map.unmodifiable(_heights);

  Map<String, Object> _requestFor(FeedPost p) => {
        'id': p.eventId,
        'text': {
          'content': p.content,
          'font_size_px': _fontSizePx * _textScale,
          'line_height_factor': _lineHeightFactor,
          'max_width_px':
              (_screenWidth - 16).toDouble(), // card margins + padding
          'bold': false,
        },
        'media': <Map<String, Object>>[],
        'chrome': {
          'header_px': _headerPx,
          'action_px': _actionPx,
          'padding_px': _paddingPx,
          'gap_px': _gapPx,
          'max_media_height_px': 480.0,
        },
      };

  /// Compute the layout of a single card via feed-core (sync FFI).
  (double, double)? _computeCardLayout(FeedPost post) {
    try {
      final json = RustLib.instance.api.crateFfiFeedFeedComputeCardLayout(
        requestJson: jsonEncode(_requestFor(post)),
      );
      final m = jsonDecode(json) as Map<String, dynamic>;
      final id = m['id'] as String? ?? '';
      if (id.isEmpty) return null;
      final height = (m['height_px'] as num?)?.toDouble() ?? 0;
      final media = (m['media_height_px'] as num?)?.toDouble() ?? 0;
      return (height, media);
    } catch (e) {
      debugPrint('card layout: $e');
      return null;
    }
  }

  /// Feed cards: compute all layouts for the visible posts in one call.
  Future<void> refresh(
    List<FeedPost> posts, {
    int screenWidth = 360,
    double textScale = 1.0,
  }) async {
    _screenWidth = screenWidth;
    _textScale = textScale;
    _lastPosts = posts;
    final requests = posts
        .where((p) => !_heights.containsKey(p.eventId))
        .map((p) => _requestFor(p))
        .toList();
    if (requests.isEmpty) {
      _lastPosts = posts;
      _ready = true;
      return; // nothing new to lay out
    }
    try {
      final json = RustLib.instance.api.crateFfiFeedFeedComputeCardLayouts(
          requestsJson: jsonEncode(requests));
      final results = jsonDecode(json) as List<dynamic>;
      _heights.clear();
      _mediaHeights.clear();
      for (final r in results) {
        final m = r as Map<String, dynamic>;
        final id = m['id'] as String? ?? '';
        if (id.isEmpty) continue;
        _heights[id] = (m['height_px'] as num?)?.toDouble() ?? 0;
        _mediaHeights[id] = (m['media_height_px'] as num?)?.toDouble() ?? 0;
      }
      _ready = true;
      notifyListeners();
    } catch (e) {
      debugPrint('layout refresh: $e');
    }
  }

  /// Height for a post card, or null when not computed yet (natural layout).
  /// Falls back to a per-card [computeCardLayout] call for uncached posts.
  double? heightFor(FeedPost post) {
    final cached = _heights[post.eventId];
    if (cached != null) return cached;
    final result = _computeCardLayout(post);
    if (result != null) {
      _heights[post.eventId] = result.$1;
      _mediaHeights[post.eventId] = result.$2;
      _ready = true;
      notifyListeners();
      return result.$1;
    }
    return null;
  }

  /// True when any [posts] card lacks a cached extent — a structural change
  /// (new ids) that needs a [refresh] before the next render.
  bool needsLayout(List<FeedPost> posts) =>
      posts.any((p) => !_heights.containsKey(p.eventId));

  /// Extent for ListView.itemExtentBuilder — MUST never return null
  /// (the framework null-checks the result). Footer index gets a fixed
  /// extent; uncached posts fall back to a per-card compute, then a
  /// default when the compute fails.
  static const double _footerExtent = 56.0;
  static const double _defaultCardExtent = 180.0;

  double extentFor(int index, List<FeedPost> posts) {
    if (index >= posts.length) return _footerExtent;
    final cached = _heights[posts[index].eventId];
    if (cached != null) return cached;
    return heightFor(posts[index]) ?? _defaultCardExtent;
  }

  void updateViewMetrics({required int screenWidth, double textScale = 1.0}) {
    _screenWidth = screenWidth;
    _textScale = textScale;
    final posts = _lastPosts;
    if (posts == null) return;
    final requests = posts.map((p) => _requestFor(p)).toList();
    try {
      final json = RustLib.instance.api.crateFfiFeedFeedComputeCardLayouts(
          requestsJson: jsonEncode(requests));
      final results = jsonDecode(json) as List<dynamic>;
      for (final r in results) {
        final m = r as Map<String, dynamic>;
        final id = m['id'] as String? ?? '';
        if (id.isEmpty) continue;
        _heights[id] = (m['height_px'] as num?)?.toDouble() ?? 0;
        _mediaHeights[id] = (m['media_height_px'] as num?)?.toDouble() ?? 0;
      }
    } catch (e) {
      debugPrint('layout metrics: $e');
    }
    if (_heights.isNotEmpty) _ready = true;
    notifyListeners();
  }

  String createRenderSession({required int width, required int height}) =>
      RustLib.instance.api.crateFfiRenderRenderCreateSession(
        width: width,
        height: height,
      );

  Future<Uint8List> renderMeshFrame({
    required int sessionId,
    required String nodesJson,
    required double deltaTime,
  }) async =>
      await RustLib.instance.api.crateFfiRenderRenderComputeMeshFrame(
        sessionId: PlatformInt64Util.from(sessionId),
        nodesJson: nodesJson,
        deltaTime: deltaTime,
      );

  Future<ImpellerFrameBufferInfo> allocateRasterFrameBuffer({
    required int width,
    required int height,
  }) =>
      RustLib.instance.api.crateFfiRasterRasterAllocateFrameBuffer(
        width: width,
        height: height,
      );

  Future<bool> signalRasterFrameReady({
    required int textureId,
    required BigInt frameTimestampNs,
  }) =>
      RustLib.instance.api.crateFfiRasterRasterSignalImpellerFrameReady(
        textureId: PlatformInt64Util.from(textureId),
        frameTimestampNs: frameTimestampNs,
      );
}
