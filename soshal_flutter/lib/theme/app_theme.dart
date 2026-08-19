import 'package:flutter/material.dart';

class AppTheme {
  // Frutiger Aero glass palette configuration
  static const Map<String, _BgConfig> _bgConfigs = {
    'light': _BgConfig(
      hue: 210,
      bgSat: 30,
      surfSat: 25,
      bg: 96,
      surface: 90,
      elevated: 98,
      border: 82,
      surfAlpha: 0.5,
      elevAlpha: 0.58,
    ),
    'dark': _BgConfig(
      hue: 220,
      bgSat: 10,
      surfSat: 10,
      bg: 4,
      surface: 9,
      elevated: 14,
      border: 18,
      surfAlpha: 0.65,
      elevAlpha: 0.75,
    ),
    'darker': _BgConfig(
      hue: 220,
      bgSat: 10,
      surfSat: 8,
      bg: 3,
      surface: 6,
      elevated: 10,
      border: 14,
      surfAlpha: 0.6,
      elevAlpha: 0.7,
    ),
    'deepest': _BgConfig(
      hue: 220,
      bgSat: 10,
      surfSat: 6,
      bg: 2,
      surface: 3,
      elevated: 7,
      border: 10,
      surfAlpha: 0.55,
      elevAlpha: 0.65,
    ),
  };

  static const Map<String, String> _fontFamilyMap = {
    'default': 'NotoSans',
    'serif': 'Georgia',
    'monospace': 'Courier',
    'retro': 'NotoSans',
    'funky': 'NotoSans',
  };

  // Layout constants
  static const _spacing = {
    'xs2': 2.0,
    'xs': 4.0,
    'sm': 8.0,
    'md': 16.0,
    'md2': 12.0,
    'lg': 24.0,
    'lg2': 20.0,
    'xl': 32.0,
    'xl2': 40.0,
    'xxl': 48.0,
    'xxl2': 64.0,
    'xxxl': 96.0,
  };

  static const _borderRadius = {
    'sm': 16.0,
    'md': 24.0,
    'lg': 32.0,
    'round': 9999.0,
  };

