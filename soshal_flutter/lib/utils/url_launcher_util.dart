import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

/// Safe URL launcher utility providing validation and user feedback.
class UrlLauncherUtil {
  const UrlLauncherUtil._();

  /// Validates and launches a URL string in the external system browser or appropriate app.
  /// If launching fails and [context] is provided, displays a brief snackbar notification.
  static Future<bool> launchSafeUrl(
    BuildContext? context,
    String urlString, {
    LaunchMode mode = LaunchMode.platformDefault,
  }) async {
    final trimmed = urlString.trim();
    if (trimmed.isEmpty) return false;

    final uri = Uri.tryParse(trimmed);
    if (uri == null) {
      _showFeedback(context, 'Invalid URL format: $urlString');
      return false;
    }

    // Only allow safe, known schemes
    final scheme = uri.scheme.toLowerCase();
    if (scheme != 'https' && scheme != 'http' && scheme != 'mailto') {
      _showFeedback(context, 'Unsupported link scheme: $scheme');
      return false;
    }

    try {
      final launched = await launchUrl(uri, mode: mode);
      if (!launched) {
        if (context != null && context.mounted) {
          _showFeedback(context, 'Could not open link: $urlString');
        }
        return false;
      }
      return true;
    } catch (e) {
      if (context != null && context.mounted) {
        _showFeedback(context, 'Failed to open link: $e');
      }
      return false;
    }
  }

  static void _showFeedback(BuildContext? context, String message) {
    if (context == null || !context.mounted) return;
    ScaffoldMessenger.of(context).hideCurrentSnackBar();
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(
        content: Text(message),
        duration: const Duration(seconds: 2),
      ),
    );
  }
}
