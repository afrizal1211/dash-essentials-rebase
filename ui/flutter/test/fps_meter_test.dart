import 'package:dash_essential_ui/fps_meter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  group('DashFpsMeterModel', () {
    test('returns empty when fewer than two frames', () {
      expect(
        DashFpsMeterModel.compute(<FrameTiming>[]),
        equals(DashFpsSample.empty),
      );
      expect(
        DashFpsMeterModel.compute([const FrameTiming()]),
        equals(DashFpsSample.empty),
      );
    });

    test('estimates fps for uniform frames', () {
      final List<FrameTiming> frames = <FrameTiming>[
        const FrameTiming(
          buildDuration: Duration.zero,
          rasterDuration: Duration.zero,
          presentDuration: Duration.zero,
          totalSpan: Duration.zero,
          intervalStartInMicroseconds: 0,
          intervalEndInMicroseconds: 8333,
          buildStartTimestampMicroseconds: 0,
          buildFinishTimestampMicroseconds: 100,
          rasterStartTimestampMicroseconds: 100,
          rasterFinishTimestampMicroseconds: 200,
          presentStartTimeMicroseconds: 200,
          presentFinishTimeMicroseconds: 8333,
        ),
        const FrameTiming(
          buildDuration: Duration.zero,
          rasterDuration: Duration.zero,
          presentDuration: Duration.zero,
          totalSpan: Duration.zero,
          intervalStartInMicroseconds: 8333,
          intervalEndInMicroseconds: 16666,
          buildStartTimestampMicroseconds: 8333,
          buildFinishTimestampMicroseconds: 8433,
          rasterStartTimestampMicroseconds: 8433,
          rasterFinishTimestampMicroseconds: 8533,
          presentStartTimeMicroseconds: 8533,
          presentFinishTimeMicroseconds: 16666,
        ),
      ];
      final DashFpsSample sample = DashFpsMeterModel.compute(frames, targetHz: 120);
      expect(sample.valid, isTrue);
      // Two frames spanning ~8ms => ~120fps expected.
      expect(sample.fps, isInInclusiveInterval(90.0, 150.0));
      expect(sample.min, greaterThan(0));
      expect(sample.low1, greaterThan(0));
    });

    test('detects dropped frames above budget', () {
      final List<FrameTiming> frames = <FrameTiming>[];
      for (int i = 0; i < 10; i++) {
        frames.add(const FrameTiming(
          buildDuration: Duration.zero,
          rasterDuration: Duration.zero,
          presentDuration: Duration.zero,
          totalSpan: Duration.zero,
          intervalStartInMicroseconds: i * 8333,
          intervalEndInMicroseconds: (i + 1) * 8333,
          buildStartTimestampMicroseconds: i * 8333,
          buildFinishTimestampMicroseconds: i * 8333 + 100,
          rasterStartTimestampMicroseconds: i * 8333 + 100,
          rasterFinishTimestampMicroseconds: i * 8333 + 200,
          presentStartTimeMicroseconds: i * 8333 + 200,
          presentFinishTimeMicroseconds: (i + 1) * 8333,
        ));
      }
      // Make one frame take 20ms (double the 120 Hz budget of ~8.3ms).
      frames[5] = FrameTiming(
        buildDuration: const Duration(milliseconds: 10),
        rasterDuration: const Duration(milliseconds: 5),
        presentDuration: Duration.zero,
        totalSpan: const Duration(milliseconds: 15),
        intervalStartInMicroseconds: 5 * 8333,
        intervalEndInMicroseconds: 6 * 8333 + 6667,
        buildStartTimestampMicroseconds: 5 * 8333,
        buildFinishTimestampMicroseconds: 5 * 8333 + 10000,
        rasterStartTimestampMicroseconds: 5 * 8333 + 10000,
        rasterFinishTimestampMicroseconds: 5 * 8333 + 15000,
        presentStartTimeMicroseconds: 5 * 8333 + 15000,
        presentFinishTimeMicroseconds: 6 * 8333 + 6667,
      );
      final DashFpsSample sample = DashFpsMeterModel.compute(frames, targetHz: 120);
      expect(sample.dropped, greaterThanOrEqualTo(1));
    });
  });

  group('DashFpsMeter widget', () {
    testWidgets('shows meter when enabled', (WidgetTester tester) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: DashFpsMeter(accent: const Color(0xFF59BCFF)),
          ),
        ),
      );
      // Meter mounts; initial sample is empty so it shows "FPS —".
      expect(find.textContaining('FPS'), findsOneWidget);
      // Chip surface is rendered (RepaintBoundary + DecoratedBox).
      expect(tester.widget<DecoratedBox>(find.byType(DecoratedBox)), isNotNull);
    });

    testWidgets('chip is IgnorePointer and ExcludeSemantics',
        (WidgetTester tester) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: DashFpsMeter(accent: const Color(0xFF59BCFF)),
          ),
        ),
      );
      expect(find.byType(IgnorePointer), findsOneWidget);
      expect(find.byType(ExcludeSemantics), findsOneWidget);
    });
  });
}