  static ThemeData createTheme({
    double accentHue = 195.0,
    String bgLevel = 'light',
    String fontFamily = 'default',
    double fontScale = 1.0,
    String? customAccent,
    String? fontColor,
  }) {
    final bgConfig = _bgConfigs[bgLevel] ?? _bgConfigs['light']!;
    final isLight = bgLevel == 'light';
    final ff = _fontFamilyMap[fontFamily] ?? 'NotoSans';

    // Generate HSL colors
    final primary = _hsl(accentHue, 85, 55);
    final secondary = _hsl(accentHue + 55, 85, 55);

    final surfaceColor = _hsla(
        bgConfig.hue, bgConfig.surfSat, bgConfig.surface, bgConfig.surfAlpha);
    final elevatedColor = _hsla(
        bgConfig.hue, bgConfig.surfSat, bgConfig.elevated, bgConfig.elevAlpha);
    final borderColor = _hsl(bgConfig.hue, isLight ? 15 : 6, bgConfig.border);

    final textPrimary = isLight ? _hsl(210, 20, 18) : _hsl(0, 0, 95);
    final textSecondary = isLight ? _hsl(210, 12, 42) : _hsl(240, 5, 65);
    final textMuted = isLight ? _hsl(210, 8, 58) : _hsl(240, 4, 45);

    final primaryTextColor =
        fontColor != null ? _parseColor(fontColor) : _parseColor(textPrimary);
    final secondaryTextColor = fontColor != null
        ? primaryTextColor.withValues(alpha: 0.7)
        : _parseColor(textSecondary);
    final mutedTextColor = fontColor != null
        ? primaryTextColor.withValues(alpha: 0.5)
        : _parseColor(textMuted);

    // Glass colors
    final glassGreen = _rgba(74, 222, 128, 0.15);
    final glassGreenBorder = _rgba(74, 222, 128, 0.4);
    final glassGreenText = isLight ? '#15803d' : '#4ade80';

    final glassBlue = _rgba(0, 150, 255, 0.15);
    final glassBlueBorder = _rgba(0, 150, 255, 0.4);
    final glassBlueText = isLight ? '#0055aa' : '#66bbff';

    final glassError = _rgba(255, 60, 60, 0.15);
    final glassErrorBorder = _rgba(255, 60, 60, 0.4);
    final glassErrorText = isLight ? '#aa0000' : '#ff6666';

    final glassBack =
        isLight ? _rgba(173, 216, 230, 0.3) : _hsla(210, 40, 40, 0.35);
    final glassBackBorder =
        isLight ? _rgba(173, 216, 230, 0.5) : _hsla(210, 50, 55, 0.5);

    final glassWindow =
        isLight ? _hsla(210, 25, 96, 0.55) : _hsla(220, 20, 15, 0.5);
    final glassWindowBorder =
        isLight ? _rgba(150, 190, 230, 0.25) : _hsla(210, 30, 40, 0.2);

    final colorScheme = ColorScheme(
      brightness: isLight ? Brightness.light : Brightness.dark,
      primary: _parseColor(customAccent ?? primary),
      onPrimary: Colors.white,
      secondary: _parseColor(secondary),
      onSecondary: Colors.white,
      error: _parseColor('hsl(0, 85, 58)'),
      onError: Colors.white,
      surface: _parseColor(surfaceColor),
      onSurface: primaryTextColor,
      outline: _parseColor(borderColor),
    );

    return ThemeData(
      useMaterial3: true,
      fontFamily: ff,
      colorScheme: colorScheme,
      // Transparent scaffold: screens paint over the AppShell background
      // image. Out-of-shell fallback color is set in main.dart's builder.
      scaffoldBackgroundColor: Colors.transparent,
      appBarTheme: AppBarTheme(
        centerTitle: true,
        elevation: 0,
        backgroundColor: _parseColor(surfaceColor),
        foregroundColor: primaryTextColor,
      ),
      cardTheme: CardThemeData(
        elevation: 0,
        color: _parseColor(elevatedColor),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(_borderRadius['md']!),
        ),
      ),
      elevatedButtonTheme: ElevatedButtonThemeData(
        style: ElevatedButton.styleFrom(
          backgroundColor: _parseColor(glassBack),
          foregroundColor: _parseColor(glassBlueText),
          elevation: 0,
          side: BorderSide(color: _parseColor(glassBackBorder)),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(_borderRadius['md']!),
          ),
        ),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          backgroundColor: _parseColor(glassBack),
          foregroundColor: _parseColor(glassBlueText),
          elevation: 0,
          side: BorderSide(color: _parseColor(glassBackBorder)),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(_borderRadius['md']!),
          ),
        ),
      ),
      textTheme: TextTheme(
        displayLarge: TextStyle(
          fontSize: 32 * fontScale,
          fontWeight: FontWeight.w700,
          color: primaryTextColor,
        ),
        displayMedium: TextStyle(
          fontSize: 28 * fontScale,
          fontWeight: FontWeight.w700,
          color: primaryTextColor,
        ),
        displaySmall: TextStyle(
          fontSize: 24 * fontScale,
          fontWeight: FontWeight.w600,
          color: primaryTextColor,
        ),
        headlineMedium: TextStyle(
          fontSize: 20 * fontScale,
          fontWeight: FontWeight.w600,
          color: primaryTextColor,
        ),
        headlineSmall: TextStyle(
          fontSize: 18 * fontScale,
          fontWeight: FontWeight.w600,
          color: primaryTextColor,
        ),
        titleLarge: TextStyle(
          fontSize: 16 * fontScale,
          fontWeight: FontWeight.w600,
          color: primaryTextColor,
        ),
        bodyLarge: TextStyle(
          fontSize: 15 * fontScale,
          fontWeight: FontWeight.w400,
          color: primaryTextColor,
        ),
        bodyMedium: TextStyle(
          fontSize: 13 * fontScale,
          fontWeight: FontWeight.w400,
          color: secondaryTextColor,
        ),
        bodySmall: TextStyle(
          fontSize: 12 * fontScale,
          fontWeight: FontWeight.w400,
          color: mutedTextColor,
        ),
        labelLarge: TextStyle(
          fontSize: 15 * fontScale,
          fontWeight: FontWeight.w600,
          color: primaryTextColor,
        ),
      ),
      extensions: [
        AppThemeExtension(
          spacing: _spacing,
          borderRadius: _borderRadius,
          glassGreen: _parseColor(glassGreen),
          glassGreenBorder: _parseColor(glassGreenBorder),
          glassGreenText: _parseColor(glassGreenText),
          glassBlue: _parseColor(glassBlue),
          glassBlueBorder: _parseColor(glassBlueBorder),
          glassBlueText: _parseColor(glassBlueText),
          glassError: _parseColor(glassError),
          glassErrorBorder: _parseColor(glassErrorBorder),
          glassErrorText: _parseColor(glassErrorText),
          glassBack: _parseColor(glassBack),
          glassBackBorder: _parseColor(glassBackBorder),
          glassWindow: _parseColor(glassWindow),
          glassWindowBorder: _parseColor(glassWindowBorder),
        ),
      ],
    );
  }

  static String _hsl(double h, double s, double l) {
    return 'hsl(${h.round()}, ${s.round()}%, ${l.round()}%)';
  }

  static String _hsla(double h, double s, double l, double a) {
    return 'hsla(${h.round()}, ${s.round()}%, ${l.round()}%, $a)';
  }

  static String _rgba(int r, int g, int b, double a) {
    return 'rgba($r, $g, $b, $a)';
  }

  static Color _parseColor(String colorString) {
    if (colorString.startsWith('#')) {
      return Color(
        0xFF000000 | int.parse(colorString.substring(1), radix: 16),
      );
    }
    if (colorString.startsWith('hsl(')) {
      final parts = colorString.substring(4, colorString.length - 1).split(',');
      if (parts.length >= 3) {
        final h = double.tryParse(parts[0].trim());
        final s = double.tryParse(parts[1].trim().replaceAll('%', ''));
        final l = double.tryParse(parts[2].trim().replaceAll('%', ''));
        if (h != null && s != null && l != null) {
          return HSVColor.fromAHSV(1.0, h, s / 100, l / 100).toColor();
        }
      }
      return Colors.grey;
    }
    if (colorString.startsWith('hsla(')) {
      final parts = colorString.substring(5, colorString.length - 1).split(',');
      if (parts.length >= 4) {
        final h = double.tryParse(parts[0].trim());
        final s = double.tryParse(parts[1].trim().replaceAll('%', ''));
        final l = double.tryParse(parts[2].trim().replaceAll('%', ''));
        final a = double.tryParse(parts[3].trim());
        if (h != null && s != null && l != null && a != null) {
          return HSVColor.fromAHSV(a, h, s / 100, l / 100).toColor();
        }
      }
      return Colors.grey;
    }
    if (colorString.startsWith('rgba(')) {
      final parts = colorString.substring(5, colorString.length - 1).split(',');
      if (parts.length >= 4) {
        final r = int.tryParse(parts[0].trim());
        final g = int.tryParse(parts[1].trim());
        final b = int.tryParse(parts[2].trim());
        final a = double.tryParse(parts[3].trim());
        if (r != null && g != null && b != null && a != null) {
          return Color.fromRGBO(r, g, b, a);
        }
      }
      return Colors.grey;
    }
    return Colors.grey;
  }

  static ThemeData get lightTheme => createTheme(bgLevel: 'light');
  static ThemeData get darkTheme => createTheme(bgLevel: 'dark');
  static ThemeData get darkerTheme => createTheme(bgLevel: 'darker');
  static ThemeData get deepestTheme => createTheme(bgLevel: 'deepest');
}

