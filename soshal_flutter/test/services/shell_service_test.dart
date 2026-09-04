// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/shell_service.dart';

import '../helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-shell');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('ShellService', () {
    test('moveItem reorders nav, persists order, notifies', () {
      final shell = ShellService();
      var notified = 0;
      shell.addListener(() => notified++);
      api.stubBool('crateFfiDbDbSetSetting', true);

      shell.moveItem(0, 3);

      expect(shell.items[3].id, 'feed');
      expect(shell.items.first.id, 'notifications');
      final inv = api.callsOf('crateFfiDbDbSetSetting').single;
      expect(api.namedArg(inv, 'key'), 'sidebar_order');
      final order =
          jsonDecode(api.namedArg(inv, 'value') as String) as List<dynamic>;
      expect((order[3] as Map<String, dynamic>)['id'], 'feed');
      expect(notified, 1);
    });

    test('moveItem out of range is a no-op', () {
      final shell = ShellService();
      var notified = 0;
      shell.addListener(() => notified++);

      shell.moveItem(-1, 0);
      shell.moveItem(0, 99);

      expect(api.callCount('crateFfiDbDbSetSetting'), 0);
      expect(notified, 0);
    });

    test('setExtraItemsVisible appends conditionals and persists', () {
      final shell = ShellService();
      api.stubBool('crateFfiDbDbSetSetting', true);

      shell.setExtraItemsVisible(const ['network', 'vouch']);

      expect(shell.items.any((i) => i.id == 'network'), isTrue);
      expect(shell.items.any((i) => i.id == 'vouch'), isTrue);
      expect(api.callCount('crateFfiDbDbSetSetting'), 1);
    });

    test('initialize loads sidebar, theme, biometrics and lock state',
        () async {
      final shell = ShellService();
      var notified = 0;
      shell.addListener(() => notified++);
      api.stub('crateFfiDbDbGetSetting', (inv) {
        switch (api.namedArg(inv, 'key')) {
          case 'sidebar_order':
            return '[{"id":"feed","label":"Feed"},'
                '{"id":"settings","label":"Settings"}]';
          case 'theme':
            return 'dark';
          case 'theme_options':
            return '{}';
          case 'biometrics_enabled':
            return 'true';
        }
        return null;
      });
      api.stubBool('crateFfiPinPinHas', true);
      api.stubString(
        'crateFfiPinPinLockoutState',
        '{"attemptCount":2,"lockoutUntil":0,"permanentLocked":false}',
      );

      await shell.initialize();

      expect(shell.initialized, isTrue);
      expect(shell.items.length, 2);
      expect(shell.items.first.id, 'feed');
      expect(shell.theme, 'dark');
      expect(shell.biometricsEnabled, isTrue);
      expect(shell.locked, isTrue);
      expect(shell.lockAttempts, 2);
      expect(api.callCount('crateFfiPinPinLockoutState'), 1);
      expect(notified, 1);
    });

    test('initialize is idempotent after first load', () async {
      final shell = ShellService();
      api.stub('crateFfiDbDbGetSetting', (inv) => null);
      api.stubBool('crateFfiPinPinHas', false);

      await shell.initialize();
      await shell.initialize();

      expect(api.callCount('crateFfiPinPinHas'), 1);
    });

    test('initialize swallows FFI errors, stays usable and retries',
        () async {
      final shell = ShellService();
      api.stub('crateFfiDbDbGetSetting', (_) => throw Exception('db down'));

      await shell.initialize();

      // On load failure the flag stays false so a later call retries
      // (avoids the PIN-lock state never being established on first try).
      expect(shell.initialized, isFalse);
      expect(shell.items.length, ShellService.defaultItems.length);

      // A subsequent successful load completes initialization.
      api.stub('crateFfiDbDbGetSetting', (_) => null);
      api.stubBool('crateFfiPinPinHas', false);
      await shell.initialize();
      expect(shell.initialized, isTrue);
    });

    test('unlock verifies PIN, unlocks and notifies', () async {
      final shell = ShellService();
      var notified = 0;
      shell.addListener(() => notified++);
      api.stubBool('crateFfiPinPinVerify', true);

      final ok = await shell.unlock('1234');

      expect(ok, isTrue);
      expect(shell.locked, isFalse);
      final inv = api.callsOf('crateFfiPinPinVerify').single;
      expect(api.namedArg(inv, 'pin'), '1234');
      expect(notified, greaterThanOrEqualTo(2));
    });

    test('unlock wrong PIN sets error and refreshes lockout', () async {
      final shell = ShellService();
      api.stubBool('crateFfiPinPinVerify', false);
      api.stubString(
        'crateFfiPinPinLockoutState',
        '{"attemptCount":3,"lockoutUntil":0,"permanentLocked":false}',
      );

      final ok = await shell.unlock('0000');

      expect(ok, isFalse);
      expect(shell.locked, isTrue);
      expect(shell.unlockError, 'Wrong PIN');
      expect(shell.lockAttempts, 3);
      expect(api.callCount('crateFfiPinPinLockoutState'), 1);
    });

    test('unlock empty PIN returns false without FFI call', () async {
      final shell = ShellService();
      expect(await shell.unlock(''), isFalse);
      expect(api.callCount('crateFfiPinPinVerify'), 0);
    });

    test('setPin persists PIN via FFI and notifies', () async {
      final shell = ShellService();
      var notified = 0;
      shell.addListener(() => notified++);
      api.stubBool('crateFfiPinPinSet', true);

      final ok = await shell.setPin('4321');

      expect(ok, isTrue);
      expect(shell.hasPin, isTrue);
      final inv = api.callsOf('crateFfiPinPinSet').single;
      expect(api.namedArg(inv, 'pin'), '4321');
      expect(notified, 1);
    });
  });
}