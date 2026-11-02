import 'package:dash_essential_ui/backend/fps_record_backend.dart';
import 'package:dash_essential_ui/main.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('appearance remembers the recording switch', () {
    const DashAppearanceConfig defaults = DashAppearanceConfig.defaults();
    expect(defaults.fpsRecord, isFalse);

    final DashAppearanceConfig restored = DashAppearanceConfig.fromJson(
      defaults.copyWith(fpsRecord: true).toJson(),
    );
    expect(restored.fpsRecord, isTrue);

    // A file written before the switch existed must read as "off".
    final Map<String, Object> legacy = defaults.toJson()..remove('fpsRecord');
    expect(DashAppearanceConfig.fromJson(legacy).fpsRecord, isFalse);
  });

  test('a stored session converts tenths back into frame rates', () {
    final FpsSession session = FpsSession.fromJson(<String, dynamic>{
      'id': 1700000000000,
      'package': 'com.game',
      'startedAt': 1700000000000,
      'durationMs': 65000,
      'samples': 130,
      'avgX10': 597,
      'minX10': 420,
      'maxX10': 610,
      'low1X10': 455,
      'slowPctX10': 125,
      'series': <int>[600, 590, 420],
      'seriesStepMs': 500,
    });

    expect(session.package, 'com.game');
    expect(session.average, closeTo(59.7, 1e-9));
    expect(session.minimum, closeTo(42.0, 1e-9));
    expect(session.maximum, closeTo(61.0, 1e-9));
    expect(session.low1, closeTo(45.5, 1e-9));
    expect(session.slowPercent, closeTo(12.5, 1e-9));
    expect(session.duration, const Duration(seconds: 65));
    expect(session.series, <double>[60.0, 59.0, 42.0]);
    expect(session.seriesStep, const Duration(milliseconds: 500));
  });

  test('a missing package never becomes a game called null', () {
    final FpsSession session = FpsSession.fromJson(<String, dynamic>{
      'startedAt': 1700000000000,
      'package': null,
    });
    expect(session.package, isEmpty);
    expect(session.series, isEmpty);
    expect(session.average, 0);

    final FpsSession stringy = FpsSession.fromJson(<String, dynamic>{
      'startedAt': 1700000000000,
      'package': 'null',
    });
    expect(stringy.package, isEmpty);
  });

  test('live status tolerates a daemon without a foreground package', () {
    final FpsRecordStatus status = FpsRecordStatus.fromJson(<String, dynamic>{
      'active': true,
      'elapsedMs': 1500,
      'samples': 3,
      'package': null,
      'avgX10': 600,
      'lastFpsX10': 590,
    });

    expect(status.active, isTrue);
    expect(status.elapsed, const Duration(milliseconds: 1500));
    expect(status.samples, 3);
    expect(status.package, isNull);
    expect(status.average, closeTo(60.0, 1e-9));
    expect(status.lastFps, closeTo(59.0, 1e-9));

    expect(const FpsRecordStatus.inactive().active, isFalse);
  });
}
