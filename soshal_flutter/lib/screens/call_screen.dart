import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import '../services/calls_service.dart';

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
  final CallsService _service = CallsService();
  bool _muted = false;
  bool _speaker = false;
  bool _ending = false;

  @override
  void initState() {
    super.initState();
    _service.startCall(
      callId: widget.callId,
      peer: widget.peer,
      mediaType: widget.mediaType,
    );
  }

  @override
  void dispose() {
    _service.endCall();
    _service.dispose();
    super.dispose();
  }

  String get _shortPeer {
    final pk = widget.peer;
    if (pk.length <= 12) return pk;
    return '${pk.substring(0, 6)}…${pk.substring(pk.length - 6)}';
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
                  _shortPeer,
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
                const SizedBox(height: 48),
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
                  'Ending publishes a kind-20004 signal to $_shortPeer',
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
