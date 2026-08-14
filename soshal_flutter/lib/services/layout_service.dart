// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';

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

  bool get ready => _ready;
  Map<String, double> get heights => Map.unmodifiable(_heights);

  /// Feed cards: compute all layouts for the visible posts in one call.
  Future<void> refresh(
    List<FeedPost> posts, {
    int screenWidth = 360,
    double textScale = 1.0,
  }) async {
    _screenWidth = screenWidth;
    _textScale = textScale;
    final requests = posts
        .map((p) => {
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
            })
        .toList();
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
  double? heightFor(FeedPost post) => _heights[post.eventId];

  /// For ListView.itemExtentBuilder — index beyond posts maps to null
  /// (loading footer sizes naturally).
  double? extentFor(int index, List<FeedPost> posts) {
    if (index >= posts.length) return null;
    return _heights[posts[index].eventId];
  }

  void updateViewMetrics({required int screenWidth, double textScale = 1.0}) {
    _screenWidth = screenWidth;
    _textScale = textScale;
  }
}