class _BgConfig {
  final double hue;
  final double bgSat;
  final double surfSat;
  final double bg;
  final double surface;
  final double elevated;
  final double border;
  final double surfAlpha;
  final double elevAlpha;

  const _BgConfig({
    required this.hue,
    required this.bgSat,
    required this.surfSat,
    required this.bg,
    required this.surface,
    required this.elevated,
    required this.border,
    required this.surfAlpha,
    required this.elevAlpha,
  });
}

@immutable
class AppThemeExtension extends ThemeExtension<AppThemeExtension> {
  final Map<String, double> spacing;
  final Map<String, double> borderRadius;
  final Color glassGreen;
  final Color glassGreenBorder;
  final Color glassGreenText;
  final Color glassBlue;
  final Color glassBlueBorder;
  final Color glassBlueText;
  final Color glassError;
  final Color glassErrorBorder;
  final Color glassErrorText;
  final Color glassBack;
  final Color glassBackBorder;
  final Color glassWindow;
  final Color glassWindowBorder;

  const AppThemeExtension({
    required this.spacing,
    required this.borderRadius,
    required this.glassGreen,
    required this.glassGreenBorder,
    required this.glassGreenText,
    required this.glassBlue,
    required this.glassBlueBorder,
    required this.glassBlueText,
    required this.glassError,
    required this.glassErrorBorder,
    required this.glassErrorText,
    required this.glassBack,
    required this.glassBackBorder,
    required this.glassWindow,
    required this.glassWindowBorder,
  });

