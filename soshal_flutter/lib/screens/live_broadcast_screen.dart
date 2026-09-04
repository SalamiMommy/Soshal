import 'dart:async';
import 'dart:isolate';
import 'dart:typed_data';

import 'package:camera/camera.dart';
import 'package:flutter/material.dart';
import 'package:image/image.dart' as img;
import 'package:provider/provider.dart';
import '../services/audio_codec.dart';
import '../services/h264_codec.dart';
import '../services/permissions_service.dart';
import '../services/streaming_service.dart';
import '../widgets/app_snack.dart';
import '../widgets/error_state_text.dart';

/// Live broadcast capture screen.
///
/// Real camera capture: frames are grabbed from the camera image stream and
/// published as MoQ video groups into the local live registry — the same
/// registry LAN peers crawl via `MoqViewerScreen`.
///
/// Tracks: hardware H.264 (track 1, ~15 fps, MediaCodec via `H264Codec`)
/// when the device supports it, a cheap JPEG keyframe track (track 0, ~4
/// fps) for codec-less viewers, and optional AAC-LC mic audio (track 2,
/// `AudioDatagram`, via `AudioCodec`) toggled by the mic button. Falls back
/// to JPEG-only when the native channels are unavailable.
class LiveBroadcastScreen extends StatefulWidget {
  final String streamId;
  final String title;

  const LiveBroadcastScreen({
    super.key,
    required this.streamId,
    required this.title,
  });

  @override
  State<LiveBroadcastScreen> createState() => _LiveBroadcastScreenState();
}

class _LiveBroadcastScreenState extends State<LiveBroadcastScreen> {
  CameraController? _camera;
  bool _initializing = true;
  bool _broadcasting = false;
  String? _error;
  int _framesPublished = 0;
  int _h264Frames = 0;
  DateTime? _lastFrameAt;
  bool _frameInFlight = false;
  DateTime? _lastH264At;
  bool _h264Ready = false;
  bool _h264Tried = false;
  bool _audioReady = false;
  bool _audioTried = false;
  bool _micOn = false;
  bool _audioBusy = false;
  Timer? _audioTimer;
  Uint8List? _aacConfig;
  int _audioGroupsSent = 0;
  bool _recording = false;
  int? _onWireBytes;
  String? _micDeniedReason;
  bool _cameraDenied = false;

  /// ~4 fps JPEG fallback track.
  static const Duration _frameInterval = Duration(milliseconds: 250);

  /// ~15 fps hardware track.
  static const Duration _h264Interval = Duration(milliseconds: 66);

  static const int _h264Bitrate = 700000;

  @override
  void initState() {
    super.initState();
    _initCamera();
  }

