// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/services/theme_service.dart';

import 'helpers/test_env.dart';

late FakeApi api;

void main() {
  final env = bootstrapTestEnv('test-theme');
  api = env.$1;
  setUp(() {
    api.handlers.clear();
    api.calls.clear();
  });

  group('ThemeService', () {
    test('update toggles dark/light and notifies', () {
      final theme = ThemeService();
      var notified = 0;
      theme.addListener(() => notified++);

      expect(theme.isDark, isFalse);
      theme.update(bgLevel: 'dark');
      expect(theme.isDark, isTrue);
      expect(theme.bgLevel, 'dark');
      theme.update(bgLevel: 'light');
      expect(theme.isDark, isFalse);
      expect(notified, 2);
    });

    test('save persists options and theme via FFI settings', () async {
      final theme = ThemeService();
      theme.update(hue: 280, bgLevel: 'dark', fontScale: 1.2);
      api.stubBool('crateFfiDbDbSetSetting', true);

      await theme.save();

      final calls = api.callsOf('crateFfiDbDbSetSetting');
      expect(calls.length, 2);
      expect(api.namedArg(calls[0], 'key'), 'theme_options');
      final options =
          jsonDecode(api.namedArg(calls[0], 'value') as String)
              as Map<String, dynamic>;
      expect(options['bgLevel'], 'dark');
      expect(options['accentHue'], 280.0);
      expect(options['fontSizeScale'], 1.2);
      expect(api.namedArg(calls[1], 'key'), 'theme');
      expect(api.namedArg(calls[1], 'value'), 'dark');
    });

    test('save writes light theme name when bgLevel light', () async {
      final theme = ThemeService();
      api.stubBool('crateFfiDbDbSetSetting', true);
      await theme.save();
      final calls = api.callsOf('crateFfiDbDbSetSetting');
      expect(api.namedArg(calls[1], 'value'), 'light');
    });

    test('load restores saved options and notifies', () async {
      final theme = ThemeService();
      var notified = 0;
      theme.addListener(() => notified++);
      const options =
          '{"accentHue":280,"bgLevel":"deepest","customAccent":"#ff0000",'
          '"fontSizeScale":1.2,"fontFamily":"Serif"}';
      api.stub('crateFfiDbDbGetSetting', (inv) =>
          api.namedArg(inv, 'key') == 'theme_options' ? options : 'deepest');

      await theme.load();

      expect(theme.loaded, isTrue);
      expect(theme.hue, 280.0);
      expect(theme.bgLevel, 'deepest');
      expect(theme.customAccent, '#ff0000');
      expect(theme.fontScale, 1.2);
      expect(theme.fontFamily, 'Serif');
      expect(theme.isDark, isTrue);
      expect(notified, 1);
    });

    test('load swallows FFI errors but still marks loaded and notifies',
        () async {
      final theme = ThemeService();
      var notified = 0;
      theme.addListener(() => notified++);
      api.stub('crateFfiDbDbGetSetting', (_) => throw Exception('db down'));

      await theme.load();

      expect(theme.loaded, isTrue);
      expect(theme.bgLevel, 'light', reason: 'defaults kept');
      expect(notified, 1);
      expect(theme.load(), completes, reason: 'idempotent reload');
    });

    test('save swallows FFI errors and still notifies', () async {
      final theme = ThemeService();
      var notified = 0;
      theme.addListener(() => notified++);
      api.stub('crateFfiDbDbSetSetting', (_) => throw Exception('db down'));

      await theme.save();

      expect(notified, 1);
    });
  });
}