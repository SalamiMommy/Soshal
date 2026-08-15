// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/ffi/p2p.dart' as moq;
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Streaming Service
/// Live stream + story rows (kind 30311 / 30312 posts table entries).
class StreamingService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify {
  final List<StreamRow> _live = [];
  final List<StreamRow> _stories = [];

  int? _localVideoServerPort;

  String? _activeMoqStreamId;
  int _moqGroupCounter = 0;

  List<StreamRow> get live => _live;
  List<StreamRow> get stories => _stories;
  int? get localVideoServerPort => _localVideoServerPort;

  /// Own broadcast stream id, set while `startMoqBroadcast` is active.
  String? get activeMoqStreamId => _activeMoqStreamId;
  bool get isBroadcasting => _activeMoqStreamId != null;

  /// Monotonic group sequence for the local broadcast (per session).
  int nextMoqGroupSeq() => _moqGroupCounter++;

  /// Builds a single-object video group (JPEG keyframe, track 0) for
  /// `publishLiveGroup`. `jpeg` bytes are the encoded frame payload.
  Map<String, dynamic> buildVideoGroup({
    required int groupSeq,
    required int timestampMs,
    required List<int> jpeg,
  }) {
    return {
      'group_sequence': groupSeq,
      'objects': [
        {
          'header': {
            'track_id': 0,
            'group_sequence': groupSeq,
            'object_sequence': 0,
            'payload_size': jpeg.length,
            'track_type': 'VideoKeyframe',
            'timestamp_ms': timestampMs,
          },
          'payload': jpeg,
        },
      ],
    };
  }

  /// Builds a single-object H.264 group (track 1) for `publishLiveGroup`.
  /// `nal` is an Annex-B NAL blob (as produced by the hardware encoder);
  /// `keyframe` selects the VideoKeyframe/VideoDelta track type so viewers
  /// know whether decoding can start from this group.
  Map<String, dynamic> buildH264Group({
    required int groupSeq,
    required int timestampMs,
    required List<int> nal,
    required bool keyframe,
  }) {
    return {
      'group_sequence': groupSeq,
      'objects': [
        {
          'header': {
            'track_id': 1,
            'group_sequence': groupSeq,
            'object_sequence': 0,
            'payload_size': nal.length,
            'track_type': keyframe ? 'VideoKeyframe' : 'VideoDelta',
            'timestamp_ms': timestampMs,
          },
          'payload': nal,
        },
      ],
    };
  }

  /// Builds a single-object audio group (track 2, `AudioDatagram`) for
  /// `publishLiveGroup`. `aac` is one native AAC-LC blob — `[tag, ...aac]`
  /// where tag `2` = codec config and `1` = audio frame (viewers gate decode
  /// on the config object); `config` mirrors the tag in the MoQ header.
  Map<String, dynamic> buildAudioGroup({
    required int groupSeq,
    required int timestampMs,
    required List<int> aac,
    bool config = false,
  }) {
    return {
      'group_sequence': groupSeq,
      'objects': [
        {
          'header': {
            'track_id': 2,
            'group_sequence': groupSeq,
            'object_sequence': 0,
            'payload_size': aac.length,
            'track_type': 'AudioDatagram',
            'timestamp_ms': timestampMs,
          },
          'payload': aac,
        },
      ],
    };
  }

