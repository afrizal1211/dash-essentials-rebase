part of 'main.dart';

// Recorded FPS sessions: one card per game, a detail view with the frame-rate
// graph, and CSV export. Recording itself lives in the daemon, so closing the
// app never loses an in-progress session.
//
// Parity with RedFox Prism: each session carries Prism-style stats
// (5% low, variance, relative smoothness, jitter %, 99% frame time, jank),
// hardware telemetry series (CPU/GPU load, GPU MHz, battery temp, power, RAM,
// package RAM), a user note, delete, and file export of the graph plus CSV.

const List<String> _fpsMonthNames = <String>[
  'Jan',
  'Feb',
  'Mar',
  'Apr',
  'May',
  'Jun',
  'Jul',
  'Aug',
  'Sep',
  'Oct',
  'Nov',
  'Dec',
];

String _fpsSessionDate(DateTime time) {
  final DateTime local = time.toLocal();
  final String hour = local.hour.toString().padLeft(2, '0');
  final String minute = local.minute.toString().padLeft(2, '0');
  return '${_fpsMonthNames[local.month - 1]} ${local.day}, $hour:$minute';
}

String _fpsDurationText(Duration duration) {
  final int seconds = duration.inSeconds;
  if (seconds >= 3600) {
    final int hours = seconds ~/ 3600;
    final int minutes = (seconds % 3600) ~/ 60;
    return '${hours}h ${minutes.toString().padLeft(2, '0')}m';
  }
  if (seconds >= 60) {
    final int minutes = seconds ~/ 60;
    return '${minutes}m ${(seconds % 60).toString().padLeft(2, '0')}s';
  }
  return '${seconds}s';
}

String _fpsCsvNumber(double value) => value.toStringAsFixed(1);

/// One graphable metric, matched to the daemon's series keys.
class _FpsMetric {
  const _FpsMetric({
    required this.key,
    required this.label,
    required this.unit,
    required this.decimals,
  });

  /// Daemon series key; `'fps'` means the main frame-rate series.
  final String key;
  final String label;
  final String unit;
  final int decimals;

  static const List<_FpsMetric> all = <_FpsMetric>[
    _FpsMetric(key: 'fps', label: 'FPS', unit: 'fps', decimals: 1),
    _FpsMetric(key: 'frameTime', label: 'Frame time', unit: 'ms', decimals: 1),
    _FpsMetric(key: 'cpuLoad', label: 'CPU load', unit: '%', decimals: 0),
    _FpsMetric(key: 'gpuLoad', label: 'GPU load', unit: '%', decimals: 0),
    _FpsMetric(key: 'gpuMhz', label: 'GPU MHz', unit: 'MHz', decimals: 0),
    _FpsMetric(key: 'batteryTemp', label: 'Battery temp', unit: '\u00b0C', decimals: 1),
    _FpsMetric(key: 'power', label: 'Power', unit: 'W', decimals: 1),
    _FpsMetric(key: 'ram', label: 'RAM', unit: 'MB', decimals: 0),
    _FpsMetric(key: 'cpuTemp', label: 'CPU temp', unit: '\u00b0C', decimals: 1),
    _FpsMetric(key: 'gpuTemp', label: 'GPU temp', unit: '\u00b0C', decimals: 1),
    _FpsMetric(key: 'packageRam', label: 'Package RAM', unit: 'MB', decimals: 0),
  ];
}

/// Every recorded session, grouped by the game it belongs to.
class FpsHistoryScreen extends StatefulWidget {
  const FpsHistoryScreen({this.onStopped, super.key});

  /// Called after "Stop and save" so the settings switch follows the daemon
  /// instead of claiming a recording that is no longer running.
  final VoidCallback? onStopped;

  @override
  State<FpsHistoryScreen> createState() => _FpsHistoryScreenState();
}

class _FpsHistoryScreenState extends State<FpsHistoryScreen> {
  bool _loading = true;
  String? _error;
  List<FpsSession> _sessions = const <FpsSession>[];
  Map<String, String> _labels = const <String, String>{};
  FpsRecordStatus? _status;

  @override
  void initState() {
    super.initState();
    unawaited(_load());
  }

