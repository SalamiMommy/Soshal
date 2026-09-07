import 'package:flutter/material.dart';
import 'package:media_kit/media_kit.dart' as mk;
import 'package:media_kit_video/media_kit_video.dart';

import '../services/permissions_service.dart';
import '../utils/safe_url.dart';

/// Plays a mini video (local blob-server URL or remote fallback) inside a
/// dialog. Uses media_kit (mpv on Linux, MediaCodec/ExoPlayer on Android) —
/// the same stack on every platform.
class MiniVideoPlayer extends StatefulWidget {
  final String url;

  const MiniVideoPlayer(this.url, {super.key});

  @override
  State<MiniVideoPlayer> createState() => _MiniVideoPlayerState();
}

class _MiniVideoPlayerState extends State<MiniVideoPlayer> {
  VideoController? _controller;
  String? _error;

  static bool get _playbackSupported =>
      PermissionsService.isAndroid || PermissionsService.isLinux;

  @override
  void initState() {
    super.initState();
    if (!_playbackSupported) {
      _error = 'Video playback is not supported on this platform.';
      return;
    }
    if (!SafeUrl.isSafePlaybackUrl(widget.url)) {
      _error = 'Video source rejected (unsafe host).';
      return;
    }
    final player = mk.Player();
    _controller = VideoController(player);
    player.open(mk.Media(widget.url), play: true).catchError((e) {
      if (!mounted) return;
      setState(() => _error = 'Playback failed: $e');
    });
  }

  @override
  void dispose() {
    _controller?.player.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Mini'),
      content: SizedBox(
        width: 360,
        height: 240,
        child: _error != null
            ? Center(
                child: Text(_error!, style: const TextStyle(color: Colors.red)),
              )
            : _controller != null
                ? Video(controller: _controller!, fit: BoxFit.contain)
                : const Center(child: CircularProgressIndicator()),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Close'),
        ),
      ],
    );
  }
}
