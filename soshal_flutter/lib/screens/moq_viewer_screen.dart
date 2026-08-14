import 'dart:async';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/audio_codec.dart';
import '../services/h264_codec.dart';
import '../services/streaming_service.dart';
import '../widgets/error_state_text.dart';

/// MoQ live viewer.
///
/// SUBSCRIBES to a peer's QUIC stream server for the given broadcast id,
/// decodes the received MoQ groups and renders: H.264 (track 1, hardware
/// decode via `H264Codec`), JPEG keyframes (track 0), and AAC mic audio
/// (track 2, via `AudioCodec`). H.264 decode only starts once a
/// `VideoKeyframe` group arrives (deltas before a keyframe are skipped), and
/// AAC playback starts once the codec-config object is seen. Re-subscribes
/// in a loop until the user leaves — the real crate-driven pull path:
/// buffered replay + live follow per window, no side channels.
class MoqViewerScreen extends StatefulWidget {
  final String addr;
  final String streamId;

  const MoqViewerScreen({
    super.key,
    required this.addr,
    required this.streamId,
  });

  @override
  State<MoqViewerScreen> createState() => _MoqViewerScreenState();
}

class _MoqViewerScreenState extends State<MoqViewerScreen> {
  bool _running = false;
  bool _leaving = false;
  String? _error;
  int _frames = 0;
  int? _lastSeq;
  Uint8List? _frameBytes;
  bool _h264DecodeReady = false;
  bool _h264Tried = false;
  bool _h264GotKeyframe = false;
  bool _audioDecodeReady = false;
  bool _audioTried = false;
  bool _audioGotConfig = false;

  @override
  void initState() {
    super.initState();
    _run();
  }

  Future<void> _run() async {
    _running = true;
    final api = context.read<StreamingService>();
    while (_running && mounted) {
      try {
        final groups = await api.subscribeLiveFetch(
          addr: widget.addr,
          streamId: widget.streamId,
          windowMs: 3000,
        );
        for (final group in groups) {
          final seq = (group['group_sequence'] as num?)?.toInt();
          if (seq == null || (_lastSeq != null && seq <= _lastSeq!)) continue;
          final objects = (group['objects'] as List<dynamic>? ?? const []);
          for (final obj in objects) {
            final map = obj as Map<String, dynamic>?;
            final header = map?['header'] as Map<String, dynamic>?;
            final payload = header?['payload_size'] != null
                ? map!['payload'] as List<dynamic>
                : null;
            if (payload == null) continue;
            final bytes = Uint8List.fromList(payload.cast<int>());
            final trackId = (header?['track_id'] as num?)?.toInt() ?? 0;
            if (trackId == 1) {
              await _decodeH264(
                  seq, bytes, header?['track_type'] == 'VideoKeyframe');
            } else if (trackId == 2) {
              await _feedAac(seq, bytes);
            } else if (header?['track_type'] == 'VideoKeyframe') {
              _lastSeq = seq;
              _frames++;
              if (mounted) setState(() => _frameBytes = bytes);
            }
          }
        }
      } catch (e) {
        if (!mounted || !_running) return;
        setState(() => _error = '$e');
        await Future<void>.delayed(const Duration(seconds: 1));
      }
    }
  }

  /// Decode one H.264 NAL blob through the native decoder; every drained
  /// frame surfaces as a JPEG in `_frameBytes`. Deltas arriving before the
  /// first keyframe are skipped (decoder cannot start mid-GOP).
  Future<void> _decodeH264(int seq, Uint8List nal, bool isKeyframe) async {
    if (!_h264GotKeyframe) {
      if (!isKeyframe) return;
      _h264GotKeyframe = true;
    }
    if (!_h264Tried) {
      _h264Tried = true;
      _h264DecodeReady =
          await H264Codec.isSupported() && await H264Codec.initDecode();
    }
    if (!_h264DecodeReady) return;
    final jpegs = await H264Codec.feedDecode(nal);
    for (final jpeg in jpegs) {
      _lastSeq = seq;
      _frames++;
      if (mounted) setState(() => _frameBytes = jpeg);
    }
  }

  /// Feed one AAC blob to the native decoder. Payloads carry the native tag
  /// byte: `2` = codec config, `1` = audio frame. Frames before the config
  /// object are dropped; once a config arrives the stream plays via
  /// AudioTrack.
  Future<void> _feedAac(int seq, Uint8List blob) async {
    if (!_audioTried) {
      _audioTried = true;
      _audioDecodeReady =
          await AudioCodec.isSupported() && await AudioCodec.initDecode();
    }
    if (!_audioDecodeReady) return;
    if (blob.length < 2) return;
    if (blob[0] == 2) _audioGotConfig = true;
    if (!_audioGotConfig) return;
    _lastSeq = seq;
    await AudioCodec.feedAac(Uint8List.sublistView(blob, 1));
  }

  Future<void> _leave() async {
    _running = false;
    _leaving = true;
    if (mounted) Navigator.of(context).pop();
  }

  @override
  void dispose() {
    _running = false;
    H264Codec.release();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Live'),
        actions: [
          TextButton(
            onPressed: _leaving ? null : _leave,
            child: const Text('Leave'),
          ),
        ],
      ),
      body: _frameBytes == null
          ? Center(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const CircularProgressIndicator(),
                  const SizedBox(height: 16),
                  _error == null
                      ? const Text('Waiting for MoQ groups…')
                      : ErrorStateText('$_error'),
                ],
              ),
            )
          : Center(
              child: AspectRatio(
                aspectRatio: 1,
                child: Image.memory(
                  _frameBytes!,
                  fit: BoxFit.contain,
                  gaplessPlayback: true,
                ),
              ),
            ),
      bottomNavigationBar: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                  _h264DecodeReady
                      ? 'H.264 · seq $_lastSeq'
                      : 'MoQ JPEG · seq $_lastSeq',
                  style: const TextStyle(fontWeight: FontWeight.bold)),
              Text('$_frames frames',
                  style: const TextStyle(color: Colors.grey)),
            ],
          ),
        ),
      ),
    );
  }
}
