import 'dart:io';
import 'package:cached_network_image/cached_network_image.dart';
import 'package:flutter/material.dart';
import 'package:photo_view/photo_view.dart';

/// Fullscreen pinch-to-zoom and pan lightbox modal for media exploration.
class PhotoLightbox extends StatelessWidget {
  final ImageProvider imageProvider;
  final String? heroTag;

  const PhotoLightbox({
    super.key,
    required this.imageProvider,
    this.heroTag,
  });

  /// Displays the photo lightbox modal over the current screen.
  static Future<void> show(
    BuildContext context, {
    required String imageUrl,
    String? heroTag,
    File? localFile,
  }) {
    final ImageProvider provider;
    if (localFile != null && localFile.existsSync()) {
      provider = FileImage(localFile);
    } else if (imageUrl.startsWith('file://')) {
      provider = FileImage(File(Uri.parse(imageUrl).toFilePath()));
    } else if (imageUrl.startsWith('http://') ||
        imageUrl.startsWith('https://')) {
      provider = CachedNetworkImageProvider(imageUrl);
    } else {
      provider = FileImage(File(imageUrl));
    }

    return Navigator.of(context).push(
      PageRouteBuilder(
        opaque: false,
        barrierDismissible: true,
        barrierColor: Colors.black.withAlpha(230),
        pageBuilder: (context, animation, secondaryAnimation) {
          return FadeTransition(
            opacity: animation,
            child: PhotoLightbox(
              imageProvider: provider,
              heroTag: heroTag,
            ),
          );
        },
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    Widget photoView = PhotoView(
      imageProvider: imageProvider,
      heroAttributes:
          heroTag != null ? PhotoViewHeroAttributes(tag: heroTag!) : null,
      minScale: PhotoViewComputedScale.contained,
      maxScale: PhotoViewComputedScale.covered * 3.0,
      backgroundDecoration: const BoxDecoration(color: Colors.transparent),
      loadingBuilder: (context, event) => const Center(
        child: CircularProgressIndicator(strokeWidth: 2),
      ),
      errorBuilder: (context, error, stackTrace) => const Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.broken_image, color: Colors.white70, size: 48),
            SizedBox(height: 8),
            Text('Failed to load image',
                style: TextStyle(color: Colors.white70)),
          ],
        ),
      ),
    );

    return Scaffold(
      backgroundColor: Colors.transparent,
      body: Stack(
        children: [
          Positioned.fill(
            child: GestureDetector(
              onTap: () => Navigator.of(context).pop(),
              child: photoView,
            ),
          ),
          SafeArea(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
              child: Align(
                alignment: Alignment.topRight,
                child: IconButton.filledTonal(
                  icon: const Icon(Icons.close, color: Colors.white),
                  style: IconButton.styleFrom(
                    backgroundColor: Colors.black54,
                  ),
                  onPressed: () => Navigator.of(context).pop(),
                  tooltip: 'Close',
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
