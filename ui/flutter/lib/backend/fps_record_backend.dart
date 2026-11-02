import 'per_app_backend.dart';

/// One finished recording, as the daemon stored it.
///
/// Rates arrive in tenths of a frame per second over the socket; they are
/// converted here so the UI never has to remember the scaling.
class FpsSession {
  const FpsSession({
    required this.id,
    required this.package,
    required this.startedAt,
    required this.duration,
    required this.samples,
    required this.average,
    required this.minimum,
    required this.maximum,
    required this.low1,
    required this.slowPercent,
    required this.series,
    required this.seriesStep,
  });

  final int id;
  final String package;
  final DateTime startedAt;
  final Duration duration;
  final int samples;
  final double average;
  final double minimum;
  final double maximum;
  final double low1;

  /// Share of samples that ran more than ten percent under the median.
  final double slowPercent;

  /// The recorded frame rate, oldest first, downsampled for drawing.
  final List<double> series;

  /// Time between two neighbouring [series] points.
  final Duration seriesStep;

  factory FpsSession.fromJson(Map<String, dynamic> json) {
    final List<dynamic> rawSeries = json['series'] is List
        ? json['series'] as List<dynamic>
        : const <dynamic>[];
    final int startedAt = (json['startedAt'] as num?)?.toInt() ?? 0;
    return FpsSession(
      id: (json['id'] as num?)?.toInt() ?? startedAt,
      package: _package(json['package']),
      startedAt: DateTime.fromMillisecondsSinceEpoch(startedAt),
      duration: Duration(
        milliseconds: (json['durationMs'] as num?)?.toInt() ?? 0,
      ),
      samples: (json['samples'] as num?)?.toInt() ?? 0,
      average: _tenths(json['avgX10']),
      minimum: _tenths(json['minX10']),
      maximum: _tenths(json['maxX10']),
      low1: _tenths(json['low1X10']),
      slowPercent: ((json['slowPctX10'] as num?)?.toDouble() ?? 0) / 10,
      series: <double>[
        for (final dynamic value in rawSeries)
          ((value as num?)?.toDouble() ?? 0) / 10,
      ],
      seriesStep: Duration(
        milliseconds: (json['seriesStepMs'] as num?)?.toInt() ?? 0,
      ),
    );
  }

  static double _tenths(Object? raw) => ((raw as num?)?.toDouble() ?? 0) / 10;

  /// A daemon that only knows the string form must not produce a game called
  /// "null" in the history list.
  static String _package(Object? raw) {
    final String value = raw?.toString() ?? '';
    return value == 'null' ? '' : value;
  }
}

/// Live state of the recorder, polled by the settings screen.
class FpsRecordStatus {
  const FpsRecordStatus({
    required this.active,
    required this.elapsed,
    required this.samples,
    required this.package,
    required this.average,
    required this.lastFps,
  });

  const FpsRecordStatus.inactive()
    : active = false,
      elapsed = Duration.zero,
      samples = 0,
      package = null,
      average = 0,
      lastFps = 0;

  final bool active;
  final Duration elapsed;
  final int samples;
  final String? package;
  final double average;
  final double lastFps;

  factory FpsRecordStatus.fromJson(Map<String, dynamic> json) {
    final Object? package = json['package'];
    return FpsRecordStatus(
      active: json['active'] == true,
      elapsed: Duration(
        milliseconds: (json['elapsedMs'] as num?)?.toInt() ?? 0,
      ),
      samples: (json['samples'] as num?)?.toInt() ?? 0,
      package: package is String && package.isNotEmpty && package != 'null'
          ? package
          : null,
      average: ((json['avgX10'] as num?)?.toDouble() ?? 0) / 10,
      lastFps: ((json['lastFpsX10'] as num?)?.toDouble() ?? 0) / 10,
    );
  }
}

/// The daemon owns the recording and the history file; this is only the
/// typed face of its four commands.
class FpsRecordBackend {
  FpsRecordBackend._();

  static final FpsRecordBackend instance = FpsRecordBackend._();

  Future<FpsRecordStatus> status() async {
    final dynamic data = await PerAppBackend.instance.command(
      'GET app.fps.record',
    );
    return FpsRecordStatus.fromJson(_asMap(data));
  }

  Future<FpsRecordStatus> setRecording(bool active) async {
    final dynamic data = await PerAppBackend.instance.command(
      'SET app.fps.record ${active ? 1 : 0}',
    );
    return FpsRecordStatus.fromJson(_asMap(data));
  }

  /// Newest session first.
  Future<List<FpsSession>> sessions() async {
    final dynamic data = await PerAppBackend.instance.command(
      'GET app.fps.sessions',
    );
    if (data is! List) {
      return const <FpsSession>[];
    }
    return <FpsSession>[
      for (final dynamic entry in data)
        if (entry is Map<String, dynamic>) FpsSession.fromJson(entry),
    ];
  }

  Future<int> clear() async {
    final dynamic data = await PerAppBackend.instance.command(
      'ACTION app.fps.clear',
    );
    if (data is Map<String, dynamic>) {
      return (data['cleared'] as num?)?.toInt() ?? 0;
    }
    return 0;
  }

  static Map<String, dynamic> _asMap(dynamic data) {
    return data is Map<String, dynamic> ? data : <String, dynamic>{};
  }
}
