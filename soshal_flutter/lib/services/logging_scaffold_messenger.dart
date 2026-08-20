import 'package:flutter/material.dart';
import 'error_log.dart';

/// ScaffoldMessenger that appends every SnackBar's text to
/// soshal-error.log. Installed in main.dart via MaterialApp's
/// scaffoldMessengerKey; because showSnackBar is overridden here, every
/// in-app error message (58+ call sites) gets logged for later copy/paste —
/// no per-call-site changes needed.
class LoggingScaffoldMessenger extends ScaffoldMessenger {
  const LoggingScaffoldMessenger({super.key, required super.child});

  @override
  ScaffoldMessengerState createState() => LoggingScaffoldMessengerState();
}

class LoggingScaffoldMessengerState extends ScaffoldMessengerState {
  @override
  ScaffoldFeatureController<SnackBar, SnackBarClosedReason> showSnackBar(
    SnackBar snackBar, {
    AnimationStyle? snackBarAnimationStyle,
  }) {
    final text = _snackBarText(snackBar.content);
    if (text != null && text.isNotEmpty) {
      logRuntimeError('SnackBar: $text');
      debugPrint('SNACKBAR SITE: ${redactSensitive(text)}\n'
          '${redactSensitive(StackTrace.current.toString())}');
    }
    return super
        .showSnackBar(snackBar, snackBarAnimationStyle: snackBarAnimationStyle);
  }

  String? _snackBarText(Widget content) {
    if (content is Text) return content.data;
    if (content is SelectableText) return content.data;
    if (content is RichText) return content.text.toPlainText();
    return content.toStringShort();
  }
}
