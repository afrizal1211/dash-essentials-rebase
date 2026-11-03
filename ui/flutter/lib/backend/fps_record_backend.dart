import 'dart:convert';
import 'dart:ffi' as ffi;

import 'per_app_backend.dart';

typedef _ExportSaveNative =
    ffi.Int32 Function(ffi.Pointer<ffi.Uint8>, ffi.Int32, ffi.Pointer<ffi.Uint8>, ffi.Int32);
typedef _ExportSaveDart = int Function(ffi.Pointer<ffi.Uint8>, int, ffi.Pointer<ffi.Uint8>, int);

typedef _AllocateNative = ffi.Pointer<ffi.Void> Function(ffi.IntPtr);
typedef _AllocateDart = ffi.Pointer<ffi.Void> Function(int);
typedef _ReleaseNative = ffi.Void Function(ffi.Pointer<ffi.Void>);
typedef _ReleaseDart = void Function(ffi.Pointer<ffi.Void>);

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
    required this.low5,
    required this.variance,
    required this.smoothPercent,
    required this.jitterPercent,
    required this.p99FrameTime,
    required this.jank,
    required this.description,
    required this.auto,
    required this.resolution,
    required this.cpuAverage,
    required this.cpuMaximum,
    required this.gpuLoadAverage,
    required this.gpuLoadMaximum,
    required this.gpuMhzAverage,
    required this.gpuMhzMaximum,
    required this.batteryTempAverage,
    required this.batteryTempMaximum,
    required this.powerAverage,
    required this.powerMaximum,
    required this.ramAverage,
    required this.ramMaximum,
    required this.cpuTempMaximum,
    required this.gpuTempMaximum,
    required this.packageRamAverage,
    required this.packageRamMaximum,
    required this.metrics,
    required this.available,
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

  /// Prism's 5% low: the fifth percentile of frame rate. Zero until the
  /// session has enough samples for the number to mean anything.
  final double low5;

  /// Variance of the frame rate in fps².
  final double variance;

  /// Share of samples within ten percent of the average.
  final double smoothPercent;

  /// Frame-time standard deviation over its mean, as a percentage.
  final double jitterPercent;

  /// 99th percentile frame time.
  final Duration p99FrameTime;

  /// Samples slower than one and a half mean frame times.
  final int jank;

  /// User's note. Empty when the session was never described.
  final String description;

  /// True when the foreground-following recorder started this session.
  final bool auto;

  final String resolution;

  final double cpuAverage;
  final double cpuMaximum;
  final double gpuLoadAverage;
  final double gpuLoadMaximum;
  final double gpuMhzAverage;
  final double gpuMhzMaximum;

  /// Battery temperature in degrees Celsius.
  final double batteryTempAverage;
  final double batteryTempMaximum;

  /// Power draw in watts.
  final double powerAverage;
  final double powerMaximum;

  /// System RAM in use, in megabytes.
  final double ramAverage;
  final double ramMaximum;

  /// Peak CPU/GPU thermal-zone temperature in degrees Celsius.
  final double cpuTempMaximum;
  final double gpuTempMaximum;

  /// RAM used by the recorded package, in megabytes.
  final double packageRamAverage;
  final double packageRamMaximum;

  /// Extra graph series keyed by metric name. Frame time is in milliseconds;
  /// the rest carry the units their name implies.
  final Map<String, List<double>> metrics;

  /// Metric keys the daemon could actually measure for this session, in the
  /// order the detail screen offers them.
  final List<String> available;

  factory FpsSession.fromJson(Map<String, dynamic> json) {
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
      slowPercent: _percent(json['slowPctX10']),
      series: _series(json['series'], 10),
      seriesStep: Duration(
        milliseconds: (json['seriesStepMs'] as num?)?.toInt() ?? 0,
      ),
      low5: _tenths(json['low5X10']),
      variance: ((json['varianceX100'] as num?)?.toDouble() ?? 0) / 100,
      smoothPercent: _percent(json['smoothPctX10']),
      jitterPercent: _percent(json['jitterPctX10']),
      p99FrameTime: Duration(
        microseconds: (json['p99FrameUs'] as num?)?.toInt() ?? 0,
      ),
      jank: (json['jank'] as num?)?.toInt() ?? 0,
      description: json['description'] is String
          ? json['description'] as String
          : '',
      auto: json['auto'] == true,
      resolution: json['resolution'] is String
          ? json['resolution'] as String
          : '',
      cpuAverage: _int(json['cpuAvg']),
      cpuMaximum: _int(json['cpuMax']),
      gpuLoadAverage: _int(json['gpuLoadAvg']),
      gpuLoadMaximum: _int(json['gpuLoadMax']),
      gpuMhzAverage: _int(json['gpuMhzAvg']),
      gpuMhzMaximum: _int(json['gpuMhzMax']),
      batteryTempAverage: _deci(json['tempAvgDc']),
      batteryTempMaximum: _deci(json['tempMaxDc']),
      powerAverage: _milliwatts(json['powerAvgMw']),
      powerMaximum: _milliwatts(json['powerMaxMw']),
      ramAverage: _int(json['ramAvgMb']),
      ramMaximum: _int(json['ramMaxMb']),
      cpuTempMaximum: _deci(json['cpuTempMaxDc']),
      gpuTempMaximum: _deci(json['gpuTempMaxDc']),
      packageRamAverage: _int(json['packageRamAvgMb']),
      packageRamMaximum: _int(json['packageRamMaxMb']),
      metrics: <String, List<double>>{
        if (json['frameSeries'] is List)
          'frameTime': _series(json['frameSeries'], 1000),
        if (json['cpuSeries'] is List) 'cpuLoad': _series(json['cpuSeries'], 1),
        if (json['gpuSeries'] is List) 'gpuLoad': _series(json['gpuSeries'], 1),
        if (json['gpuMhzSeries'] is List)
          'gpuMhz': _series(json['gpuMhzSeries'], 1),
        if (json['tempSeries'] is List)
          'batteryTemp': _series(json['tempSeries'], 10),
        if (json['cpuTempSeries'] is List)
          'cpuTemp': _series(json['cpuTempSeries'], 10),
        if (json['gpuTempSeries'] is List)
          'gpuTemp': _series(json['gpuTempSeries'], 10),
        if (json['powerSeries'] is List)
          'power': _series(json['powerSeries'], 1000),
        if (json['ramSeries'] is List) 'ram': _series(json['ramSeries'], 1),
        if (json['packageRamSeries'] is List)
          'packageRam': _series(json['packageRamSeries'], 1),
      },
      available: <String>[
        for (final dynamic key in (json['available'] as List? ?? const []))
          if (key is String) key,
      ],
    );
  }

  static double _tenths(Object? raw) => raw is num ? raw.toDouble() / 10 : 0;

  /// Percentages are stored as tenths of a percent.
  static double _percent(Object? raw) => raw is num ? raw.toDouble() / 10 : 0;

  static double _int(Object? raw) => raw is num ? raw.toDouble() : 0;

  static double _deci(Object? raw) =>
      raw is num ? raw.toDouble() / 10 : 0;

  static double _milliwatts(Object? raw) =>
      raw is num ? raw.toDouble() / 1000 : 0;

  static List<double> _series(Object? raw, num scale) {
    if (raw is! List) {
      return const <double>[];
    }
    return <double>[
      for (final dynamic value in raw)
        value is num ? value.toDouble() / scale : 0,
    ];
  }

  /// A daemon that only knows the string form must not produce a game called
  /// "null" in the history list.
  static String _package(Object? raw) {
    final String value = raw?.toString() ?? '';
    return value == 'null' ? '' : value;
  }

  /// The graph series for [key], or an empty list when it was not recorded.
  List<double> metricSeries(String key) {
    if (key == 'fps') {
      return series;
    }
    return metrics[key] ?? const <double>[];
  }
}