  /// Initialize local Rust video proxy HTTP micro-server.
  Future<int> initLocalVideoServer() async {
    try {
      final port = await RustLib.instance.api
          .crateFfiStreamingStreamingStartLocalServer();
      _localVideoServerPort = port;
      clearLastError();
      notifyDeferred();
      return port;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Get local video proxy URL for a registered video.
  String getLocalVideoUrl(String videoId, String sourcePath) {
    try {
      return RustLib.instance.api.crateFfiStreamingStreamingGetVideoUrl(
        videoId: videoId,
        sourcePath: sourcePath,
      );
    } catch (e, st) {
      setLastError(e, st);
      rethrow;
    }
  }

  Future<List<StreamRow>> fetchLive({int limit = 50}) async {
    return _decode(
      () => RustLib.instance.api
          .crateFfiStreamingStreamingFetchLive(limit: limit),
    );
  }

  Future<List<StreamRow>> fetchFollowedLive(String userPubkey) async {
    return _decode(
      () => RustLib.instance.api
          .crateFfiStreamingStreamingFetchFollowedLive(userPubkey: userPubkey),
    );
  }

  Future<String> startLive(
    String broadcasterPubkey,
    String title,
    String description,
    String streamUrl,
  ) async {
    try {
      final eventId = RustLib.instance.api.crateFfiStreamingStreamingStartLive(
        broadcasterPubkey: broadcasterPubkey,
        title: title,
        description: description,
        streamUrl: streamUrl,
      );
      clearLastError();
      notifyDeferred();
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> endLive(String streamId, String broadcasterPubkey) async {
    try {
      final ok = RustLib.instance.api.crateFfiStreamingStreamingEndLive(
        streamId: streamId,
        broadcasterPubkey: broadcasterPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<String> postStory(
    String authorPubkey,
    String content,
    List<String> images,
    int expiresInHours,
  ) async {
    try {
      final eventId = RustLib.instance.api.crateFfiStreamingStreamingPostStory(
        authorPubkey: authorPubkey,
        content: content,
        imagesJson: jsonEncode(images),
        expiresInHours: expiresInHours,
      );
      clearLastError();
      notifyDeferred();
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<List<StreamRow>> fetchStories(String userPubkey) async {
    return _decode(
      () => RustLib.instance.api.crateFfiStreamingStreamingFetchStories(
        userPubkey: userPubkey,
      ),
    );
  }

  Future<List<StreamRow>> fetchFollowedStories(String viewerPubkey) async {
    return _decode(
      () => RustLib.instance.api.crateFfiStreamingStreamingFetchFollowedStories(
        viewerPubkey: viewerPubkey,
      ),
    );
  }

  Future<bool> markStoryViewed(String storyId, String viewerPubkey) async {
    try {
      final ok = RustLib.instance.api.crateFfiStreamingStreamingMarkStoryViewed(
        storyId: storyId,
        viewerPubkey: viewerPubkey,
      );
      clearLastError();
      notifyDeferred();
      return ok;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  Future<bool> storyReact(String storyId, String pubkey, String emoji) async {
    try {
      final ok = RustLib.instance.api.crateFfiStreamingStreamingStoryReact(
        storyId: storyId,
        pubkey: pubkey,
        emoji: emoji,
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

  /// Publish a video/audio frame via Media over QUIC (MoQ).
  Future<String> publishMoqObject({
    required String streamId,
    required String publisherPubkey,
    required int trackId,
    required bool isKeyframe,
    required String payloadHex,
  }) async {
    try {
      final json =
          RustLib.instance.api.crateFfiStreamingStreamingMoqPublishObject(
        streamId: streamId,
        publisherPubkey: publisherPubkey,
        trackId: trackId,
        isKeyframe: isKeyframe,
        payloadHex: payloadHex,
      );
      clearLastError();
      notifyDeferred();
      return json;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Subscribe to a Media over QUIC (MoQ) P2P media stream.
  Future<String> subscribeMoqStream({
    required String streamId,
    required String subscriberPubkey,
  }) async {
    try {
      final status =
          RustLib.instance.api.crateFfiStreamingStreamingMoqSubscribeStream(
        streamId: streamId,
        subscriberPubkey: subscriberPubkey,
      );
      clearLastError();
      notifyDeferred();
      return status;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Encode a `MoqGroup` (JSON map) into on-stream binary framing.
  Uint8List encodeMoqGroup(Map<String, dynamic> group) {
    return moq.p2PMoqEncodeGroup(groupJson: jsonEncode(group));
  }

  /// Decode an encoded MoQ group frame back to a `MoqGroup` JSON map.
  Map<String, dynamic> decodeMoqGroup(List<int> frame) {
    return jsonDecode(moq.p2PMoqDecodeGroup(bytes: frame))
        as Map<String, dynamic>;
  }

  /// Publish one encoded MoQ group into the local live registry. Subscribers
  /// on the LAN pull it via `subscribeLiveFetch` (QUIC stream). Returns the
  /// publish status JSON; `groups` is the monotonic group counter.
  Future<Map<String, dynamic>> publishLiveGroup({
    required String streamId,
    required Map<String, dynamic> group,
  }) async {
    try {
      final encoded = encodeMoqGroup(group);
      final json = moq.p2PMoqPublishGroup(streamId: streamId, encoded: encoded);
      clearLastError();
      notifyDeferred();
      return jsonDecode(json) as Map<String, dynamic>;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  /// Open a MoQ broadcast for `streamId`: publishes a bootstrap control
  /// group (metadata object, track 0) so LAN peers see the stream as live in
  /// the registry, then marks this service as broadcasting. Media groups
  /// follow via `publishLiveGroup`.
  Future<void> startMoqBroadcast({
    required String streamId,
    required String title,
  }) async {
    await publishLiveGroup(
      streamId: streamId,
      group: _controlGroup(
        groupSeq: 0,
        timestampMs: DateTime.now().millisecondsSinceEpoch,
        payload: utf8.encode('{"title":"$title","start":true}'),
      ),
    );
    _activeMoqStreamId = streamId;
    _moqGroupCounter = 0;
    notifyDeferred();
  }

  /// End the local MoQ broadcast (publishes a stop control group).
  Future<void> stopMoqBroadcast() async {
    final streamId = _activeMoqStreamId;
    _activeMoqStreamId = null;
    if (streamId != null) {
      try {
        await publishLiveGroup(
          streamId: streamId,
          group: _controlGroup(
            groupSeq: 1,
            timestampMs: DateTime.now().millisecondsSinceEpoch,
            payload: utf8.encode('{"start":false}'),
          ),
        );
      } catch (_) {
        // best-effort stop frame
      }
    }
    notifyDeferred();
  }

  static Map<String, dynamic> _controlGroup({
    required int groupSeq,
    required int timestampMs,
    required List<int> payload,
  }) {
    return {
      'group_sequence': groupSeq,
      'objects': [
        {
          'header': {
            'track_id': 0,
            'group_sequence': groupSeq,
            'object_sequence': 0,
            'payload_size': payload.length,
            'track_type': 'VideoKeyframe',
            'timestamp_ms': timestampMs,
          },
          'payload': payload,
        },
      ],
    };
  }

  /// Pull a live MoQ stream from a LAN peer's QUIC server for up to
  /// `windowMs`. Returns decoded `MoqGroup` maps in received order (buffered
  /// replay, then live follow). Throws with "stream not found" / power-denied
  /// errors surfaced from the transport.
  Future<List<Map<String, dynamic>>> subscribeLiveFetch({
    required String addr,
    required String streamId,
    required int windowMs,
  }) async {
    try {
      final json = moq.p2PMoqSubscribeFetch(
        addr: addr,
        streamId: streamId,
        windowMs: BigInt.from(windowMs),
      );
      final parsed = jsonDecode(json) as Map<String, dynamic>;
      final frames = (parsed['groups'] as List<dynamic>? ?? [])
          .map((g) => _hexToBytes(g as String))
          .toList();
      clearLastError();
      notifyDeferred();
      return frames.map(decodeMoqGroup).toList();
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  static List<int> _hexToBytes(String hex) {
    final out = <int>[];
    for (var i = 0; i < hex.length; i += 2) {
      out.add(int.parse(hex.substring(i, i + 2), radix: 16));
    }
    return out;
  }

  Future<List<StreamRow>> _decode(String Function() call) async {
    try {
      final json = call();
      final decoded = jsonDecode(json);
      final parsed = (decoded as List<dynamic>)
          .map((e) => StreamRow.fromJson(e as Map<String, dynamic>))
          .toList();
      clearLastError();
      notifyDeferred();
      return parsed;
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }
}

/// A live stream or story row from the posts table.
class StreamRow {
  final String id;
  final String pubkey;
  final String content;
  final int createdAt;
  final int views;

  final String title;
  final String summary;

  StreamRow({
    required this.id,
    required this.pubkey,
    required this.content,
    required this.createdAt,
    required this.views,
    required this.title,
    required this.summary,
  });

  factory StreamRow.fromJson(Map<String, dynamic> json) {
    final content = json['content'] as String? ?? '';
    String parsedTitle = 'Untitled';
    String parsedSummary = '';

    try {
      final decoded = jsonDecode(content);
      if (decoded is Map<String, dynamic>) {
        parsedTitle = (decoded['title'] as String?) ??
            (decoded['text'] as String?) ??
            'Untitled';
        parsedSummary = (decoded['summary'] as String?) ?? '';
      } else {
        final firstLine = content.trim().split('\n').first;
        parsedTitle = firstLine.isEmpty ? 'Untitled' : firstLine;
        parsedSummary = content.trim();
      }
    } catch (e, st) {
      debugPrint('moq parse fallback: $e');
      logRuntimeError('moq parse fallback: $e', st);
      final firstLine = content.trim().split('\n').first;
      parsedTitle = firstLine.isEmpty ? 'Untitled' : firstLine;
      parsedSummary = content.trim();
    }

    return StreamRow(
      id: json['id'] as String? ?? '',
      pubkey: json['pubkey'] as String? ?? '',
      content: content,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
      views: (json['views'] as num?)?.toInt() ?? 0,
      title: parsedTitle,
      summary: parsedSummary,
    );
  }
}
