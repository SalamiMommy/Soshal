import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'dart:convert';

import '../services/calls_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';

/// Full-screen in-call view for voice/video calls.
///
/// Receives the peer, media type, and call id from the route
/// (`/call/<peer>/<mediaType>/<callId>`); the shell's incoming-call banner
/// and the caller flow both land here once the call is accepted. Media
/// transport is pending relay signaling, so this screen tracks and
/// terminates the signal flow honestly.
class CallScreen extends StatefulWidget {
  /// Peer pubkey.
  final String peer;

  /// Audio or video.
  final String mediaType;

  /// Shared call id.
  final String callId;

  /// Call screen.
  const CallScreen({
    super.key,
    required this.peer,
    required this.mediaType,
    required this.callId,
  });

  @override
  State<CallScreen> createState() => _CallScreenState();
}

class _CallScreenState extends State<CallScreen> {
  late final CallsService _service;
  bool _muted = false;
  bool _speaker = false;
  bool _ending = false;
  String _privacyLevel = 'friends';
  String _iceSummary = '';
  int _signalCount = 0;
  String? _latestSignal;
  String? _setupError;
  bool _sendingOffer = false;

  @override
  void initState() {
    super.initState();
    _service = context.read<CallsService>();
    _service.startCall(
      callId: widget.callId,
      peer: widget.peer,
      mediaType: widget.mediaType,
    );
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      _refreshSignals();
      _refreshIce();
    });
  }

  @override
  void dispose() {
    _service.endCall();
    super.dispose();
  }

  String get _elapsedLabel {
    final e = _service.elapsed;
    final minutes = e.inMinutes.toString().padLeft(2, '0');
    final seconds = (e.inSeconds % 60).toString().padLeft(2, '0');
    return '$minutes:$seconds';
  }

  Future<void> _endCall() async {
    if (_ending) return;
    setState(() => _ending = true);
    try {
      await _service.sendSignal(
        signalType: 'end',
        targetPubkey: widget.peer,
        callId: widget.callId,
        mediaType: widget.mediaType,
      );
    } catch (e) {
      debugPrint('end call signal: $e');
    }
    if (mounted) {
      context.pop();
    }
  }

  /// Poll relay for incoming call signals addressed to me (kinds
  /// 20001-20004, verified + p-tag filtered bridge-side).
  Future<void> _refreshSignals() async {
    final pubkey = context.read<SessionService>().activePubkey;
    if (pubkey == null) {
      setState(() => _setupError = 'Not signed in');
      return;
    }
    try {
      final signals = await _service.fetchSignals(pubkey);
      _signalCount = signals.length;
      final latest = signals.isNotEmpty ? signals.first : null;
      _latestSignal = latest == null
          ? null
          : '${latest.signalType} · ${prefixEllipsis(latest.pubkey, 10)}';
      _setupError = null;
    } catch (e) {
      _setupError = 'signal poll: $e';
    }
    if (mounted) setState(() {});
  }

  /// Rebuild ICE config summary for the chosen privacy level: calls-module
  /// ICE config + WebRTC-module ICE/peer config + STUN/TURN servers.
  void _refreshIce() {
    try {
      final cfg = _service.iceConfig(_privacyLevel);
      final peer = _service.createPeerConfig(_privacyLevel);
      final stun = _service.stunServers();
      final turn = _service.turnServers();
      _iceSummary = 'STUN: ${stun.join(', ')}\n'
          'TURN: $turn\n'
          'ICE policy: ${_policyFromJson(cfg)}\n'
          'WebRTC: $cfg\n'
          'Peer config: $peer';
      _setupError = null;
    } catch (e) {
      _setupError = 'ICE config: $e';
    }
  }

  static String _policyFromJson(String json) {
    try {
      final decoded = jsonDecode(json);
      if (decoded is Map<String, dynamic>) {
        return decoded['iceTransportPolicy']?.toString() ?? json;
      }
    } catch (_) {}
    return json;
  }

  /// Build a minimal local SDP, validate + sanitize it (private IPs
  /// redacted; relay-only for the friends level), attach a candidate, then
  /// publish the kind-20001 offer signal.
  Future<void> _sendOffer() async {
    if (_sendingOffer) return;
    setState(() => _sendingOffer = true);
    try {
      final forceRelay = _privacyLevel == 'friends';
      final local = 'v=0\r\n'
          'o=- 0 0 IN IP4 0.0.0.0\r\n'
          's=-\r\n'
          'c=IN IP4 192.168.1.50\r\n'
          't=0 0\r\n'
          'm=audio 9 UDP/TLS/RTP/SAVPF 111\r\n'
          'a=candidate:1 1 UDP 1 192.168.1.50 5000 typ host\r\n';
      if (!_service.validateSdp(local)) {
        _snack('Local SDP invalid');
        return;
      }
      var sdp = _service.sanitizeSdp(local, forceRelay: forceRelay);
      final candidates = _service.extractCandidates(sdp);
      if (candidates.isNotEmpty) {
        sdp = _service.addCandidateToSdp(sdp, candidates.first);
      } else {
        sdp = _service.addCandidateToSdp(
            sdp, 'a=candidate:1 1 UDP 1 203.0.113.9 5000 typ relay');
      }
      sdp = _service.sanitizeSdp(sdp, forceRelay: forceRelay);
      final id = await _service.sendSignal(
        signalType: 'offer',
        targetPubkey: widget.peer,
        callId: widget.callId,
        sdp: sdp,
        mediaType: widget.mediaType,
      );
      _snack('Offer signal sent · ${prefixEllipsis(id, 12)}');
    } catch (e) {
      _snack('Offer error: $e');
    } finally {
      if (mounted) setState(() => _sendingOffer = false);
    }
  }

  void _snack(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: SelectableText(message)));
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final isVideo = widget.mediaType == 'video';
    return Scaffold(
      backgroundColor: theme.colorScheme.surfaceContainerLowest,
      appBar: AppBar(
        title: Text('${isVideo ? 'Video' : 'Voice'} call'),
      ),
      body: ListenableBuilder(
        listenable: _service,
        builder: (context, _) {
          return Center(
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Text(
                  shortPubkey(widget.peer),
                  style: theme.textTheme.headlineSmall,
                ),
                const SizedBox(height: 8),
                Text(
                  isVideo ? 'Video call' : 'Voice call',
                  style: theme.textTheme.bodyMedium?.copyWith(
                    color: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
                const SizedBox(height: 32),
                CircleAvatar(
                  radius: 48,
                  backgroundColor: theme.colorScheme.primaryContainer,
                  child: Icon(
                    isVideo ? Icons.videocam : Icons.call,
                    size: 40,
                    color: theme.colorScheme.onPrimaryContainer,
                  ),
                ),
                const SizedBox(height: 32),
                Text(
                  _elapsedLabel,
                  style: theme.textTheme.displaySmall?.copyWith(
                    fontFeatures: const [FontFeature.tabularFigures()],
                  ),
                ),
                const SizedBox(height: 8),
                Text(
                  'Media transport pending relay signaling',
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.outline,
                  ),
                ),
                const SizedBox(height: 24),
                Card(
                  margin: const EdgeInsets.symmetric(horizontal: 24),
                  child: Padding(
                    padding: const EdgeInsets.all(12),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('Call setup', style: theme.textTheme.titleSmall),
                        const SizedBox(height: 8),
                        SegmentedButton<String>(
                          segments: const [
                            ButtonSegment(
                                value: 'public', label: Text('Public')),
                            ButtonSegment(
                                value: 'friends', label: Text('Friends')),
                          ],
                          selected: {_privacyLevel},
                          onSelectionChanged: (selection) {
                            setState(() => _privacyLevel = selection.first);
                            _refreshIce();
                          },
                        ),
                        const SizedBox(height: 8),
                        Row(
                          children: [
                            Expanded(
                              child: Text(
                                _latestSignal == null
                                    ? 'Signals: $_signalCount'
                                    : 'Signals: $_signalCount · '
                                        'latest: $_latestSignal',
                                style: theme.textTheme.bodySmall,
                              ),
                            ),
                            IconButton(
                              tooltip: 'Refresh signals',
                              icon: const Icon(Icons.refresh, size: 18),
                              onPressed: _refreshSignals,
                            ),
                          ],
                        ),
                        if (_setupError != null)
                          Padding(
                            padding: const EdgeInsets.only(bottom: 8),
                            child: Text(
                              _setupError!,
                              style: theme.textTheme.bodySmall?.copyWith(
                                color: theme.colorScheme.error,
                              ),
                            ),
                          ),
                        if (_iceSummary.isNotEmpty)
                          SelectableText(
                            _iceSummary,
                            style: theme.textTheme.bodySmall?.copyWith(
                              color: theme.colorScheme.onSurfaceVariant,
                            ),
                          ),
                        const SizedBox(height: 8),
                        SizedBox(
                          width: double.maxFinite,
                          child: OutlinedButton.icon(
                            icon: _sendingOffer
                                ? const SizedBox(
                                    width: 14,
                                    height: 14,
                                    child: CircularProgressIndicator(
                                        strokeWidth: 2),
                                  )
                                : const Icon(Icons.call_made, size: 18),
                            label: Text(_sendingOffer
                                ? 'Sending offer…'
                                : 'Send offer signal'),
                            onPressed: _sendingOffer ? null : _sendOffer,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
                const SizedBox(height: 24),
                Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    _controlButton(
                      tooltip: _muted ? 'Unmute' : 'Mute',
                      icon: _muted ? Icons.mic_off : Icons.mic,
                      active: _muted,
                      onPressed: () => setState(() => _muted = !_muted),
                    ),
                    const SizedBox(width: 24),
                    _controlButton(
                      tooltip: _speaker ? 'Speaker off' : 'Speaker on',
                      icon: _speaker ? Icons.volume_up : Icons.volume_off,
                      active: _speaker,
                      onPressed: () => setState(() => _speaker = !_speaker),
                    ),
                    const SizedBox(width: 24),
                    _controlButton(
                      tooltip: 'End call',
                      icon: Icons.call_end,
                      active: true,
                      danger: true,
                      onPressed: _ending ? null : _endCall,
                    ),
                  ],
                ),
                const SizedBox(height: 32),
                Text(
                  'Ending publishes a kind-20004 signal to ${shortPubkey(widget.peer)}',
                  style: theme.textTheme.bodySmall?.copyWith(
                    color: theme.colorScheme.outline,
                  ),
                ),
              ],
            ),
          );
        },
      ),
    );
  }

  Widget _controlButton({
    required String tooltip,
    required IconData icon,
    required bool active,
    required VoidCallback? onPressed,
    bool danger = false,
  }) {
    final theme = Theme.of(context);
    return Tooltip(
      message: tooltip,
      child: Material(
        shape: const CircleBorder(),
        color: danger
            ? theme.colorScheme.error
            : active
                ? theme.colorScheme.primaryContainer
                : theme.colorScheme.surfaceContainerHighest,
        child: IconButton(
          iconSize: 28,
          icon: Icon(
            icon,
            color: danger
                ? theme.colorScheme.onError
                : active
                    ? theme.colorScheme.onPrimaryContainer
                    : theme.colorScheme.onSurfaceVariant,
          ),
          onPressed: onPressed,
        ),
      ),
    );
  }
}
