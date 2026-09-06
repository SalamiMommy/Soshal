import '../utils/json_ext.dart';
// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';

import 'package:flutter/widgets.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';
import '../utils/service_guard.dart';

/// Calls Service
/// Relay-based WebRTC signaling (kinds 20001-20004) plus in-call state:
/// call id, peer, media type, and an elapsed-call timer. Media transport
/// itself is gated behind the backend; this service only moves signals.
class CallsService extends ChangeNotifier
    with LastErrorMixin, DeferredNotify, ServiceGuard {
  final List<CallSignal> _signals = [];

  String? _callId;
  String? _peer;
  String? _mediaType;
  DateTime? _startedAt;
  Timer? _timer;
  AppLifecycleListener? _lifecycle;

  List<CallSignal> get signals => _signals;
  String? get callId => _callId;
  String? get peer => _peer;
  String? get mediaType => _mediaType;
  bool get inCall => _callId != null && _startedAt != null;

  /// Elapsed wall time since the call screen opened.
  Duration get elapsed {
    final start = _startedAt;
    if (start == null) return Duration.zero;
    return DateTime.now().difference(start);
  }

  /// Clear relay signal inbox + in-call state on account switch so Account B
  /// never sees Account A's call signaling or a stale active call.
  void resetForAccountSwitch() {
    _signals.clear();
    _callId = null;
    _peer = null;
    _mediaType = null;
    _startedAt = null;
    _timer?.cancel();
    _timer = null;
    clearLastError();
    notifyDeferred();
  }

  /// Send a call signal (offer/answer/ice/end) to `targetPubkey`. SDP and
  /// candidates are redacted of private IPs bridge-side. Returns event id.
  Future<String> sendSignal({
    required String signalType,
    required String targetPubkey,
    required String callId,
    String? sdp,
    String? candidate,
    String? mediaType,
  }) =>
      guard(() async {
        return await RustLib.instance.api.crateFfiCallsCallsSendSignal(
          signalType: signalType,
          targetPubkey: targetPubkey,
          callId: callId,
          sdp: sdp,
          candidate: candidate,
          mediaType: mediaType,
        );
      }, onNotify: notifyDeferred);

  /// Fetch call signals addressed to me (verified, p-tag filtered).
  Future<List<CallSignal>> fetchSignals(String myPubkey) => guard(() async {
        final json = await RustLib.instance.api
            .crateFfiCallsCallsFetchSignals(myPubkey: myPubkey);
        final list = jsonDecode(json) as List<dynamic>;
        _signals
          ..clear()
          ..addAll(list
              .map((e) => CallSignal.fromJson(e as Map<String, dynamic>))
              .toList());
        return List.unmodifiable(_signals);
      }, onNotify: notifyDeferred);

  /// Sanitize an SDP session description for relay publication (private IPs
  /// redacted). Sync FFI.
  String sanitizeSdp(String sdp, {bool forceRelay = false}) =>
      guardSync(() {
        return RustLib.instance.api.crateFfiWebrtcWebrtcSanitizeSdp(
          sdp: sdp,
          forceRelay: forceRelay,
        );
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// ICE server configuration JSON for the given privacy level. Sync FFI.
  String iceConfig(String privacyLevel) => guardSync(() {
        return RustLib.instance.api
            .crateFfiWebrtcWebrtcGetIceConfig(privacyLevel: privacyLevel);
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// Default STUN servers. Sync FFI.
  List<String> stunServers() => guardSync(() {
        return RustLib.instance.api.crateFfiWebrtcWebrtcGetStunServers();
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// Configured TURN servers JSON (empty list when unprovisioned). Sync FFI.
  String turnServers({String? authToken}) => guardSync(() {
        return RustLib.instance.api
            .crateFfiWebrtcWebrtcGetTurnServers(authToken: authToken);
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// Peer connection config JSON for a privacy level. Sync FFI.
  String createPeerConfig(String privacyLevel) => guardSync(() {
        return RustLib.instance.api
            .crateFfiWebrtcWebrtcCreatePeerConfig(privacyLevel: privacyLevel);
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// Extract ICE candidates from an SDP. Sync FFI.
  List<String> extractCandidates(String sdp) => guardSync(() {
        return RustLib.instance.api
            .crateFfiWebrtcWebrtcExtractCandidates(sdp: sdp);
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// Append an ICE candidate line to an SDP. Sync FFI.
  String addCandidateToSdp(String sdp, String candidate) => guardSync(() {
        return RustLib.instance.api.crateFfiWebrtcWebrtcAddCandidateToSdp(
          sdp: sdp,
          candidate: candidate,
        );
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// Basic SDP validity check. Sync FFI.
  bool validateSdp(String sdp) => guardSync(() {
        return RustLib.instance.api.crateFfiWebrtcWebrtcValidateSdp(sdp: sdp);
      }, onNotify: notifyDeferred, notifyOnSuccess: false);

  /// Begin tracking an active call (starts the elapsed timer). Ticks are
  /// paused while the app is backgrounded; elapsed wall time keeps counting.
  void startCall({
    required String callId,
    required String peer,
    required String mediaType,
  }) {
    _callId = callId;
    _peer = peer;
    _mediaType = mediaType;
    _startedAt = DateTime.now();
    _lifecycle ??= AppLifecycleListener(
      onHide: _pauseTick,
      onPause: _pauseTick,
      onResume: _resumeTick,
    );
    _resumeTick();
    clearLastError();
    notifyDeferred();
  }

  void _pauseTick() {
    _timer?.cancel();
    _timer = null;
  }

  void _resumeTick() {
    if (_callId == null || _startedAt == null) return;
    _timer?.cancel();
    var lastTick = DateTime.now();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) {
      final now = DateTime.now();
      if (now.second == lastTick.second) return;
      lastTick = now;
      notifyDeferred();
    });
  }

  /// Stop tracking the active call (cancels the timer). Called by the call
  /// screen when the call ends or the route is disposed.
  void endCall() {
    _timer?.cancel();
    _timer = null;
    _callId = null;
    _peer = null;
    _mediaType = null;
    _startedAt = null;
    notifyDeferred();
  }

  @override
  void dispose() {
    _timer?.cancel();
    _lifecycle?.dispose();
    super.dispose();
  }
}

/// A relay-fetched call signal as served by the bridge.
class CallSignal {
  final String id;
  final String pubkey;
  final String content;
  final int createdAt;
  final int kind;
  final List<String> pTags;
  final String callId;

  final String signalType;
  final String? signalMediaType;

  CallSignal({
    required this.id,
    required this.pubkey,
    required this.content,
    required this.createdAt,
    required this.kind,
    required this.pTags,
    required this.callId,
    required this.signalType,
    this.signalMediaType,
  });

  factory CallSignal.fromJson(Map<String, dynamic> json) {
    final content = json.strOf('content');
    final contentJson = content.isNotEmpty ? jsonDecode(content) : null;
    final contentMap = contentJson is Map<String, dynamic> ? contentJson : {};
    return CallSignal(
      id: json.strOf('id'),
      pubkey: json.strOf('pubkey'),
      content: content,
      createdAt: json.intOf('created_at'),
      kind: json.intOf('kind'),
      pTags: (json['p_tags'] as List<dynamic>?)
              ?.map((e) => e.toString())
              .toList() ??
          [],
      callId: contentMap['call_id'] as String? ?? '',
      signalType: contentMap['type'] as String? ?? '',
      signalMediaType: contentMap['media_type'] as String?,
    );
  }
}
