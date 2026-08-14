// ignore_for_file: invalid_use_of_internal_member
import 'dart:async';
import 'dart:ui' as ui;
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/media_service.dart';

/// Widget that renders an image decoded by Rust background worker threads.
/// Offloads image decoding off the Dart UI Isolate to maintain 60/120 FPS scrolling.
class RustNativeImage extends StatefulWidget {
  final String filePathOrUrl;
  final double? width;
  final double? height;
  final BoxFit fit;
  final WidgetBuilder? placeholderBuilder;
  final WidgetBuilder? errorBuilder;

  const RustNativeImage({
    super.key,
    required this.filePathOrUrl,
    this.width,
    this.height,
    this.fit = BoxFit.cover,
    this.placeholderBuilder,
    this.errorBuilder,
  });

  @override
  State<RustNativeImage> createState() => _RustNativeImageState();
}

class _RustNativeImageState extends State<RustNativeImage> {
  static final Map<String, ui.Image> _imageCache = {};
  static const int _maxCacheSize = 50;

  ui.Image? _decodedImage;
  bool _isFromCache = false;
  bool _isLoading = true;
  bool _hasError = false;

  @override
  void initState() {
    super.initState();
    _loadImage();
  }

  @override
  void didUpdateWidget(RustNativeImage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.filePathOrUrl != widget.filePathOrUrl) {
      _loadImage();
    }
  }

  Future<void> _loadImage() async {
    final cacheKey = widget.filePathOrUrl;
    if (_imageCache.containsKey(cacheKey)) {
      if (mounted) {
        setState(() {
          _decodedImage = _imageCache[cacheKey];
          _isFromCache = true;
          _isLoading = false;
          _hasError = false;
        });
      }
      return;
    }

    setState(() {
      _isLoading = true;
      _hasError = false;
    });

    try {
      final maxWidth = widget.width != null && widget.width! > 0
          ? (widget.width! * 2).toInt()
          : 1080;
      final maxHeight = widget.height != null && widget.height! > 0
          ? (widget.height! * 2).toInt()
          : 1080;

      final media = context.read<MediaService>();
      final dto = await media.decodeImageRgba(
        widget.filePathOrUrl,
        maxWidth: maxWidth,
        maxHeight: maxHeight,
      );

      final buffer = await ui.ImmutableBuffer.fromUint8List(dto.pixels);
      final descriptor = ui.ImageDescriptor.raw(
        buffer,
        width: dto.width,
        height: dto.height,
        pixelFormat: ui.PixelFormat.rgba8888,
      );

      final codec = await descriptor.instantiateCodec();
      final frameInfo = await codec.getNextFrame();

      if (_imageCache.length >= _maxCacheSize) {
        final oldestKey = _imageCache.keys.first;
        final oldImg = _imageCache.remove(oldestKey);
        oldImg?.dispose();
      }
      _imageCache[cacheKey] = frameInfo.image;

      if (mounted) {
        setState(() {
          _decodedImage = frameInfo.image;
          _isFromCache = true;
          _isLoading = false;
        });
      }
    } catch (_) {
      if (mounted) {
        setState(() {
          _hasError = true;
          _isLoading = false;
        });
      }
    }
  }

  @override
  void dispose() {
    if (!_isFromCache) {
      _decodedImage?.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (_isLoading) {
      return widget.placeholderBuilder != null
          ? widget.placeholderBuilder!(context)
          : Container(
              width: widget.width,
              height: widget.height,
              color: Colors.grey.withAlpha(50),
              child: const Center(
                  child: CircularProgressIndicator(strokeWidth: 2)),
            );
    }

    if (_hasError || _decodedImage == null) {
      return widget.errorBuilder != null
          ? widget.errorBuilder!(context)
          : Container(
              width: widget.width,
              height: widget.height,
              color: Colors.grey.withAlpha(30),
              child: const Icon(Icons.broken_image, color: Colors.grey),
            );
    }

    return RawImage(
      image: _decodedImage,
      width: widget.width,
      height: widget.height,
      fit: widget.fit,
    );
  }
}