  Future<void> _load() async {
    // initState already starts the first load with a spinner; only later
    // reloads need to raise it again.
    if (!_loading && mounted) {
      setState(() {
        _loading = true;
        _error = null;
      });
    }
    try {
      final List<FpsSession> sessions = await FpsRecordBackend.instance
          .sessions();
      final FpsRecordStatus? status = await _loadStatus();
      if (!mounted) return;
      // Draw the history before resolving friendly names: the app list shells
      // out and must never hold up the page.
      setState(() {
        _sessions = sessions;
        _status = status;
        _loading = false;
      });
      final Map<String, String> labels = await _loadLabels(sessions);
      if (!mounted || labels.isEmpty) return;
      setState(() => _labels = labels);
    } catch (_) {
      if (!mounted) return;
      setState(() {
        _error =
            'The recording history could not be read. Is the daemon '
            'running?';
        _loading = false;
      });
    }
  }

  Future<FpsRecordStatus?> _loadStatus() async {
    try {
      return await FpsRecordBackend.instance.status();
    } catch (_) {
      return null;
    }
  }

  /// Package labels come from the same app list the per-app screens use. A
  /// failure here only costs the friendly name, never the history.
  Future<Map<String, String>> _loadLabels(List<FpsSession> sessions) async {
    final Set<String> packages = <String>{
      for (final FpsSession session in sessions)
        if (session.package.isNotEmpty) session.package,
    };
    if (packages.isEmpty) {
      return const <String, String>{};
    }
    try {
      final dynamic data = await PerAppBackend.instance
          .command('GET app.list')
          .timeout(const Duration(seconds: 5));
      if (data is! List) {
        return const <String, String>{};
      }
      final Map<String, String> labels = <String, String>{};
      for (final dynamic app in data) {
        if (app is! Map<String, dynamic>) continue;
        final Object? package = app['package'];
        if (package is! String || !packages.contains(package)) continue;
        final Object? label = app['label'];
        labels[package] = label is String && label.isNotEmpty ? label : package;
      }
      return labels;
    } catch (_) {
      return const <String, String>{};
    }
  }

  String _labelFor(String package) {
    if (package.isEmpty) {
      return 'Unknown app';
    }
    return _labels[package] ?? package;
  }

  List<MapEntry<String, List<FpsSession>>> _groups() {
    final Map<String, List<FpsSession>> groups = <String, List<FpsSession>>{};
    for (final FpsSession session in _sessions) {
      groups.putIfAbsent(session.package, () => <FpsSession>[]).add(session);
    }
    return groups.entries.toList();
  }

  Future<void> _copy(String text, String message) async {
    await Clipboard.setData(ClipboardData(text: text));
    if (!mounted) return;
    DashHaptics.tap();
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(SnackBar(content: Text(message)));
  }

  Future<void> _copyEverything() async {
    final StringBuffer buffer = StringBuffer()
      ..writeln(
        'package,started,duration_s,samples,avg_fps,low_1pct_fps,min_fps,'
        'max_fps,slow_percent',
      );
    for (final FpsSession session in _sessions) {
      buffer.writeln(
        '${session.package},'
        '${session.startedAt.toIso8601String()},'
        '${session.duration.inMilliseconds / 1000},'
        '${session.samples},'
        '${_fpsCsvNumber(session.average)},'
        '${_fpsCsvNumber(session.low1)},'
        '${_fpsCsvNumber(session.minimum)},'
        '${_fpsCsvNumber(session.maximum)},'
        '${_fpsCsvNumber(session.slowPercent)}',
      );
    }
    await _copy(buffer.toString(), 'Session table copied as CSV');
  }

