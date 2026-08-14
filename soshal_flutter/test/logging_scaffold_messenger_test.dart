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

    await tester.pumpWidget(
      LoggingScaffoldMessenger(
        child: MaterialApp(
          home: Builder(
            builder: (context) => Scaffold(
              body: Center(
                child: ElevatedButton(
                  onPressed: () => ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(content: Text('snack hello')),
                  ),
                  child: const Text('tap'),
                ),
              ),
            ),
          ),
        ),
      ),
    );

    await tester.tap(find.text('tap'));
    await tester.pumpAndSettle();

    final file = File('$dir/soshal-error.log');
    expect(await file.exists(), true);
    final content = await file.readAsString();
    expect(content, contains('SnackBar: snack hello'));
  });
}
