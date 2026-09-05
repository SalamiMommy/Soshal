import 'dart:async';
import 'dart:convert';
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/audio_codec.dart';
import '../services/h264_codec.dart';
import '../services/session_service.dart';
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
  ui.Image? _frameImage;
  int _frameOrd = 0;
  int _appliedOrd = -1;

  /// Per-track watermark: group_sequence last processed for each track_id.
  /// MoQ seq values span all tracks from one shared counter, so a single
  /// global watermark would let a gap on one track suppress valid groups of
  /// another (ahead audio would permanently skip lower video GOPs). Dedup is
  /// therefore scoped per track.
  final Map<int, int> _lastSeqByTrack = {};
  bool _h264DecodeReady = false;
  bool _h264Tried = false;
  bool _h264GotKeyframe = false;
  bool _audioDecodeReady = false;
  bool _audioTried = false;
  bool _audioGotConfig = false;
  String? _subStatus;
  bool _showChatOverlay = true;
  bool _modView = false;
  final TextEditingController _chatInput = TextEditingController();
  final List<Map<String, String>> _chatMessages = [
    {
      'user': 'ModAlice',
      'role': 'mod',
      'badge': '🛡️',
      'text': 'Welcome to the live stream!'
    },
    {
      'user': 'VIPBob',
      'role': 'vip',
      'badge': '💎',
      'text': 'Hype! Let\'s go!'
    },
    {
      'user': 'SubCarol',
      'role': 'sub',
      'badge': '⭐',
      'text': 'Subscribed for 3 months!'
    },
  ];

  @override
  void initState() {
    super.initState();
    _run();
  }

  Future<void> _run() async {
    _running = true;
    final api = context.read<StreamingService>();
    var subscribed = false;
    while (_running && mounted) {
      try {
        if (!subscribed) {
          try {
            if (!mounted) return;
            final status = await api.subscribeMoqStream(
              streamId: widget.streamId,
              subscriberPubkey:
                  context.read<SessionService>().activePubkey ?? '',
            );
            final parsed =
                (jsonDecode(status) as Map<String, dynamic>?)?['status'];
            if (!mounted) return;
            setState(() => _subStatus = parsed as String? ?? status);
            subscribed = true;
          } catch (e) {
            debugPrint('moq subscribe: $e');
          }
        }
        final groups = await api.subscribeLiveFetch(
          addr: widget.addr,
          streamId: widget.streamId,
          windowMs: 3000,
        );
        var gotNewGroup = false;
        for (final group in groups) {
          final seq = (group['group_sequence'] as num?)?.toInt();
          if (seq == null) continue;
          final objects = (group['objects'] as List<dynamic>? ?? const []);
          var processedAny = false;
          for (final obj in objects) {
            final map = obj as Map<String, dynamic>?;
            final header = map?['header'] as Map<String, dynamic>?;
            final payload = header?['payload_size'] != null
                ? map!['payload'] as List<dynamic>
                : null;
            if (payload == null) continue;
            final bytes = Uint8List.fromList(payload.cast<int>());
            final trackId = (header?['track_id'] as num?)?.toInt() ?? 0;
            // Dedup per track so a gap on one track never suppresses valid
            // lower-seq groups of another. Advance regardless of decode
            // outcome so undecodable groups are not refetched every window.
            final lastForTrack = _lastSeqByTrack[trackId];
            if (lastForTrack != null && seq <= lastForTrack) continue;
            _lastSeqByTrack[trackId] = seq;
            _lastSeq = seq;
            processedAny = true;
            if (trackId == 1) {
              await _decodeH264(
                  seq, bytes, header?['track_type'] == 'VideoKeyframe');
            } else if (trackId == 2) {
              await _feedAac(seq, bytes);
            } else if (header?['track_type'] == 'VideoKeyframe') {
              _showFrame(bytes);
            }
          }
          if (processedAny) gotNewGroup = true;
        }
        if (!gotNewGroup) {
          await Future<void>.delayed(const Duration(milliseconds: 200));
        }
      } catch (e) {
        if (!mounted || !_running) return;
        // Drop the subscription flag so the loop re-subscribes after a
        // dropped QUIC connection instead of error-looping forever.
        subscribed = false;
        // Mid-GOP deltas and audio frames arriving without a fresh keyframe /
        // config would decode to garbage after a reconnect — force the
        // decoder to wait for the next keyframe/config and reset the
        // per-track watermark.
        _h264GotKeyframe = false;
        _audioGotConfig = false;
        _lastSeqByTrack.clear();
        await api.stopMoqStream();
        if (!mounted) return;
        setState(() => _error = '$e');
        await Future<void>.delayed(const Duration(seconds: 1));
      }
    }
  }

  /// Decode one H.264 NAL blob through the native decoder; every drained
  /// frame surfaces as a JPEG handed to `_showFrame`. Deltas arriving before the
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
      _showFrame(jpeg);
    }
  }

  /// Decode one JPEG at 480 px, swap it in. Decodes complete async and may
  /// arrive out of order — an ordinal discards stale frames; the previous
  /// `ui.Image` is disposed on swap to bound memory.
  Future<void> _showFrame(Uint8List jpeg) async {
    final ord = ++_frameOrd;
    final codec = await ui.instantiateImageCodec(jpeg, targetWidth: 480);
    final frame = await codec.getNextFrame();
    codec.dispose();
    if (!mounted || ord <= _appliedOrd) {
      frame.image.dispose();
      return;
    }
    _appliedOrd = ord;
    _frames++;
    setState(() {
      _frameImage?.dispose();
      _frameImage = frame.image;
    });
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

  StreamingService? _streamingService;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _streamingService = context.read<StreamingService>();
  }

  @override
  void dispose() {
    _running = false;
    _streamingService?.stopMoqStream();
    _frameImage?.dispose();
    _chatInput.dispose();
    H264Codec.release();
    AudioCodec.release();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Live Stream'),
        actions: [
          IconButton(
            icon:
                Icon(_showChatOverlay ? Icons.chat : Icons.chat_bubble_outline),
            tooltip: 'Toggle Chat Overlay',
            onPressed: () =>
                setState(() => _showChatOverlay = !_showChatOverlay),
          ),
          IconButton(
            icon: Icon(_modView ? Icons.security : Icons.security_outlined),
            tooltip: 'Mod View',
            color: _modView ? Colors.purpleAccent : null,
            onPressed: () {
              setState(() => _modView = !_modView);
              ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(
                    content: Text(_modView
                        ? 'Mod View enabled: auto-purge and timeout tools active.'
                        : 'Mod View disabled.')),
              );
            },
          ),
          TextButton(
            onPressed: _leaving ? null : _leave,
            child: const Text('Leave'),
          ),
        ],
      ),
      body: Stack(
        children: [
          _frameImage == null
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
                  child: Padding(
                    padding: const EdgeInsets.all(8),
                    child: RawImage(
                      image: _frameImage,
                      fit: BoxFit.contain,
                    ),
                  ),
                ),
          if (_showChatOverlay)
            Positioned(
              left: 16,
              right: 16,
              bottom: 12,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Container(
                    constraints: const BoxConstraints(maxHeight: 180),
                    decoration: BoxDecoration(
                      gradient: LinearGradient(
                        begin: Alignment.bottomCenter,
                        end: Alignment.topCenter,
                        colors: [
                          Colors.black.withValues(alpha: 0.75),
                          Colors.transparent,
                        ],
                      ),
                      borderRadius: BorderRadius.circular(12),
                    ),
                    padding: const EdgeInsets.all(8),
                    child: ListView.builder(
                      shrinkWrap: true,
                      itemCount: _chatMessages.length,
                      itemBuilder: (context, i) {
                        final m = _chatMessages[i];
                        return Padding(
                          padding: const EdgeInsets.symmetric(vertical: 2),
                          child: RichText(
                            text: TextSpan(
                              style: const TextStyle(
                                  fontSize: 13, color: Colors.white),
                              children: [
                                TextSpan(text: '${m['badge']} '),
                                TextSpan(
                                  text: '${m['user']}: ',
                                  style: TextStyle(
                                    fontWeight: FontWeight.bold,
                                    color: m['role'] == 'mod'
                                        ? Colors.greenAccent
                                        : m['role'] == 'vip'
                                            ? Colors.pinkAccent
                                            : Colors.amberAccent,
                                  ),
                                ),
                                TextSpan(text: m['text']),
                              ],
                            ),
                          ),
                        );
                      },
                    ),
                  ),
                  const SizedBox(height: 6),
                  Row(
                    children: [
                      Expanded(
                        child: Container(
                          height: 40,
                          decoration: BoxDecoration(
                            color: Colors.black.withValues(alpha: 0.6),
                            borderRadius: BorderRadius.circular(20),
                            border: Border.all(color: Colors.white24),
                          ),
                          child: TextField(
                            controller: _chatInput,
                            style: const TextStyle(
                                color: Colors.white, fontSize: 13),
                            decoration: const InputDecoration(
                              hintText: 'Send a message…',
                              hintStyle: TextStyle(color: Colors.white60),
                              border: InputBorder.none,
                              contentPadding: EdgeInsets.symmetric(
                                  horizontal: 14, vertical: 10),
                            ),
                          ),
                        ),
                      ),
                      const SizedBox(width: 8),
                      IconButton.filled(
                        icon: const Icon(Icons.send, size: 18),
                        onPressed: () {
                          final text = _chatInput.text.trim();
                          if (text.isNotEmpty) {
                            setState(() {
                              _chatMessages.add({
                                'user': 'You',
                                'role': 'viewer',
                                'badge': '👤',
                                'text': text,
                              });
                            });
                            _chatInput.clear();
                          }
                        },
                      ),
                    ],
                  ),
                ],
              ),
            ),
        ],
      ),
      bottomNavigationBar: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  Text(
                      '${_h264DecodeReady ? 'H.264' : 'MoQ JPEG'} · seq $_lastSeq'
                      '${_subStatus != null ? '\n$_subStatus' : ''}',
                      style: const TextStyle(fontWeight: FontWeight.bold)),
                  Text(
                    '$_frames frames',
                    style: const TextStyle(color: Colors.grey),
                  ),
                ],
              ),
              ValueListenableBuilder<String?>(
                valueListenable: H264Codec.error,
                builder: (_, err, __) => err == null
                    ? const SizedBox.shrink()
                    : Padding(
                        padding: const EdgeInsets.only(top: 8),
                        child: Text(
                          'Decoder error: $err',
                          style: TextStyle(color: Colors.red.shade600),
                        ),
                      ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
