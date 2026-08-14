import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import '../services/error_log.dart';

class ErrorStateText extends StatelessWidget {
  ErrorStateText(this.error, {super.key}) {
    debugPrint('UI ERROR: $error');
    logRuntimeError('UI error: $error', StackTrace.current);
  }

  final Object error;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: SelectableText('$error'),
      ),
    );
  }
}
