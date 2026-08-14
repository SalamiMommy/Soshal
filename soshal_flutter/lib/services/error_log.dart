import 'dart:io';
import 'package:path_provider/path_provider.dart';

/// Appends a runtime error to `<app documents>/soshal-error.log` so failures
/// shown only in the debug banner / SnackBar remain readable and copyable.
/// Never throws: logging must not crash the app.
Future<void> logRuntimeError(Object error, [StackTrace? stack]) async {
  final buffer = StringBuffer()
    ..writeln('[${DateTime.now().toIso8601String()}] ERROR')
    ..writeln('$error');
  if (stack != null) buffer.writeln('$stack');
  buffer.writeln('---');
  try {
    final dir = await getApplicationDocumentsDirectory();
    await dir.create(recursive: true);
    final file = File('${dir.path}/soshal-error.log');
    await file.writeAsString(buffer.toString(), mode: FileMode.append);
  } catch (_) {}
}
