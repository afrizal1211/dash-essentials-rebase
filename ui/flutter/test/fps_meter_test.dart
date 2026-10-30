import 'package:dash_essential_ui/fps_meter.dart';
import 'package:flutter/material.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  /// Build a FrameTiming using the current (int-based) constructor.
  FrameTiming frame(int i, {int intervalUs = 8333, int totalSpanUs = 0}) {
    final int start = i * intervalUs;
    final int end = (i + 1) * intervalUs;
    return FrameTiming(
      vsyncStart: start,
      buildStart: start,
      buildFinish: start + 100,
      rasterStart: start + 100,
      rasterFinish: start + 100 + totalSpanUs,
      rasterFinishWallTime: end,
      frameNumber: i,
    );
  }

  group('DashFpsMeterModel', () {
    test('returns empty when fewer than two frames', () {
      expect(
        DashFpsMeterModel.compute(<FrameTiming>[]),
        equals(DashFpsSample.empty),
      );
      expect(
        DashFpsMeterModel.compute(<FrameTiming>[frame(0)]),
        equals(DashFpsSample.empty),
      );
    });

    test('estimates fps for uniform frames', () {
      final List<FrameTiming> frames = <FrameTiming>[
        frame(0),
        frame(1),
      ];
      final DashFpsSample sample =
          DashFpsMeterModel.compute(frames, targetHz: 120);
      expect(sample.valid, isTrue);
      // Two frames spanning ~8ms => ~120fps expected.
      expect(sample.fps, greaterThan(60.0));
      expect(sample.fps, lessThan(200.0));
      expect(sample.min, greaterThan(0));
      expect(sample.low1, greaterThan(0));
    });

    test('detects dropped frames above budget', () {
      final List<FrameTiming> frames = <FrameTiming>[];
      for (int i = 0; i < 10; i++) {
        frames.add(frame(i));
      }
      // Make one frame take 20ms (double the 120 Hz budget of ~8.3ms).
      frames[5] = frame(5, totalSpanUs: 20000);
      final DashFpsSample sample =
          DashFpsMeterModel.compute(frames, targetHz: 120);
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
      expect(
        find.descendant(
          of: find.byType(DashFpsMeter),
          matching: find.byType(IgnorePointer),
        ),
        findsOneWidget,
      );
      expect(
        find.descendant(
          of: find.byType(DashFpsMeter),
          matching: find.byType(ExcludeSemantics),
        ),
        findsOneWidget,
      );
    });
  });
}