  Future<void> _initCamera() async {
    try {
      final cam = await PermissionsService.ensureCamera();
      if (!cam.granted) {
        if (mounted) {
          setState(() {
            _error = cam.reason;
            _cameraDenied = true;
            _initializing = false;
          });
        }
        return;
      }
      final mic = await PermissionsService.ensureMic();
      if (!mic.granted && mounted) {
        _micDeniedReason = mic.reason;
      }
      final cameras = await availableCameras();
      final camera = cameras.isEmpty
          ? null
          : cameras.firstWhere(
              (c) => c.lensDirection == CameraLensDirection.back,
              orElse: () => cameras.first,
            );
      if (camera == null) throw Exception('No camera on this device');
      final controller = CameraController(
        camera,
        ResolutionPreset.low,
        enableAudio: false,
        imageFormatGroup: ImageFormatGroup.bgra8888,
      );
      await controller.initialize();
      _camera = controller;
      if (!mounted) return;
      setState(() => _initializing = false);
      await controller.startImageStream(_onFrame);
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = '$e';
          _initializing = false;
        });
      }
    }
  }

  Future<void> _onFrame(CameraImage image) async {
    if (!mounted) return;
    // The plugin does not await this callback: once a frame passes the
    // throttle gate, its awaits (Isolate.run, publish) run to completion
    // concurrently with later frames. Serialize with an in-flight guard so
    // encodes/publishes (and MoQ group sequence allocation) cannot overlap.
    if (_frameInFlight) return;
    _frameInFlight = true;
    final now = DateTime.now();
    final last = _lastFrameAt;
    try {
      if (last != null && now.difference(last) < _frameInterval) return;
      _lastFrameAt = now;
      if (!_broadcasting) return;
      final api = context.read<StreamingService>();
      final plane = image.planes.first;
      final width = image.width;
      final height = image.height;
      final rowStride = plane.bytesPerRow;
      final bytes = Uint8List.fromList(plane.bytes);
      final jpeg = await Isolate.run(() {
        final frame = img.Image.fromBytes(
          width: width,
          height: height,
          bytes: bytes.buffer,
          order: img.ChannelOrder.bgra,
          rowStride: rowStride,
        );
        return img.encodeJpg(frame, quality: 60);
      });
      final group = api.buildVideoGroup(
        groupSeq: api.nextMoqGroupSeq(),
        timestampMs: now.millisecondsSinceEpoch,
        jpeg: jpeg,
      );
      await api.publishLiveGroupSilent(streamId: widget.streamId, group: group);
      _onWireBytes = jpeg.length + 64;
      _framesPublished++;
      await _publishH264(image, now, api);
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      _frameInFlight = false;
    }
  }

  /// Hardware H.264 track: lazily init the codec on the first frame, then
  /// encode BGRA frames at ~15 fps and publish each drained NAL blob as its
  /// own group (key frame when the encoder flags one).
  Future<void> _publishH264(
      CameraImage image, DateTime now, StreamingService api) async {
    if (!_h264Tried) {
      _h264Tried = true;
      _h264Ready = await H264Codec.isSupported() &&
          await H264Codec.initEncode(
            width: image.width,
            height: image.height,
            bitrate: _h264Bitrate,
            fps: 15,
          );
    }
    if (!_h264Ready) return;
    final last = _lastH264At;
    if (last != null && now.difference(last) < _h264Interval) return;
    _lastH264At = now;
    final plane = image.planes.first;
    final bgra = plane.bytes;
    final blobs = await H264Codec.feedEncode(bgra);
    for (final blob in blobs) {
      if (blob.length < 2) continue;
      final keyframe = blob[0] == 1;
      final nal = Uint8List.sublistView(blob, 1);
      await api.publishLiveGroupSilent(
        streamId: widget.streamId,
        group: api.buildH264Group(
          groupSeq: api.nextMoqGroupSeq(),
          timestampMs: now.millisecondsSinceEpoch,
          nal: nal,
          keyframe: keyframe,
        ),
      );
      if (!mounted) return;
      _h264Frames++;
    }
  }

  Future<void> _stop() async {
    _broadcasting = false;
    _micOn = false;
    _audioTimer?.cancel();
    _audioTimer = null;
    await AudioCodec.setMicEnable(false);
    final recorded = await _stopRecording();
    await _camera?.stopImageStream();
    if (!mounted) {
      await _camera?.dispose();
      await H264Codec.release();
      await AudioCodec.release();
      return;
    }
    await _camera?.dispose();
    await H264Codec.release();
    await AudioCodec.release();
    if (!mounted) return;
    await context.read<StreamingService>().stopMoqBroadcast();
    if (mounted) {
      if (recorded != null) _toast('Recorded: $recorded');
      Navigator.of(context).pop(true);
    }
  }

  /// Toggle the local DVR. Recording feeds from the live codec drains
  /// (H.264 + AAC when the mic is up); stops only when the broadcast stops
  /// or the user taps the record button again.
  Future<void> _toggleRecord() async {
    if (_recording) {
      final path = await _stopRecording();
      if (path != null) _toast('Recorded: $path');
      return;
    }
    final path = await H264Codec.initRecord();
    if (path == null) {
      _toast('Recording unavailable');
      return;
    }
    setState(() {
      _recording = true;
    });
  }

  Future<String?> _stopRecording() async {
    if (!_recording) return null;
    final path = await H264Codec.stopRecord();
    if (mounted) setState(() => _recording = false);
    return path;
  }

  void _toast(String msg) {
    if (!mounted) return;
    showAppSnack(context, msg);
  }

  /// Lazily init the AAC encoder once, then toggle the mic + drain timer.
  /// Blobs are published as `AudioDatagram` groups: the codec-config blob
  /// first (re-sent every ~100 groups so late joiners can start decoding),
  /// then audio frames.
  Future<void> _toggleMic(StreamingService api) async {
    final deniedReason = _micDeniedReason;
    if (deniedReason != null) {
      _toast(deniedReason);
      return;
    }
    if (!_audioTried) {
      _audioTried = true;
      _audioReady =
          await AudioCodec.isSupported() && await AudioCodec.initEncode();
    }
    if (!_audioReady) {
      if (mounted) setState(() => _error = 'AAC encoder unavailable');
      return;
    }
    _micOn = !_micOn;
    await AudioCodec.setMicEnable(_micOn);
    if (_micOn) {
      _audioTimer?.cancel();
      _audioTimer =
          Timer.periodic(const Duration(milliseconds: 100), (_) async {
        if (!_broadcasting || _audioBusy) return;
        _audioBusy = true;
        try {
          final blobs = await AudioCodec.drainAudio();
          for (final blob in blobs) {
            if (blob.length < 2) continue;
            final config = blob[0] == 2;
            if (config) _aacConfig = blob;
            await api.publishLiveGroupSilent(
              streamId: widget.streamId,
              group: api.buildAudioGroup(
                groupSeq: api.nextMoqGroupSeq(),
                timestampMs: DateTime.now().millisecondsSinceEpoch,
                aac: blob,
                config: config,
              ),
            );
            _audioGroupsSent++;
            if (!config && _audioGroupsSent % 100 == 0 && _aacConfig != null) {
              await api.publishLiveGroupSilent(
                streamId: widget.streamId,
                group: api.buildAudioGroup(
                  groupSeq: api.nextMoqGroupSeq(),
                  timestampMs: DateTime.now().millisecondsSinceEpoch,
                  aac: _aacConfig!,
                  config: true,
                ),
              );
            }
          }
        } catch (e) {
          if (mounted) setState(() => _error = '$e');
        } finally {
          _audioBusy = false;
        }
      });
    } else {
      _audioTimer?.cancel();
      _audioTimer = null;
    }
    if (mounted) setState(() {});
  }

  StreamingService? _streamingService;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _streamingService = context.read<StreamingService>();
  }

  @override
  void dispose() {
    _audioTimer?.cancel();
    _camera?.dispose();
    final service = _streamingService;
    if (service != null) {
      unawaited(service.stopMoqBroadcast());
    }
    H264Codec.stopRecord();
    H264Codec.release();
    AudioCodec.release();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final camera = _camera;
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.title),
        actions: [
          TextButton(
            onPressed: _broadcasting ? _stop : null,
            child: const Text('Stop'),
          ),
        ],
      ),
      body: _initializing
          ? const Center(child: CircularProgressIndicator())
          : _error != null && camera == null
              ? Center(
                  child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      ErrorStateText('Camera unavailable: $_error'),
                      if (_cameraDenied) ...[
                        const SizedBox(height: 12),
                        FilledButton.icon(
                          onPressed: () => PermissionsService.openSettings(),
                          icon: const Icon(Icons.settings),
                          label: const Text('Open app settings'),
                        ),
                      ],
                    ],
                  ),
                ))
              : Column(
                  children: [
                    Expanded(
                      child: camera == null
                          ? const Center(child: Text('No camera'))
                          : CameraPreview(camera),
                    ),
                    Padding(
                      padding: const EdgeInsets.all(12),
                      child: Column(
                        children: [
                          if (_error != null)
                            Padding(
                              padding: const EdgeInsets.only(bottom: 8),
                              child: ErrorStateText('Publish error: $_error'),
                            ),
                          Row(
                            mainAxisAlignment: MainAxisAlignment.spaceBetween,
                            children: [
                              Text(
                                  _h264Ready
                                      ? 'H.264 + JPEG stream'
                                      : 'MoQ JPEG stream',
                                  style: const TextStyle(
                                      fontWeight: FontWeight.bold)),
                              Text(
                                _broadcasting
                                    ? _h264Ready
                                        ? '$_h264Frames h264 · $_framesPublished jpeg · ON AIR'
                                        : '$_framesPublished frames · ON AIR'
                                    : 'OFF',
                                style: TextStyle(
                                  color: _broadcasting
                                      ? Colors.red.shade600
                                      : Colors.grey,
                                ),
                              ),
                              if (_onWireBytes != null)
                                Text(
                                  '$_onWireBytes B/group',
                                  style: const TextStyle(fontSize: 11),
                                ),
                            ],
                          ),
                          const SizedBox(height: 8),
                          Row(
                            mainAxisAlignment: MainAxisAlignment.center,
                            children: [
                              FilledButton.icon(
                                onPressed: _broadcasting
                                    ? null
                                    : () =>
                                        setState(() => _broadcasting = true),
                                icon: const Icon(Icons.play_arrow),
                                label: const Text('Go live'),
                              ),
                              const SizedBox(width: 12),
                              if (_micDeniedReason != null)
                                IconButton(
                                  tooltip: 'Mic denied — open app settings',
                                  onPressed: () =>
                                      PermissionsService.openSettings(),
                                  icon: const Icon(Icons.settings),
                                )
                              else
                                IconButton(
                                  tooltip: _micOn
                                      ? 'Mute microphone'
                                      : 'Enable microphone (AAC)',
                                  onPressed: _broadcasting
                                      ? () => _toggleMic(
                                          context.read<StreamingService>())
                                      : null,
                                  icon: Icon(
                                    _micOn ? Icons.mic : Icons.mic_off,
                                    color: _micOn ? Colors.red.shade600 : null,
                                  ),
                                ),
                              IconButton(
                                tooltip: _recording
                                    ? 'Stop recording'
                                    : 'Record broadcast (MP4)',
                                onPressed: _broadcasting ? _toggleRecord : null,
                                icon: Icon(
                                  _recording
                                      ? Icons.stop_circle
                                      : Icons.fiber_manual_record,
                                  color:
                                      _recording ? Colors.red.shade600 : null,
                                ),
                              ),
                              if (_audioReady)
                                Text(
                                  _micOn ? 'AAC ON' : 'AAC off',
                                  style: const TextStyle(fontSize: 12),
                                ),
                            ],
                          ),
                          ValueListenableBuilder<String?>(
                            valueListenable: AudioCodec.error,
                            builder: (_, err, __) => err == null
                                ? const SizedBox.shrink()
                                : Padding(
                                    padding: const EdgeInsets.only(top: 8),
                                    child: Text(
                                      'Audio error: $err',
                                      style:
                                          TextStyle(color: Colors.red.shade600),
                                    ),
                                  ),
                          ),
                          ValueListenableBuilder<String?>(
                            valueListenable: H264Codec.error,
                            builder: (_, err, __) => err == null
                                ? const SizedBox.shrink()
                                : Padding(
                                    padding: const EdgeInsets.only(top: 8),
                                    child: Text(
                                      'Encoder error: $err',
                                      style:
                                          TextStyle(color: Colors.red.shade600),
                                    ),
                                  ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
    );
  }
}
