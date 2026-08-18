// ignore_for_file: invalid_use_of_internal_member
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:soshal_flutter/frb_generated.dart';
import '../theme/app_theme.dart';

/// Theme customization state (accent hue, background level, font scale,
/// font family, custom accent). Persisted in the `theme_options` settings
/// key — same shape as the legacy rust-native customization page.
class ThemeService extends ChangeNotifier {
  static const _optionsKey = 'theme_options';
  static const _themeKey = 'theme';

  static const defaultBackgroundImage = 'assets/clouds.jpg';

  double hue = 195.0;
  String bgLevel = 'light';
  String customAccent = '';
  double fontScale = 1.0;
  String fontFamily = 'default';
  String backgroundImage = defaultBackgroundImage;

  bool _loaded = false;
  bool get loaded => _loaded;

  Future<void> load() async {
    if (_loaded) return;
    try {
      final raw = RustLib.instance.api.crateFfiDbDbGetSetting(key: _optionsKey);
      if (raw != null && raw.isNotEmpty) {
        try {
          final v = jsonDecode(raw) as Map<String, dynamic>;
          hue = (v['accentHue'] as num?)?.toDouble() ?? hue;
          bgLevel = v['bgLevel'] as String? ?? bgLevel;
          customAccent = v['customAccent'] as String? ?? '';
          fontScale = (v['fontSizeScale'] as num?)?.toDouble() ?? 1.0;
          fontFamily = v['fontFamily'] as String? ?? 'default';
          backgroundImage =
              v['backgroundImage'] as String? ?? defaultBackgroundImage;
        } catch (_) {}
      }
      final theme = RustLib.instance.api.crateFfiDbDbGetSetting(key: _themeKey);
      if (theme != null && theme.isNotEmpty) {
        final t = theme.toLowerCase();
        if (t == 'dark' || t == 'darker' || t == 'deepest') {
          bgLevel = t;
        } else if (t == 'light') {
          bgLevel = 'light';
        }
      }
    } catch (e) {
      debugPrint('theme load: $e');
    }
    _loaded = true;
    notifyListeners();
  }

  Future<void> save() async {
    try {
      RustLib.instance.api.crateFfiDbDbSetSetting(
        key: _optionsKey,
        value: jsonEncode({
          'accentHue': hue,
          'bgLevel': bgLevel,
          'customAccent': customAccent,
          'fontSizeScale': fontScale,
          'fontFamily': fontFamily,
          'backgroundImage': backgroundImage,
        }),
      );
      final themeName = bgLevel == 'light' ? 'light' : 'dark';
      RustLib.instance.api.crateFfiDbDbSetSetting(
        key: _themeKey,
        value: themeName,
      );
    } catch (e) {
      debugPrint('theme save: $e');
    }
    notifyListeners();
  }

  void update({
    double? hue,
    String? bgLevel,
    String? customAccent,
    double? fontScale,
    String? fontFamily,
    String? backgroundImage,
  }) {
    if (hue != null) this.hue = hue;
    if (bgLevel != null) this.bgLevel = bgLevel;
    if (customAccent != null) this.customAccent = customAccent;
    if (fontScale != null) this.fontScale = fontScale;
    if (fontFamily != null) this.fontFamily = fontFamily;
    if (backgroundImage != null) this.backgroundImage = backgroundImage;
    notifyListeners();
  }

  bool get isDark =>
      bgLevel == 'dark' || bgLevel == 'darker' || bgLevel == 'deepest';

  /// Build a theme using the new Frutiger Aero glass theme system.
  ThemeData themeFor() {
    return AppTheme.createTheme(
      accentHue: hue,
      bgLevel: bgLevel,
      fontFamily: fontFamily,
      fontScale: fontScale,
      customAccent: customAccent.isNotEmpty ? customAccent : null,
    );
  }
}
