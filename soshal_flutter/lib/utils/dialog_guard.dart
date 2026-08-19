import 'dart:async';
import 'package:flutter/material.dart';

/// Shows a dialog after the current frame completes.
///
/// Pushing a dialog while another route is mid-transition (popup menu closing,
/// previous dialog popping) — or while pointer events are still being
/// dispatched — can leave the new dialog's render tree built but never laid
/// out. The next pointer event then hit-tests it and Flutter throws
/// "Cannot hit test a render box with no size". Deferring the push past the
/// current frame guarantees the dialog is built and laid out before any
/// subsequent hit test.
Future<T?> showDialogDeferred<T>({
  required BuildContext context,
  required WidgetBuilder builder,
  bool barrierDismissible = true,
}) {
  final navigator = Navigator.of(context);
  final completer = Completer<T?>();
  WidgetsBinding.instance.addPostFrameCallback((_) async {
    if (!navigator.mounted) {
      completer.complete(null);
      return;
    }
    try {
      completer.complete(await showDialog<T>(
        context: navigator.context,
        barrierDismissible: barrierDismissible,
        builder: builder,
      ));
    } catch (_) {
      completer.complete(null);
    }
  });
  return completer.future;
}
