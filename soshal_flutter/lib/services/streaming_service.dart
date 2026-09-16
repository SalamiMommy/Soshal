// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:soshal_flutter/ffi/p2p.dart' as moq;
import 'package:soshal_flutter/frb_generated.dart';
import '../utils/json_ext.dart';
import '../utils/offthread.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Streaming Service
/// Live stream + story rows (kind 30311 / 30312 posts table entries).
class StreamingService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify, ServiceGuard {
  final List<StreamRow> _live = [];
  final List<StreamRow> _stories = [];

  String? _activeMoqStreamId;
  int _moqGroupCounter = 0;
  AppLifecycleListener? _lifecycle;

  List<StreamRow> get live => _live;
  List<StreamRow> get stories => _stories;

  /// Own broadcast stream id, set while `startMoqBroadcast` is active.
  String? get activeMoqStreamId => _activeMoqStreamId;
  bool get isBroadcasting => _activeMoqStreamId != null;

  StreamingService() {
    _lifecycle = AppLifecycleListener(
      onHide: _shutdownOnBackground,
      onPause: _shutdownOnBackground,
    );
  }

  /// Reset in-memory streams and stop active broadcasts on account switch or logout.
  void resetForAccountSwitch() {
    _live.clear();
    _stories.clear();
    _activeMoqStreamId = null;
    _moqGroupCounter = 0;
    clearLastError();
    notifyDeferred();
  }

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

  Future<List<StreamRow>> fetchLive({int limit = 50}) async {
    final parsed = await _decode(
      () => RustLib.instance.api.crateFfiStreamingStreamingFetchLive(
          limit: limit, audience: 'public'),
    );
    _live
      ..clear()
      ..addAll(parsed);
    notifyDeferred();
    return parsed;
  }

  Future<List<StreamRow>> fetchFollowedLive(String userPubkey) async {
    final parsed = await _decode(
      () => RustLib.instance.api
          .crateFfiStreamingStreamingFetchFollowedLive(userPubkey: userPubkey),
    );
    _live
      ..clear()
      ..addAll(parsed);
    notifyDeferred();
    return parsed;
  }

  Future<String> startLive(
    String broadcasterPubkey,
    String title,
    String description,
    String streamUrl,
  ) =>
      guard(() {
        return RustLib.instance.api.crateFfiStreamingStreamingStartLive(
          broadcasterPubkey: broadcasterPubkey,
          title: title,
          description: description,
          streamUrl: streamUrl,
        );
      }, onNotify: notifyDeferred);

  Future<bool> endLive(String streamId, String broadcasterPubkey) => guard(() {
        return RustLib.instance.api.crateFfiStreamingStreamingEndLive(
          streamId: streamId,
          broadcasterPubkey: broadcasterPubkey,
        );
      }, onNotify: notifyDeferred);

  Future<String> postStory(
    String authorPubkey,
    String content,
    List<String> images,
    int expiresInHours,
  ) =>
      guard(() {
        return RustLib.instance.api.crateFfiStreamingStreamingPostStory(
          authorPubkey: authorPubkey,
          content: content,
          imagesJson: jsonEncode(images),
          expiresInHours: expiresInHours,
        );
      }, onNotify: notifyDeferred);

  Future<List<StreamRow>> fetchStories(String userPubkey) async {
    final parsed = await _decode(
      () => RustLib.instance.api.crateFfiStreamingStreamingFetchStories(
        userPubkey: userPubkey,
      ),
    );
    _stories
      ..clear()
      ..addAll(parsed);
    notifyDeferred();
    return parsed;
  }

  Future<List<StreamRow>> fetchFollowedStories(String viewerPubkey, {String audience = 'following'}) async {
    final parsed = await _decode(
      () => RustLib.instance.api.crateFfiStreamingStreamingFetchFollowedStories(
        audience: audience,
      ),
    );
    _stories
      ..clear()
      ..addAll(parsed);
    notifyDeferred();
    return parsed;
  }

