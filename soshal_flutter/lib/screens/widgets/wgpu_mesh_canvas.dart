// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:convert';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';
import 'package:soshal_flutter/frb_generated.dart';

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
  int? _sessionId;
  ui.Image? _renderedImage;
  Timer? _renderTimer;
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _initWgpuSession();
  }

  Future<void> _initWgpuSession() async {
    try {
      final jsonStr = RustLib.instance.api.crateFfiRenderRenderCreateSession(
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
    _renderTimer = Timer.periodic(const Duration(milliseconds: 33), (_) async {
      if (_sessionId == null || !mounted) return;
      try {
        final nodesJson =
            jsonEncode(widget.nodes.map((n) => n.toJson()).toList());
        final frameBytes =
            RustLib.instance.api.crateFfiRenderRenderComputeMeshFrame(
          sessionId: PlatformInt64Util.from(_sessionId!),
          nodesJson: nodesJson,
          deltaTime: 0.033,
        );

        if (frameBytes.isNotEmpty) {
          // Allocate a shared frame buffer on the Rust raster side before
          // painting (Android-only path; failures are silent).
          BigInt? bufferPtr;
          try {
            final fb = await RustLib.instance.api
                .crateFfiRasterRasterAllocateFrameBuffer(
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
            if (bufferPtr != null) {
              try {
                await RustLib.instance.api
                    .crateFfiRasterRasterSignalImpellerFrameReady(
                  textureId: PlatformInt64Util.from(bufferPtr.toInt()),
                  frameTimestampNs:
                      BigInt.from(DateTime.now().microsecondsSinceEpoch * 1000),
                );
              } catch (_) {}
            }
          } else {
            img.dispose();
          }
        }
      } catch (e) {
        // Suppress frame loop transient errors gracefully
      }
    });
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
      return Container(
        width: widget.width,
        height: widget.height,
        color: const Color(0xFF0D1117),
        child: ErrorStateText('WGPU Compute Error: $_error'),
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