  Future<void> _stopAndSave() async {
    try {
      await FpsRecordBackend.instance.setRecording(false);
    } catch (_) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('The recording could not be stopped.')),
      );
      return;
    }
    widget.onStopped?.call();
    await _load();
  }

  Future<void> _clear() async {
    final bool confirmed =
        await showDialog<bool>(
          context: context,
          builder: (BuildContext context) => AlertDialog(
            title: const Text('Clear FPS history?'),
            content: const Text(
              'Every recorded session is deleted. A recording that is running '
              'now keeps running.',
            ),
            actions: <Widget>[
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: const Text('Cancel'),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(context, true),
                child: const Text('Clear'),
              ),
            ],
          ),
        ) ??
        false;
    if (!confirmed) return;
    try {
      await FpsRecordBackend.instance.clear();
    } catch (_) {
      // The list reload below is the source of truth.
    }
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme colors = theme.colorScheme;
    return Scaffold(
      appBar: AppBar(
        title: const Text('FPS history'),
        actions: <Widget>[
          if (_sessions.isNotEmpty)
            IconButton(
              tooltip: 'Copy CSV',
              onPressed: _copyEverything,
              icon: const Icon(Icons.copy_all_rounded),
            ),
          if (_sessions.isNotEmpty)
            IconButton(
              tooltip: 'Clear history',
              onPressed: _clear,
              icon: const Icon(Icons.delete_outline_rounded),
            ),
        ],
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : _error != null
          ? _message(colors, icon: Icons.error_outline_rounded, title: _error!)
          : _buildBody(colors),
    );
  }

  Widget _buildBody(ColorScheme colors) {
    final List<MapEntry<String, List<FpsSession>>> groups = _groups();
    return ListView(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 32),
      children: <Widget>[
        if (_status?.active == true) ...<Widget>[
          _buildRecordingCard(colors),
          const SizedBox(height: 14),
        ],
        if (groups.isEmpty)
          _message(
            colors,
            icon: Icons.insights_rounded,
            title: 'No sessions yet',
            detail:
                'Turn on "Record FPS sessions" in Settings, play a game, then '
                'turn it off. The session is saved here with its frame-rate '
                'graph.',
          )
        else
          for (final MapEntry<String, List<FpsSession>> group in groups) ...[
            Padding(
              padding: const EdgeInsets.only(left: 4, bottom: 7),
              child: Text(
                _labelFor(group.key),
                style: TextStyle(
                  fontSize: 12.5,
                  fontWeight: FontWeight.w700,
                  color: colors.onSurfaceVariant,
                  letterSpacing: 0.2,
                ),
              ),
            ),
            SurfaceCard(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 2),
              child: Column(
                children: <Widget>[
                  for (
                    int index = 0;
                    index < group.value.length;
                    index += 1
                  ) ...[
                    if (index > 0) const Divider(height: 1),
                    _FpsSessionRow(
                      session: group.value[index],
                      onTap: () => Navigator.of(context).push<void>(
                        MaterialPageRoute<void>(
                          builder: (BuildContext context) =>
                              _FpsSessionDetailScreen(
                                session: group.value[index],
                                label: _labelFor(group.key),
                              ),
                        ),
                      ),
                    ),
                  ],
                ],
              ),
            ),
            const SizedBox(height: 14),
          ],
      ],
    );
  }

  Widget _buildRecordingCard(ColorScheme colors) {
    final FpsRecordStatus status = _status!;
    return SurfaceCard(
      child: Row(
        children: <Widget>[
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Text(
                  'Recording now',
                  style: TextStyle(
                    fontSize: 14,
                    fontWeight: FontWeight.w700,
                    color: colors.primary,
                  ),
                ),
                const SizedBox(height: 3),
                Text(
                  '${_fpsDurationText(status.elapsed)} · '
                  '${status.samples} samples · '
                  '${status.average > 0 ? '${_fpsCsvNumber(status.average)} fps avg' : 'waiting for frames'}',
                  style: TextStyle(
                    fontSize: 11.5,
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(width: 10),
          FilledButton.tonal(
            onPressed: _stopAndSave,
            child: const Text('Stop and save'),
          ),
        ],
      ),
    );
  }

  Widget _message(
    ColorScheme colors, {
    required IconData icon,
    required String title,
    String? detail,
  }) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 36, vertical: 40),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Icon(icon, size: 38, color: colors.onSurfaceVariant),
            const SizedBox(height: 12),
            Text(
              title,
              textAlign: TextAlign.center,
              style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w700),
            ),
            if (detail != null) ...<Widget>[
              const SizedBox(height: 7),
              Text(
                detail,
                textAlign: TextAlign.center,
                style: TextStyle(
                  fontSize: 12,
                  height: 1.45,
                  color: colors.onSurfaceVariant,
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class _FpsSessionRow extends StatelessWidget {
  const _FpsSessionRow({required this.session, required this.onTap});

  final FpsSession session;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final ColorScheme colors = Theme.of(context).colorScheme;
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(10),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 11),
        child: Row(
          children: <Widget>[
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: <Widget>[
                  Text(
                    _fpsSessionDate(session.startedAt),
                    style: const TextStyle(
                      fontSize: 13,
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                  const SizedBox(height: 3),
                  Text(
                    'avg ${_fpsCsvNumber(session.average)} fps · '
                    '1% low ${_fpsCsvNumber(session.low1)} · '
                    '${_fpsDurationText(session.duration)}',
                    style: TextStyle(
                      fontSize: 11.5,
                      color: colors.onSurfaceVariant,
                    ),
                  ),
                ],
              ),
            ),
            Icon(
              Icons.chevron_right_rounded,
              size: 20,
              color: colors.onSurfaceVariant,
            ),
          ],
        ),
      ),
    );
  }
}

