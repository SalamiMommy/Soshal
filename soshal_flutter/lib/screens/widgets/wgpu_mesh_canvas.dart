import 'dart:async';
import 'dart:convert';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:provider/provider.dart';

import '../../services/layout_service.dart';
import '../../widgets/error_state_text.dart';

/// WGPU Mesh Node representation for Flutter rendering canvas
class WgpuMeshNodeItem {
  final String id;
  final double x;
  final double y;
  final double z;
  final int latencyMs;

  WgpuMeshNodeItem({
    required this.id,
    required this.x,
    required this.y,
    required this.z,
    required this.latencyMs,
  });

  Map<String, dynamic> toJson() => {
        'id': id,
        'x': x,
        'y': y,
        'z': z,
        'vx': 0.05,
        'vy': 0.05,
        'vz': 0.0,
        'latency_ms': latencyMs,
        'connections': <String>[],
      };
}

/// Interactive 3D P2P Mesh Topology Canvas Widget powered by WGPU Compute Shaders in Rust
class WgpuMeshCanvasWidget extends StatefulWidget {
  final double width;
  final double height;
  final List<WgpuMeshNodeItem> nodes;

  const WgpuMeshCanvasWidget({
    super.key,
    this.width = 320.0,
    this.height = 240.0,
    this.nodes = const [],
  });

  @override
  State<WgpuMeshCanvasWidget> createState() => _WgpuMeshCanvasWidgetState();
}

class _WgpuMeshCanvasWidgetState extends State<WgpuMeshCanvasWidget> {
  late final LayoutService _layout;
  int? _sessionId;
  ui.Image? _renderedImage;
  Timer? _renderTimer;
  List<WgpuMeshNodeItem>? _lastRenderedNodes;
  int _idleTicks = 0;
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _layout = context.read<LayoutService>();
    _initWgpuSession();
  }

  @override
  void didUpdateWidget(covariant WgpuMeshCanvasWidget oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (!identical(oldWidget.nodes, widget.nodes)) {
      _idleTicks = 0;
      _startRenderLoop();
    }
  }

  Future<void> _initWgpuSession() async {
    try {
      _layout.updateViewMetrics(
        screenWidth:
            (widget.width * MediaQuery.devicePixelRatioOf(context)).round(),
        textScale: MediaQuery.textScalerOf(context).scale(1.0),
      );
      final jsonStr = _layout.createRenderSession(
        width: widget.width.toInt(),
        height: widget.height.toInt(),
      );
      final Map<String, dynamic> cfg =
          Map<String, dynamic>.from(jsonDecode(jsonStr) as Map);
      _sessionId = (cfg['session_id'] as num?)?.toInt();

      if (_sessionId != null) {
        _startRenderLoop();
      }
      setState(() {
        _loading = false;
      });
    } catch (e) {
      setState(() {
        _error = e.toString();
        _loading = false;
      });
    }
  }

  void _startRenderLoop() {
    _renderTimer?.cancel();
    _renderTimer = Timer(const Duration(milliseconds: 33), _renderTick);
  }

  Future<void> _renderTick() async {
    _renderTimer?.cancel();
    if (_sessionId == null || !mounted) return;
    final nodes = widget.nodes;
    if (identical(nodes, _lastRenderedNodes)) {
      // No state change: idle backoff, then stop the loop entirely.
      _idleTicks++;
      if (_idleTicks >= 8) return;
      _renderTimer = Timer(const Duration(milliseconds: 250), _renderTick);
      return;
    }
    _idleTicks = 0;
    try {
      final nodesJson = jsonEncode(nodes.map((n) => n.toJson()).toList());
      final frameBytes = await _layout.renderMeshFrame(
        sessionId: _sessionId!,
        nodesJson: nodesJson,
        deltaTime: 0.033,
      );

      if (frameBytes.isNotEmpty) {
        // Allocate a shared frame buffer on the Rust raster side before
        // painting (Android-only path; failures are silent).
        BigInt? bufferPtr;
        try {
          final fb = await _layout.allocateRasterFrameBuffer(
            width: widget.width.toInt(),
            height: widget.height.toInt(),
          );
          bufferPtr = fb.bufferPtrAddr;
        } catch (_) {}

        final completer = Completer<ui.Image>();
        ui.decodeImageFromPixels(
          frameBytes,
          widget.width.toInt(),
          widget.height.toInt(),
          ui.PixelFormat.rgba8888,
          completer.complete,
        );
        final img = await completer.future;
        if (mounted) {
          final oldImg = _renderedImage;
          setState(() {
            _renderedImage = img;
          });
          oldImg?.dispose();
          _lastRenderedNodes = nodes;
          if (bufferPtr != null) {
            try {
              await _layout.signalRasterFrameReady(
                textureId: bufferPtr.toInt(),
                frameTimestampNs:
                    BigInt.from(DateTime.now().microsecondsSinceEpoch * 1000),
              );
            } catch (_) {}
            // The buffer is consumed by the frame signal; release the Rust
            // allocation so the registry does not grow one entry per frame.
            try {
              await _layout.releaseRasterFrameBuffer(ptrAddr: bufferPtr);
            } catch (_) {}
          }
        } else {
          img.dispose();
        }
      }
    } catch (e) {
      // Persistent render failure: stop the loop, surface error state.
      _renderTimer?.cancel();
      if (mounted) {
        setState(() {
          _error = 'Mesh render failed: $e';
        });
      }
    } finally {
      if (mounted && _error == null) {
        _renderTimer ??= Timer(const Duration(milliseconds: 33), _renderTick);
      }
    }
  }

  void _retry() {
    setState(() {
      _error = null;
      _loading = true;
      _lastRenderedNodes = null;
    });
    _initWgpuSession();
  }

  @override
  void dispose() {
    _renderTimer?.cancel();
    _renderedImage?.dispose();
    _renderedImage = null;
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) {
      return Container(
        width: widget.width,
        height: widget.height,
        color: const Color(0xFF0D1117),
        child: const Center(
          child: CircularProgressIndicator(color: Color(0xFF00E5FF)),
        ),
      );
    }

    if (_error != null) {
      return GestureDetector(
        onTap: _retry,
        child: Container(
          width: widget.width,
          height: widget.height,
          color: const Color(0xFF0D1117),
          child: ErrorStateText('WGPU Compute Error: $_error\nTap to retry'),
        ),
      );
    }

    return ClipRRect(
      borderRadius: BorderRadius.circular(12),
      child: Container(
        width: widget.width,
        height: widget.height,
        color: const Color(0xFF0D1117),
        child: _renderedImage == null
            ? const SizedBox.shrink()
            : RawImage(
                image: _renderedImage,
                width: widget.width,
                height: widget.height,
                fit: BoxFit.cover,
              ),
      ),
    );
  }
}