  @override
  AppThemeExtension copyWith({
    Map<String, double>? spacing,
    Map<String, double>? borderRadius,
    Color? glassGreen,
    Color? glassGreenBorder,
    Color? glassGreenText,
    Color? glassBlue,
    Color? glassBlueBorder,
    Color? glassBlueText,
    Color? glassError,
    Color? glassErrorBorder,
    Color? glassErrorText,
    Color? glassBack,
    Color? glassBackBorder,
    Color? glassWindow,
    Color? glassWindowBorder,
  }) {
    return AppThemeExtension(
      spacing: spacing ?? this.spacing,
      borderRadius: borderRadius ?? this.borderRadius,
      glassGreen: glassGreen ?? this.glassGreen,
      glassGreenBorder: glassGreenBorder ?? this.glassGreenBorder,
      glassGreenText: glassGreenText ?? this.glassGreenText,
      glassBlue: glassBlue ?? this.glassBlue,
      glassBlueBorder: glassBlueBorder ?? this.glassBlueBorder,
      glassBlueText: glassBlueText ?? this.glassBlueText,
      glassError: glassError ?? this.glassError,
      glassErrorBorder: glassErrorBorder ?? this.glassErrorBorder,
      glassErrorText: glassErrorText ?? this.glassErrorText,
      glassBack: glassBack ?? this.glassBack,
      glassBackBorder: glassBackBorder ?? this.glassBackBorder,
      glassWindow: glassWindow ?? this.glassWindow,
      glassWindowBorder: glassWindowBorder ?? this.glassWindowBorder,
    );
  }

  @override
  AppThemeExtension lerp(ThemeExtension<AppThemeExtension>? other, double t) {
    if (other is! AppThemeExtension) return this;
    return AppThemeExtension(
      spacing: spacing,
      borderRadius: borderRadius,
      glassGreen: Color.lerp(glassGreen, other.glassGreen, t)!,
      glassGreenBorder:
          Color.lerp(glassGreenBorder, other.glassGreenBorder, t)!,
      glassGreenText: Color.lerp(glassGreenText, other.glassGreenText, t)!,
      glassBlue: Color.lerp(glassBlue, other.glassBlue, t)!,
      glassBlueBorder: Color.lerp(glassBlueBorder, other.glassBlueBorder, t)!,
      glassBlueText: Color.lerp(glassBlueText, other.glassBlueText, t)!,
      glassError: Color.lerp(glassError, other.glassError, t)!,
      glassErrorBorder:
          Color.lerp(glassErrorBorder, other.glassErrorBorder, t)!,
      glassErrorText: Color.lerp(glassErrorText, other.glassErrorText, t)!,
      glassBack: Color.lerp(glassBack, other.glassBack, t)!,
      glassBackBorder: Color.lerp(glassBackBorder, other.glassBackBorder, t)!,
      glassWindow: Color.lerp(glassWindow, other.glassWindow, t)!,
      glassWindowBorder:
          Color.lerp(glassWindowBorder, other.glassWindowBorder, t)!,
    );
  }
}
