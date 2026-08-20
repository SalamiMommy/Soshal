import 'package:flutter/material.dart';
import '../services/error_log.dart';

class ErrorStateText extends StatelessWidget {
  ErrorStateText(this.error, {super.key, this.onRetry}) {
    debugPrint('UI ERROR: $error');
    logRuntimeError('UI error: $error', StackTrace.current);
  }

  final Object error;
  final VoidCallback? onRetry;

  @override
  Widget build(BuildContext context) {
    final body = SelectableText('$error', textAlign: TextAlign.center);
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: onRetry == null
            ? body
            : Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  body,
                  const SizedBox(height: 8),
                  FilledButton.tonal(
                    onPressed: onRetry,
                    child: const Text('Retry'),
                  ),
                ],
              ),
      ),
    );
  }
}