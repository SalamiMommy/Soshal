import 'package:flutter/material.dart';

/// Fraction of the bar width a horizontal tap lands at, clamped to 0..1.
/// Pure helper so tests can pin seek math without a widget tree.
double waveformFraction(double dx, double width) {
  if (width <= 0) return 0;
  return (dx / width).clamp(0.0, 1.0);
}

/// SoundCloud-style waveform seek bar: bars from Rust-extracted RMS peaks
/// (0..1, ~64 bins via `storage_get_audio_peaks`), progress fill up to the
/// current playback position, and tap/drag-to-seek. Drawn by a
/// [CustomPainter] — Flutter renders, Rust crunches.
class AudioWaveformSeekBar extends StatelessWidget {
  final List<double> peaks;
  final Duration position;
  final Duration? duration;
  final ValueChanged<Duration> onSeek;
  final double height;
  final double barWidthFactor;
  final Color? activeColor;
  final Color? inactiveColor;

  const AudioWaveformSeekBar({
    super.key,
    required this.peaks,
    required this.position,
    required this.duration,
    required this.onSeek,
    this.height = 36,
    this.barWidthFactor = 0.6,
    this.activeColor,
    this.inactiveColor,
  });

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final active = activeColor ?? scheme.primary;
    final inactive = inactiveColor ?? scheme.outlineVariant;
    final total = duration ?? Duration.zero;
    final progress = total.inMicroseconds <= 0
        ? 0.0
        : (position.inMicroseconds / total.inMicroseconds).clamp(0.0, 1.0);
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTapDown: (details) {
        final size = context.size;
        if (size == null || total.inMicroseconds <= 0) return;
        final fraction = waveformFraction(details.localPosition.dx, size.width);
        onSeek(Duration(
          microseconds: (total.inMicroseconds * fraction).round(),
        ));
      },
      onHorizontalDragUpdate: (details) {
        final size = context.size;
        if (size == null || total.inMicroseconds <= 0) return;
        final fraction = waveformFraction(details.localPosition.dx, size.width);
        onSeek(Duration(
          microseconds: (total.inMicroseconds * fraction).round(),
        ));
      },
      child: CustomPaint(
        size: Size(double.infinity, height),
        painter: _WaveformPainter(
          peaks: peaks,
          progress: progress,
          activeColor: active,
          inactiveColor: inactive,
          barWidthFactor: barWidthFactor,
        ),
      ),
    );
  }
}

class _WaveformPainter extends CustomPainter {
  final List<double> peaks;
  final double progress;
  final Color activeColor;
  final Color inactiveColor;
  final double barWidthFactor;

  _WaveformPainter({
    required this.peaks,
    required this.progress,
    required this.activeColor,
    required this.inactiveColor,
    required this.barWidthFactor,
  });

  @override
  void paint(Canvas canvas, Size size) {
    if (peaks.isEmpty || size.width <= 0 || size.height <= 0) return;
    final count = peaks.length;
    final slot = size.width / count;
    final barWidth = slot * barWidthFactor;
    final maxBarHeight = size.height - 2;
    final midY = size.height / 2;
    for (var i = 0; i < count; i++) {
      final peak = peaks[i].clamp(0.0, 1.0).toDouble();
      final barHeight = (maxBarHeight * peak).clamp(1.0, maxBarHeight);
      final x = slot * i + (slot - barWidth) / 2;
      final fill = i / count <= progress;
      final paint = Paint()
        ..color = fill ? activeColor : inactiveColor
        ..strokeWidth = barWidth
        ..strokeCap = StrokeCap.round;
      canvas.drawLine(
        Offset(x + barWidth / 2, midY - barHeight / 2),
        Offset(x + barWidth / 2, midY + barHeight / 2),
        paint,
      );
    }
  }

  @override
  bool shouldRepaint(_WaveformPainter old) =>
      old.peaks != peaks ||
      old.progress != progress ||
      old.activeColor != activeColor ||
      old.inactiveColor != inactiveColor;
}
