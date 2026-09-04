// ignore_for_file: invalid_use_of_internal_member
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/logging_scaffold_messenger.dart';

import 'helpers/test_env.dart';

void main() {
  final env = bootstrapTestEnv('test-logging-sm');

  testWidgets('showSnackBar logs text to error log', (tester) async {
    final dir = env.$2;
    final logFile = File('$dir/soshal-error.log');
    if (logFile.existsSync()) logFile.deleteSync();

    await tester.pumpWidget(
      LoggingScaffoldMessenger(
        child: MediaQuery(
          data: const MediaQueryData(size: Size(800, 600)),
          child: Directionality(
            textDirection: TextDirection.ltr,
            child: Material(child: const Scaffold(body: Center())),
          ),
        ),
      ),
    );

    // Trigger inside runAsync: logRuntimeError's dart:io writes only
    // complete on the real event loop, which the FakeAsync test zone
    // never turns.
    await tester.runAsync(() async {
      tester
          .state<LoggingScaffoldMessengerState>(
            find.byType(LoggingScaffoldMessenger),
          )
          .showSnackBar(const SnackBar(content: Text('snack hello')));
      await Future<void>.delayed(const Duration(milliseconds: 200));
    });
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 350));

    expect(logFile.existsSync(), true);
    expect(logFile.readAsStringSync(), contains('SnackBar: snack hello'));
  }, timeout: const Timeout(Duration(seconds: 15)));
}