import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:soshal_flutter/widgets/audio_waveform_bar.dart';

void main() {
  group('waveformFraction', () {
    test('clamps to 0..1', () {
      expect(waveformFraction(-50, 300), 0);
      expect(waveformFraction(0, 300), 0);
      expect(waveformFraction(150, 300), 0.5);
      expect(waveformFraction(300, 300), 1);
      expect(waveformFraction(999, 300), 1);
    });

    test('zero width is a safe no-op', () {
      expect(waveformFraction(10, 0), 0);
    });
  });

  group('AudioWaveformSeekBar', () {
    testWidgets('tap maps to absolute position via fraction', (tester) async {
      Duration? sought;
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          body: SizedBox(
            width: 300,
            child: AudioWaveformSeekBar(
              peaks: const [0.2, 0.4, 0.6, 0.8, 1.0],
              position: const Duration(seconds: 30),
              duration: const Duration(seconds: 120),
              onSeek: (p) => sought = p,
            ),
          ),
        ),
      ));

      // Tap dead center → 50% of 120 s = 60 s.
      await tester.tapAt(const Offset(150, 18));
      expect(sought, const Duration(seconds: 60));

      // Quarter → 30 s.
      await tester.tapAt(const Offset(75, 18));
      expect(sought, const Duration(seconds: 30));
    });

    testWidgets('renders with peaks without throwing', (tester) async {
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          body: SizedBox(
            width: 300,
            child: AudioWaveformSeekBar(
              peaks: List<double>.generate(64, (i) => i / 64),
              position: const Duration(seconds: 5),
              duration: const Duration(seconds: 100),
              onSeek: (_) {},
            ),
          ),
        ),
      ));
      expect(tester.takeException(), isNull);
    });

    testWidgets('tap is ignored before duration is known', (tester) async {
      var calls = 0;
      await tester.pumpWidget(MaterialApp(
        home: Scaffold(
          body: SizedBox(
            width: 300,
            child: AudioWaveformSeekBar(
              peaks: const [0.5],
              position: Duration.zero,
              duration: null,
              onSeek: (_) => calls++,
            ),
          ),
        ),
      ));
      await tester.tapAt(const Offset(150, 18));
      expect(calls, 0);
    });
  });
}