class _FpsSessionDetailScreen extends StatefulWidget {
  const _FpsSessionDetailScreen({required this.session, required this.label});

  final FpsSession session;
  final String label;

  @override
  State<_FpsSessionDetailScreen> createState() =>
      _FpsSessionDetailScreenState();
}

class _FpsSessionDetailScreenState extends State<_FpsSessionDetailScreen> {
  late _FpsMetric _metric;
  bool _savingNote = false;

  @override
  void initState() {
    super.initState();
    // Default to FPS, falling back to the first metric the daemon measured.
    _metric = widget.session.available.contains('fps')
        ? _FpsMetric.all.first
        : _FpsMetric.all.firstWhere(
            (m) => widget.session.available.contains(m.key),
            orElse: () => _FpsMetric.all.first,
          );
  }

  List<double> _seriesFor(_FpsMetric metric) {
    if (metric.key == 'fps') {
      return widget.session.series;
    }
    return widget.session.metricSeries(metric.key);
  }

  /// CSV for the currently selected metric series.
  String _csv() {
    final List<double> series = _seriesFor(_metric);
    double avg = 0;
    double min = 0;
    double max = 0;
    if (series.isNotEmpty) {
      avg = series.reduce((a, b) => a + b) / series.length;
      min = series.reduce(math.min);
      max = series.reduce(math.max);
    }
    final StringBuffer buffer = StringBuffer()
      ..writeln('package,${widget.session.package}')
      ..writeln('started,${widget.session.startedAt.toIso8601String()}')
      ..writeln(
          'duration_s,${widget.session.duration.inMilliseconds / 1000}')
      ..writeln('samples,${widget.session.samples}')
      ..writeln('avg_${_metric.key},${_fpsCsvNumber(avg)}')
      ..writeln('min_${_metric.key},${_fpsCsvNumber(min)}')
      ..writeln('max_${_metric.key},${_fpsCsvNumber(max)}')
      ..writeln('seconds,${_metric.unit}');
    for (int index = 0; index < series.length; index += 1) {
      final double seconds =
          index * widget.session.seriesStep.inMilliseconds / 1000;
      buffer.writeln(
          '${seconds.toStringAsFixed(1)},${series[index].toStringAsFixed(_metric.decimals)}');
    }
    return buffer.toString();
  }

