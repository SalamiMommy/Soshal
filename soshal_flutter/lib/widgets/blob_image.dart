// ignore_for_file: invalid_use_of_internal_member
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/media_service.dart';
import '../services/p2p_service.dart';
import '../utils/blob_resolver.dart';
import '../utils/safe_url.dart';
import 'rust_native_image.dart';

/// Renders an image whose source is either an http(s) URL or a local CAS
/// blob reference (`blob://<hash>` or `n<hash>`, plus bare 64-hex hashes).
/// Blob hashes resolve through the chunk store first, then fall back to
/// LAN peer swarm fetch so peers can serve each other's images.
class BlobImage extends StatefulWidget {
  /// Image source: URL, `blob://hash`, `n<hash>`, or bare 64-hex hash.
  final String source;
  final double? width;
  final double? height;
  final BoxFit fit;
  final WidgetBuilder? placeholderBuilder;
  final WidgetBuilder? errorBuilder;

  const BlobImage({
    super.key,
    required this.source,
    this.width,
    this.height,
    this.fit = BoxFit.cover,
    this.placeholderBuilder,
    this.errorBuilder,
  });

  @override
  State<BlobImage> createState() => _BlobImageState();
}

class _BlobImageState extends State<BlobImage> {
  String? _path;
  bool _isUrl = false;
  bool _failed = false;
  int _generation = 0;

  static final _hashRe = RegExp(r'^[0-9a-f]{64}$');

  static String? hashFromSource(String source) {
    final trimmed = source.trim();
    if (trimmed.isEmpty) return null;
    if (trimmed.startsWith('blob://')) {
      final rest = trimmed.substring('blob://'.length);
      if (_hashRe.hasMatch(rest)) return rest;
    }
    if (trimmed.startsWith('n')) {
      final rest = trimmed.substring(1);
      if (_hashRe.hasMatch(rest)) return rest;
    }
    if (_hashRe.hasMatch(trimmed)) return trimmed;
    return null;
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) _resolve();
    });
  }

  @override
  void didUpdateWidget(BlobImage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.source != widget.source) {
      _generation++;
      _path = null;
      _isUrl = false;
      _failed = false;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _resolve();
      });
    }
  }

  Future<void> _resolve() async {
    final gen = _generation;
    final src = widget.source.trim();
    if (src.isEmpty) {
      setState(() => _failed = true);
      return;
    }
    final hash = hashFromSource(src);
    if (hash == null) {
      final uri = Uri.tryParse(src);
      if (uri != null &&
          (uri.scheme == 'http' || uri.scheme == 'https') &&
          SafeUrl.isSafeMediaUrl(src)) {
        setState(() => _isUrl = true);
      } else {
        setState(() => _failed = true);
      }
      return;
    }
    final media = context.read<MediaService>();
    final p2p = context.read<P2pService>();
    final path = await resolveBlobPath(media, p2p, hash);
    if (!mounted || gen != _generation) return;
    if (path == null) {
      setState(() => _failed = true);
    } else {
      setState(() => _path = path);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_failed) {
      return widget.errorBuilder != null
          ? widget.errorBuilder!(context)
          : Container(
              width: widget.width,
              height: widget.height,
              color: Colors.grey.withAlpha(30),
              child: const Icon(Icons.broken_image, color: Colors.grey),
            );
    }
    if (_isUrl) {
      return Image.network(
        widget.source.trim(),
        width: widget.width,
        height: widget.height,
        fit: widget.fit,
        errorBuilder: (_, __, ___) => widget.errorBuilder != null
            ? widget.errorBuilder!(context)
            : Container(
                width: widget.width,
                height: widget.height,
                color: Colors.grey.withAlpha(30),
                child: const Icon(Icons.broken_image, color: Colors.grey),
              ),
      );
    }
    if (_path != null) {
      return RustNativeImage(
        filePathOrUrl: _path!,
        width: widget.width,
        height: widget.height,
        fit: widget.fit,
        placeholderBuilder: widget.placeholderBuilder,
        errorBuilder: widget.errorBuilder,
      );
    }
    return widget.placeholderBuilder != null
        ? widget.placeholderBuilder!(context)
        : Container(
            width: widget.width,
            height: widget.height,
            color: Colors.grey.withAlpha(50),
            child: const Center(
              child: CircularProgressIndicator(strokeWidth: 2),
            ),
          );
  }
}