  Future<bool> markStoryViewed(String storyId, String viewerPubkey) =>
      guard(() {
        return RustLib.instance.api.crateFfiStreamingStreamingMarkStoryViewed(
          storyId: storyId,
          viewerPubkey: viewerPubkey,
        );
      }, onNotify: notifyDeferred);

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
  }) =>
      guard(() {
        return RustLib.instance.api.crateFfiStreamingStreamingMoqPublishObject(
          streamId: streamId,
          publisherPubkey: publisherPubkey,
          trackId: trackId,
          isKeyframe: isKeyframe,
          payloadHex: payloadHex,
        );
      }, onNotify: notifyDeferred);

  /// Subscribe to a Media over QUIC (MoQ) P2P media stream.
  Future<String> subscribeMoqStream({
    required String streamId,
    required String subscriberPubkey,
  }) =>
      guard(() {
        return RustLib.instance.api
            .crateFfiStreamingStreamingMoqSubscribeStream(
          streamId: streamId,
          subscriberPubkey: subscriberPubkey,
        );
      }, onNotify: notifyDeferred);

  /// Best-effort teardown of a MoQ viewer subscription. network-core keeps no
  /// per-subscriber registry and exposes no unsubscribe FFI — the fetch is
  /// per-window, so this only clears local state so a retry re-subscribes
  /// cleanly.
  Future<void> stopMoqStream() async {
    clearLastError();
    notifyDeferred();
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
  }) =>
      guard(() {
        final encoded = encodeMoqGroup(group);
        final json =
            moq.p2PMoqPublishGroup(streamId: streamId, encoded: encoded);
        return jsonDecode(json) as Map<String, dynamic>;
      }, onNotify: notifyDeferred);

  /// Like `publishLiveGroup` but never notifies consumers — per-frame
  /// broadcast publishing (video/audio groups) must not rebuild service
  /// listeners at 15–25 Hz. Errors still logged via `setLastError`.
  Future<Map<String, dynamic>> publishLiveGroupSilent({
    required String streamId,
    required Map<String, dynamic> group,
  }) =>
      guard(() {
        final encoded = encodeMoqGroup(group);
        final json =
            moq.p2PMoqPublishGroup(streamId: streamId, encoded: encoded);
        return jsonDecode(json) as Map<String, dynamic>;
      }, notifyOnSuccess: false, notifyOnError: false);

  /// Open a MoQ broadcast for `streamId`: publishes a bootstrap control
  /// group (metadata object, track 0) so LAN peers see the stream as live in
  /// the registry, then marks this service as broadcasting. Media groups
  /// follow via `publishLiveGroup`.
  Future<void> startMoqBroadcast({
    required String streamId,
    required String title,
  }) async {
    if (_moqGroupCounter >= 1) _moqGroupCounter = 0;
    await publishLiveGroup(
      streamId: streamId,
      group: _controlGroup(
        groupSeq: 0,
        timestampMs: DateTime.now().millisecondsSinceEpoch,
        payload: utf8.encode('{"title":"$title","start":true}'),
      ),
    );
    _activeMoqStreamId = streamId;
    if (_moqGroupCounter == 0) {
      // Reserve seq 0 for bootstrap: next media group uses 1, no collision.
      _moqGroupCounter = 1;
    }
    notifyDeferred();
  }

  /// End the local MoQ broadcast (publishes a stop control group).
  Future<void> stopMoqBroadcast() async {
    final streamId = _activeMoqStreamId;
    _activeMoqStreamId = null;
    if (streamId != null) {
      try {
        // Allocate fresh seq from counter, never hardcoded 1 (collides
        // with first media group).
        final stopSeq = _moqGroupCounter++;
        await publishLiveGroup(
          streamId: streamId,
          group: _controlGroup(
            groupSeq: stopSeq,
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

  Future<void> _shutdownOnBackground() async {
    if (isBroadcasting) {
      await stopMoqBroadcast();
    }
    try {
      moq.p2PQuicServerStop();
    } catch (_) {
      // server not running — nothing to stop
    }
  }

  static Map<String, dynamic> _controlGroup({
    required int groupSeq,
    required int timestampMs,
    required List<int> payload,
  }) {
    // Control track 99, type Control: viewers ignore unknown tracks.
    // Must NOT use track 0 VideoKeyframe with JSON payload — viewer would
    // try JPEG decode and poison the frame loop.
    return {
      'group_sequence': groupSeq,
      'objects': [
        {
          'header': {
            'track_id': 99,
            'group_sequence': groupSeq,
            'object_sequence': 0,
            'payload_size': payload.length,
            'track_type': 'Control',
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
      final json = await moq.p2PMoqSubscribeFetch(
        addr: addr,
        streamId: streamId,
        windowMs: BigInt.from(windowMs),
      );
      final parsed = jsonDecode(json) as Map<String, dynamic>;
      final frames = (parsed['groups'] as List<dynamic>? ?? [])
          .map((g) => _hexToBytes(g as String))
          .toList();
      if (clearLastError()) notifyDeferred();
      return frames.map(decodeMoqGroup).toList();
    } catch (e, st) {
      setLastError(e, st);
      notifyDeferred();
      rethrow;
    }
  }

  static Uint8List _hexToBytes(String hex) {
    if (hex.length.isOdd) hex = '0$hex';
    final out = Uint8List(hex.length ~/ 2);
    for (var i = 0; i < out.length; i++) {
      out[i] = int.parse(hex.substring(i * 2, i * 2 + 2), radix: 16);
    }
    return out;
  }

  Future<List<StreamRow>> _decode(String Function() call) => guard(() async {
        final json = call();
        return await runOffThreadCompute(_parseStreamRows, json);
      }, onNotify: notifyDeferred);

  @override
  void dispose() {
    _lifecycle?.dispose();
    super.dispose();
  }
}

/// JSON → [StreamRow] list, top-level so [compute] can run it on a
/// background isolate.
List<StreamRow> _parseStreamRows(String json) {
  final decoded = jsonDecode(json);
  return (decoded as List<dynamic>)
      .map((e) => StreamRow.fromJson(e as Map<String, dynamic>))
      .toList();
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
    // Rust emits live streams as StreamInfo (broadcaster_pubkey/title/
    // description/viewer_count) and stories as StoryInfo (author_pubkey/
    // content/views). Both flow through this single shared parser.
    final pubkey = json.strOf('broadcaster_pubkey').isNotEmpty
        ? json.strOf('broadcaster_pubkey')
        : (json.strOf('author_pubkey').isNotEmpty
            ? json.strOf('author_pubkey')
            : json.strOf('pubkey'));
    final storyContent = json.strOf('content');
    final description = json.strOf('description');
    final title = json.strOf('title').isNotEmpty
        ? json.strOf('title')
        : (storyContent.isEmpty
            ? 'Untitled'
            : storyContent.trim().split('\n').first);
    final summary = description.isNotEmpty
        ? description
        : (json.strOf('summary').isNotEmpty
            ? json.strOf('summary')
            : storyContent);
    final views = json['viewer_count'] != null
        ? json.intOf('viewer_count')
        : json.intOf('views');

    return StreamRow(
      id: json.strOf('id'),
      pubkey: pubkey,
      content: storyContent,
      createdAt: json.intOf('created_at'),
      views: views,
      title: title,
      summary: summary,
    );
  }
}