  Future<void> _copyCsv() async {
    await Clipboard.setData(ClipboardData(text: _csv()));
    if (!mounted) return;
    DashHaptics.tap();
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('Session copied as CSV')),
    );
  }

  Future<void> _exportFile() async {
    final List<double> series = _seriesFor(_metric);
    final String png = await _graphPngBytes(series);
    if (!mounted) return;
    try {
      final bool ok = await FpsRecordBackend.instance.exportFile(
        widget.session.id,
        _csv(),
        png,
      );
      if (!mounted) return;
      DashHaptics.tap();
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            ok
                ? 'FPS data exported to CSV'
                : 'The export could not be saved.',
          ),
        ),
      );
    } catch (_) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('The export could not be saved.')),
      );
    }
  }

  /// Renders the current graph to a PNG and returns a base64 string for the
  /// host to write to public storage. Returns an empty string on failure.
  Future<String> _graphPngBytes(List<double> series) async {
    final recorder = ui.PictureRecorder();
    final canvas = Canvas(recorder);
    final size = const Size(760, 420);
    final Rect plot = Rect.fromLTRB(40, 30, size.width - 16, size.height - 46);
    final double peak = series.isNotEmpty
        ? series.reduce(math.max)
        : 0;
    final double ceiling = math.max(30, (peak / 30).ceil() * 30).toDouble();
    double y(double value) =>
        plot.bottom - (value / ceiling).clamp(0.0, 1.0) * plot.height;
    double x(int index) => series.length == 1
        ? plot.left
        : plot.left + plot.width * index / (series.length - 1);

    final Paint gridPaint = Paint()
      ..color = const Color(0xFFE0E0E0)
      ..strokeWidth = 1;
    final TextPainter label = TextPainter(textDirection: TextDirection.ltr);
    for (int step = 0; step <= 3; step += 1) {
      final double value = ceiling * step / 3;
      final double lineY = y(value);
      canvas.drawLine(
        Offset(plot.left, lineY),
        Offset(plot.right, lineY),
        gridPaint,
      );
      label
        ..text = TextSpan(
          text: value.round().toString(),
          style: const TextStyle(fontSize: 11, color: Color(0xFF666666)),
        )
        ..layout();
      label.paint(
        canvas,
        Offset(plot.left - label.width - 8, lineY - label.height / 2),
      );
    }

    final Color accent = DashAppearanceScope.of(context).activeAccent;
    final Path line = Path();
    for (int index = 0; index < series.length; index += 1) {
      final Offset point = Offset(x(index), y(series[index]));
      if (index == 0) {
        line.moveTo(point.dx, point.dy);
      } else {
        line.lineTo(point.dx, point.dy);
      }
    }
    final Path area = Path.from(line)
      ..lineTo(plot.right, plot.bottom)
      ..lineTo(plot.left, plot.bottom)
      ..close();
    canvas.drawPath(
      area,
      Paint()
        ..shader = ui.Gradient.linear(
          plot.topLeft,
          plot.bottomLeft,
          <Color>[accent.withValues(alpha: 0.25), accent.withValues(alpha: 0.02)],
        ),
    );
    canvas.drawPath(
      line,
      Paint()
        ..color = accent
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..strokeJoin = StrokeJoin.round
        ..strokeCap = StrokeCap.round,
    );

    final ui.Picture picture = recorder.endRecording();
    final img = await picture.toImage(size.width.toInt(), size.height.toInt());
    final data = await img.toByteData(format: ui.ImageByteFormat.png);
    final List<int> bytes = data?.buffer.asUint8List() ?? <int>[];
    return base64Encode(bytes);
  }

  Future<void> _editNote() async {
    final TextEditingController controller =
        TextEditingController(text: widget.session.description);
    final bool saved = await showDialog<bool>(
          context: context,
          builder: (BuildContext context) => AlertDialog(
            title: const Text('Session note'),
            content: TextField(
              controller: controller,
              maxLines: 4,
              maxLength: 200,
              decoration: const InputDecoration(
                hintText: 'Add a description for this session',
                border: OutlineInputBorder(),
              ),
            ),
            actions: <Widget>[
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: const Text('Cancel'),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(context, true),
                child: const Text('Save'),
              ),
            ],
          ),
        ) ??
        false;
    if (!saved || !mounted) return;
    final String next = controller.text.trim();
    if (next == widget.session.description) return;
    setState(() => _savingNote = true);
    try {
      final bool ok =
          await FpsRecordBackend.instance.setNote(widget.session.id, next);
      if (!mounted) return;
      if (ok) {
        DashHaptics.tap();
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Note saved')),
        );
      } else {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('The session could not be found.')),
        );
      }
    } catch (_) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('The note could not be saved.')),
      );
    } finally {
      if (mounted) setState(() => _savingNote = false);
    }
  }

  Future<void> _delete() async {
    final bool confirmed = await showDialog<bool>(
          context: context,
          builder: (BuildContext context) => AlertDialog(
            title: const Text('Delete session?'),
            content: const Text(
              'This session and its graph are removed. A recording that is '
              'running now keeps running.',
            ),
            actions: <Widget>[
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: const Text('Cancel'),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(context, true),
                child: const Text('Delete'),
              ),
            ],
          ),
        ) ??
        false;
    if (!confirmed || !mounted) return;
    try {
      final bool deleted =
          await FpsRecordBackend.instance.delete(widget.session.id);
      if (!mounted) return;
      if (deleted) {
        DashHaptics.tap();
        Navigator.of(context).pop(true);
      } else {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('The session could not be found.')),
        );
      }
    } catch (_) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('The session could not be deleted.')),
      );
    }
  }

  List<Widget> _statTiles(ColorScheme colors) {
    final List<Widget> tiles = <Widget>[
      _FpsStatTile(
        label: 'Average',
        value: '${_fpsCsvNumber(widget.session.average)} fps',
      ),
      _FpsStatTile(
        label: '5% low',
        value: '${_fpsCsvNumber(widget.session.low5)} fps',
        show: widget.session.available.isNotEmpty &&
            widget.session.low5 > 0,
      ),
      _FpsStatTile(
        label: 'Minimum',
        value: '${_fpsCsvNumber(widget.session.minimum)} fps',
      ),
      _FpsStatTile(
        label: 'Maximum',
        value: '${_fpsCsvNumber(widget.session.maximum)} fps',
      ),
      _FpsStatTile(
        label: 'Variance',
        value: '${widget.session.variance.toStringAsFixed(2)} fps\u00b2',
        show: widget.session.variance > 0,
      ),
      _FpsStatTile(
        label: 'Smoothness',
        value: '${_fpsCsvNumber(widget.session.smoothPercent)} %',
        show: widget.session.smoothPercent > 0,
      ),
      _FpsStatTile(
        label: 'Jitter',
        value: '${_fpsCsvNumber(widget.session.jitterPercent)} %',
        show: widget.session.jitterPercent > 0,
      ),
      _FpsStatTile(
        label: '99% frame time',
        value:
            '${widget.session.p99FrameTime.inMicroseconds / 1000} ms',
        show: widget.session.p99FrameTime.inMicroseconds > 0,
      ),
      _FpsStatTile(
        label: 'Jank',
        value: '${widget.session.jank}',
        show: widget.session.jank > 0,
      ),
      _FpsStatTile(
        label: 'Duration',
        value: _fpsDurationText(widget.session.duration),
      ),
      _FpsStatTile(
        label: 'Samples',
        value: '${widget.session.samples}',
      ),
    ];
    // Telemetry summaries, only when the daemon measured them.
    if (widget.session.cpuAverage > 0 || widget.session.cpuMaximum > 0) {
      tiles.add(_FpsStatTile(
        label: 'CPU avg / max',
        value:
            '${widget.session.cpuAverage.round()}% / ${widget.session.cpuMaximum.round()}%',
      ));
    }
    if (widget.session.gpuLoadAverage > 0 || widget.session.gpuLoadMaximum > 0) {
      tiles.add(_FpsStatTile(
        label: 'GPU load avg / max',
        value:
            '${widget.session.gpuLoadAverage.round()}% / ${widget.session.gpuLoadMaximum.round()}%',
      ));
    }
    if (widget.session.gpuMhzAverage > 0 || widget.session.gpuMhzMaximum > 0) {
      tiles.add(_FpsStatTile(
        label: 'GPU MHz avg / max',
        value:
            '${widget.session.gpuMhzAverage.round()} / ${widget.session.gpuMhzMaximum.round()}',
      ));
    }
    if (widget.session.batteryTempAverage > 0 ||
        widget.session.batteryTempMaximum > 0) {
      tiles.add(_FpsStatTile(
        label: 'Battery temp avg / max',
        value:
            '${widget.session.batteryTempAverage.toStringAsFixed(1)}\u00b0C / ${widget.session.batteryTempMaximum.toStringAsFixed(1)}\u00b0C',
      ));
    }
    if (widget.session.powerAverage > 0 || widget.session.powerMaximum > 0) {
      tiles.add(_FpsStatTile(
        label: 'Power avg / max',
        value:
            '${widget.session.powerAverage.toStringAsFixed(1)} W / ${widget.session.powerMaximum.toStringAsFixed(1)} W',
      ));
    }
    if (widget.session.ramAverage > 0 || widget.session.ramMaximum > 0) {
      tiles.add(_FpsStatTile(
        label: 'RAM avg / max',
        value:
            '${widget.session.ramAverage.round()} MB / ${widget.session.ramMaximum.round()} MB',
      ));
    }
    if (widget.session.packageRamAverage > 0 ||
        widget.session.packageRamMaximum > 0) {
      tiles.add(_FpsStatTile(
        label: 'Package RAM avg / max',
        value:
            '${widget.session.packageRamAverage.round()} MB / ${widget.session.packageRamMaximum.round()} MB',
      ));
    }
    if (widget.session.cpuTempMaximum > 0) {
      tiles.add(_FpsStatTile(
        label: 'CPU temp max',
        value: '${widget.session.cpuTempMaximum.toStringAsFixed(1)}\u00b0C',
      ));
    }
    if (widget.session.gpuTempMaximum > 0) {
      tiles.add(_FpsStatTile(
        label: 'GPU temp max',
        value: '${widget.session.gpuTempMaximum.toStringAsFixed(1)}\u00b0C',
      ));
    }
    return tiles;
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme colors = theme.colorScheme;
    final bool autoTag = widget.session.auto;
    return Scaffold(
      appBar: AppBar(
        title: Text(widget.label, overflow: TextOverflow.ellipsis),
        actions: <Widget>[
          IconButton(
            tooltip: 'Edit note',
            onPressed: _savingNote ? null : _editNote,
            icon: const Icon(Icons.edit_note_rounded),
          ),
          IconButton(
            tooltip: 'Delete session',
            onPressed: _savingNote ? null : _delete,
            icon: const Icon(Icons.delete_outline_rounded),
          ),
          IconButton(
            tooltip: 'Export graph + CSV',
            onPressed: _exportFile,
            icon: const Icon(Icons.save_alt_rounded),
          ),
          IconButton(
            tooltip: 'Copy CSV',
            icon: const Icon(Icons.copy_all_rounded),
            onPressed: _copyCsv,
          ),
        ],
      ),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 12, 16, 32),
        children: <Widget>[
          Row(
            crossAxisAlignment: CrossAxisAlignment.end,
            children: <Widget>[
              Text(
                _fpsCsvNumber(widget.session.average),
                style: TextStyle(
                  fontSize: 34,
                  height: 1,
                  fontWeight: FontWeight.w800,
                  color: DashAppearanceScope.of(context).activeAccent,
                ),
              ),
              const SizedBox(width: 5),
              Expanded(
                child: Padding(
                  padding: const EdgeInsets.only(bottom: 3),
                  child: Text(
                    'fps average',
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 12,
                      color: colors.onSurfaceVariant,
                    ),
                  ),
                ),
              ),
              const SizedBox(width: 8),
              Flexible(
                child: Text(
                  '${_fpsSessionDate(widget.session.startedAt)}'
                  '${autoTag ? '  ·  auto' : ''}',
                  textAlign: TextAlign.right,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 11.5,
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 10),
          if (widget.session.description.isNotEmpty) ...<Widget>[
            SurfaceCard(
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: Text(
                  widget.session.description,
                  style: TextStyle(
                    fontSize: 13,
                    color: colors.onSurfaceVariant,
                    height: 1.4,
                  ),
                ),
              ),
            ),
            const SizedBox(height: 10),
          ],
          // Graph with metric selector.
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Wrap(
                spacing: 6,
                runSpacing: 6,
                children: <Widget>[
                  for (final _FpsMetric m in _FpsMetric.all)
                    if (m.key == 'fps' ||
                        widget.session.available.contains(m.key))
                      FilterChip(
                        label: Text(m.label),
                        selected: _metric == m,
                        onSelected: (bool picked) {
                          if (picked) {
                            setState(() => _metric = m);
                          }
                        },
                        avatar: _metric == m
                            ? const Icon(Icons.check, size: 16)
                            : null,
                      ),
                ],
              ),
              const SizedBox(height: 8),
              SizedBox(
                height: 190,
                width: double.infinity,
                child: CustomPaint(
                  painter: _FpsGraphPainter(
                    series: _seriesFor(_metric),
                    average: widget.session.average,
                    unit: _metric.unit,
                    decimals: _metric.decimals,
                    accent: DashAppearanceScope.of(context).activeAccent,
                    gridColor:
                        theme.colorScheme.outlineVariant.withValues(alpha: 0.4),
                    labelColor: theme.colorScheme.onSurfaceVariant,
                  ),
                ),
              ),
              const SizedBox(height: 6),
              Text(
                '${_fpsDurationText(widget.session.duration)} · '
                '${widget.session.samples} samples · '
                'one point every ${widget.session.seriesStep.inMilliseconds} ms · '
                '${_metric.label}',
                style: TextStyle(
                  fontSize: 10.5,
                  color: colors.onSurfaceVariant,
                ),
              ),
            ],
          ),
          const SizedBox(height: 14),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: _statTiles(colors),
          ),
          const SizedBox(height: 14),
          SurfaceCard(
            child: Text(
              'The 5% low is the fifth percentile of the frame rate. A sample '
              'counts as slow when it runs more than ten percent under the '
              'session median, and smoothness is the share of samples within '
              'ten percent of the average. Jank is a frame slower than one and '
              'a half times the mean frame time.',
              style: TextStyle(
                fontSize: 11.5,
                height: 1.45,
                color: colors.onSurfaceVariant,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _FpsStatTile extends StatelessWidget {
  const _FpsStatTile({
    required this.label,
    required this.value,
    this.show = true,
  });

  final String label;
  final String value;
  final bool show;

  @override
  Widget build(BuildContext context) {
    if (!show) {
      return const SizedBox.shrink();
    }
    final ColorScheme colors = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
      decoration: BoxDecoration(
        color: colors.surfaceContainerHighest.withValues(alpha: 0.42),
        borderRadius: BorderRadius.circular(11),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          Text(
            label,
            style: TextStyle(fontSize: 10, color: colors.onSurfaceVariant),
          ),
          const SizedBox(height: 2),
          Text(
            value,
            style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w700),
          ),
        ],
      ),
    );
  }
}

