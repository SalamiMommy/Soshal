// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:soshal_flutter/frb_generated.dart';
import 'error_log.dart';

/// Calls Service
/// Relay-based WebRTC signaling (kinds 20001-20004) plus in-call state:
/// call id, peer, media type, and an elapsed-call timer. Media transport
/// itself is gated behind the backend; this service only moves signals.
class CallsService extends ChangeNotifier with LastErrorMixin {
  final List<CallSignal> _signals = [];

  String? _callId;
  String? _peer;
  String? _mediaType;
  DateTime? _startedAt;
  Timer? _timer;

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

  /// Send a call signal (offer/answer/ice/end) to `targetPubkey`. SDP and
  /// candidates are redacted of private IPs bridge-side. Returns event id.
  Future<String> sendSignal({
    required String signalType,
    required String targetPubkey,
    required String callId,
    String? sdp,
    String? candidate,
    String? mediaType,
  }) async {
    try {
      final eventId = await RustLib.instance.api.crateFfiCallsCallsSendSignal(
        signalType: signalType,
        targetPubkey: targetPubkey,
        callId: callId,
        sdp: sdp,
        candidate: candidate,
        mediaType: mediaType,
      );
      _lastError = null;
      notifyListeners();
      return eventId;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Fetch call signals addressed to me (verified, p-tag filtered).
  Future<List<CallSignal>> fetchSignals(String myPubkey) async {
    try {
      final json = await RustLib.instance.api
          .crateFfiCallsCallsFetchSignals(myPubkey: myPubkey);
      final list = jsonDecode(json) as List<dynamic>;
      _signals
        ..clear()
        ..addAll(list
            .map((e) => CallSignal.fromJson(e as Map<String, dynamic>))
            .toList());
      _lastError = null;
      notifyListeners();
      return List.unmodifiable(_signals);
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Sanitize an SDP session description for relay publication (private IPs
  /// redacted). Sync FFI.
  String sanitizeSdp(String sdp, {bool forceRelay = false}) {
    try {
      final out = RustLib.instance.api.crateFfiCallsCallsSanitizeSdp(
        sdp: sdp,
        forceRelay: forceRelay,
      );
      _lastError = null;
      return out;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// ICE server configuration JSON for the given privacy level. Sync FFI.
  String iceConfig(String privacyLevel, {String stunUrl = ''}) {
    try {
      final out = RustLib.instance.api.crateFfiCallsCallsIceConfig(
        privacyLevel: privacyLevel,
        stunUrl: stunUrl,
      );
      _lastError = null;
      return out;
    } catch (e, st) {
      setLastError(e, st);
      notifyListeners();
      rethrow;
    }
  }

  /// Begin tracking an active call (starts the elapsed timer).
  void startCall({
    required String callId,
    required String peer,
    required String mediaType,
  }) {
    _callId = callId;
    _peer = peer;
    _mediaType = mediaType;
    _startedAt = DateTime.now();
    _timer?.cancel();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) {
      notifyListeners();
    });
    _lastError = null;
    notifyListeners();
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
    notifyListeners();
  }

  /// Clear error.
  void clearError() {
    _lastError = null;
    notifyListeners();
  }

  @override
  void dispose() {
    _timer?.cancel();
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
    final content = json['content'] as String? ?? '';
    final contentJson = content.isNotEmpty ? jsonDecode(content) : null;
    final contentMap = contentJson is Map<String, dynamic> ? contentJson : {};
    return CallSignal(
      id: json['id'] as String? ?? '',
      pubkey: json['pubkey'] as String? ?? '',
      content: content,
      createdAt: (json['created_at'] as num?)?.toInt() ?? 0,
      kind: (json['kind'] as num?)?.toInt() ?? 0,
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