/// Live state of the recorder, polled by the settings screen.
class FpsRecordStatus {
  const FpsRecordStatus({
    required this.active,
    required this.auto,
    required this.elapsed,
    required this.samples,
    required this.package,
    required this.average,
    required this.lastFps,
  });

  const FpsRecordStatus.inactive()
    : active = false,
      auto = false,
      elapsed = Duration.zero,
      samples = 0,
      package = null,
      average = 0,
      lastFps = 0;

  final bool active;

  /// Automatic mode is armed and waiting for a game.
  final bool auto;
  final Duration elapsed;
  final int samples;
  final String? package;
  final double average;
  final double lastFps;

  factory FpsRecordStatus.fromJson(Map<String, dynamic> json) {
    final Object? package = json['package'];
    return FpsRecordStatus(
      active: json['active'] == true,
      auto: json['auto'] == true,
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
/// typed face of its commands.
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

  /// Arms or disarms automatic recording. Manual recording is untouched until
  /// the next explicit switch.
  Future<FpsRecordStatus> setAuto(bool active) async {
    final dynamic data = await PerAppBackend.instance.command(
      'SET app.fps.auto ${active ? 1 : 0}',
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

  Future<bool> setNote(int id, String note) async {
    final dynamic data = await PerAppBackend.instance.command(
      'SET app.fps.note $id ${jsonEncode(note)}',
    );
    return _asMap(data)['updated'] == true;
  }

  Future<bool> delete(int id) async {
    final dynamic data = await PerAppBackend.instance.command(
      'ACTION app.fps.delete $id',
    );
    return _asMap(data)['deleted'] == true;
  }

  /// Writes one session's CSV and graph PNG to public storage via the host.
  /// Returns true when the host reported success.
  Future<bool> exportFile(int id, String csv, String pngBase64) async {
    final List<int> csvBytes = utf8.encode(csv);
    final List<int> pngBytes = pngBase64.isEmpty
        ? <int>[]
        : base64Decode(pngBase64);
    try {
      final ffi.DynamicLibrary host = ffi.DynamicLibrary.open(
        'libdash_essential_host.so',
      );
      final ffi.DynamicLibrary libc = ffi.DynamicLibrary.open('libc.so');
      final _AllocateDart allocate =
          libc.lookupFunction<_AllocateNative, _AllocateDart>('malloc');
      final _ReleaseDart release =
          libc.lookupFunction<_ReleaseNative, _ReleaseDart>('free');
      final _ExportSaveDart exportSave =
          host.lookupFunction<_ExportSaveNative, _ExportSaveDart>(
              'dash_host_export_save');
      final ffi.Pointer<ffi.Void> csvPtr = allocate(csvBytes.length);
      final int pngLen = pngBytes.length;
      final ffi.Pointer<ffi.Void> pngPtr = allocate(pngLen < 1 ? 1 : pngLen);
      if (csvPtr == ffi.nullptr || pngPtr == ffi.nullptr) {
        if (csvPtr != ffi.nullptr) release(csvPtr);
        if (pngPtr != ffi.nullptr) release(pngPtr);
        return false;
      }
      try {
        csvPtr.cast<ffi.Uint8>().asTypedList(csvBytes.length).setAll(0, csvBytes);
        pngPtr.cast<ffi.Uint8>().asTypedList(pngBytes.length).setAll(0, pngBytes);
        final int result = exportSave(
          csvPtr.cast<ffi.Uint8>(),
          csvBytes.length,
          pngPtr.cast<ffi.Uint8>(),
          pngBytes.length,
        );
        return result == 1;
      } finally {
        release(csvPtr);
        release(pngPtr);
      }
    } catch (_) {
      return false;
    }
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
