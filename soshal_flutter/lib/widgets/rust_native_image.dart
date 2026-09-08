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
  static final Map<ui.Image, int> _imageRefs = {};
  static const int _maxCacheSize = 48;

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
      _releaseCurrentImage();
      _loadImage();
    }
  }

  void _releaseCurrentImage() {
    if (_decodedImage != null) {
      if (_isFromCache) {
        final count = (_imageRefs[_decodedImage!] ?? 1) - 1;
        if (count <= 0) {
          _imageRefs.remove(_decodedImage!);
          // If not in cache anymore, dispose now
          if (!_imageCache.containsValue(_decodedImage!)) {
            _decodedImage!.dispose();
          }
        } else {
          _imageRefs[_decodedImage!] = count;
        }
      } else {
        _decodedImage!.dispose();
      }
      _decodedImage = null;
      _isFromCache = false;
    }
  }

  Future<void> _loadImage() async {
    final cacheKey = widget.filePathOrUrl;
    if (_imageCache.containsKey(cacheKey)) {
      final cached = _imageCache.remove(cacheKey)!;
      // Re-insert at end for true LRU
      _imageCache[cacheKey] = cached;
      _releaseCurrentImage();
      _decodedImage = cached;
      _isFromCache = true;
      _imageRefs[cached] = (_imageRefs[cached] ?? 0) + 1;
      if (mounted) {
        setState(() {
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
        final evicted = _imageCache.remove(oldestKey);
        if (evicted != null) {
          final refs = _imageRefs[evicted] ?? 0;
          if (refs <= 0) {
            evicted.dispose();
            _imageRefs.remove(evicted);
          }
        }
      }
      _imageCache[cacheKey] = frameInfo.image;

      _releaseCurrentImage();
      _decodedImage = frameInfo.image;
      _isFromCache = true;
      _imageRefs[frameInfo.image] = (_imageRefs[frameInfo.image] ?? 0) + 1;

      if (mounted) {
        setState(() {
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
    _releaseCurrentImage();
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