/// Hand-painted graph for any recorded metric: one line, a soft fill and a
/// ceiling that follows the data so a spike stays readable.
class _FpsGraphPainter extends CustomPainter {
  _FpsGraphPainter({
    required this.series,
    required this.average,
    required this.unit,
    required this.decimals,
    required this.accent,
    required this.gridColor,
    required this.labelColor,
  });

  final List<double> series;
  final double average;
  final String unit;
  final int decimals;
  final Color accent;
  final Color gridColor;
  final Color labelColor;

  static const double _leftGutter = 34;
  static const double _rightGutter = 8;
  static const double _topGutter = 12;
  static const double _bottomGutter = 20;

  @override
  void paint(Canvas canvas, Size size) {
    if (series.isEmpty ||
        size.width <= _leftGutter + _rightGutter + 8) {
      return;
    }
    final Rect plot = Rect.fromLTRB(
      _leftGutter,
      _topGutter,
      size.width - _rightGutter,
      size.height - _bottomGutter,
    );
    final double peak = series.reduce(math.max);
    final double ceiling = math.max(30, (peak / 30).ceil() * 30).toDouble();
    double y(double value) =>
        plot.bottom - (value / ceiling).clamp(0.0, 1.0) * plot.height;
    double x(int index) => series.length == 1
        ? plot.left
        : plot.left + plot.width * index / (series.length - 1);

    final Paint gridPaint = Paint()
      ..color = gridColor
      ..strokeWidth = 1;
    final TextPainter label = TextPainter(textDirection: TextDirection.ltr);
    for (int step = 0; step <= 3; step += 1) {
      final double value = ceiling * step / 3;
      final double lineY = y(value);
      canvas.drawLine(
        Offset(plot.left, lineY),
        Offset(plot.right, lineY),
        gridPaint,
      );
      label
        ..text = TextSpan(
          text: value.toStringAsFixed(decimals),
          style: TextStyle(fontSize: 9, color: labelColor),
        )
        ..layout();
      label.paint(
        canvas,
        Offset(plot.left - label.width - 6, lineY - label.height / 2),
      );
    }

    if (average > 0) {
      final Paint dashPaint = Paint()
        ..color = accent.withValues(alpha: 0.55)
        ..strokeWidth = 1;
      for (double dx = plot.left; dx < plot.right; dx += 6) {
        canvas.drawLine(
          Offset(dx, y(average)),
          Offset(math.min(dx + 3, plot.right), y(average)),
          dashPaint,
        );
      }
    }

    final Path line = Path();
    for (int index = 0; index < series.length; index += 1) {
      final Offset point = Offset(x(index), y(series[index]));
      if (index == 0) {
        line.moveTo(point.dx, point.dy);
      } else {
        line.lineTo(point.dx, point.dy);
      }
    }
    final Path area = Path.from(line)
      ..lineTo(plot.right, plot.bottom)
      ..lineTo(plot.left, plot.bottom)
      ..close();
    canvas.drawPath(
      area,
      Paint()
        ..shader = ui.Gradient.linear(plot.topLeft, plot.bottomLeft, <Color>[
          accent.withValues(alpha: 0.30),
          accent.withValues(alpha: 0.02),
        ]),
    );
    canvas.drawPath(
      line,
      Paint()
        ..color = accent
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..strokeJoin = StrokeJoin.round
        ..strokeCap = StrokeCap.round,
    );
    canvas.drawCircle(
      Offset(x(series.length - 1), y(series.last)),
      2.6,
      Paint()..color = accent,
    );
  }

  @override
  bool shouldRepaint(_FpsGraphPainter oldDelegate) {
    return oldDelegate.series != series ||
        oldDelegate.average != average ||
        oldDelegate.unit != unit ||
        oldDelegate.decimals != decimals ||
        oldDelegate.accent != accent ||
        oldDelegate.gridColor != gridColor;
  }
}
