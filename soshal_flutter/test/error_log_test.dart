// ignore_for_file: invalid_use_of_internal_member
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/error_log.dart';

import 'package:soshal_flutter/test/helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-error-log');

  test('logRuntimeError writes to soshal-error.log', () async {
    final dir = env.$2;
    await logRuntimeError('test-error', StackTrace.current);

    final file = File('$dir/soshal-error.log');
    final exists = await file.exists();
    expect(exists, true);

    final content = await file.readAsString();
    expect(content, contains('test-error'));
  });

  test('LastErrorMixin sets and clears lastError and logs', () async {
    final dir = env.$2;

    class Dummy with LastErrorMixin {}

    final d = Dummy();
    expect(d.lastError, isNull);

    d.setLastError(Exception('boom'), StackTrace.current);
    expect(d.lastError, isNotNull);

    final file = File('$dir/soshal-error.log');
    final content = await file.readAsString();
    expect(content, contains('svc: Exception: boom'));

    d.clearLastError();
    expect(d.lastError, isNull);
  });
}
