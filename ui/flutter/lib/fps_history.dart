part of 'main.dart';

// Recorded FPS sessions: one card per game, a detail view with the frame-rate
// graph, and CSV export. Recording itself lives in the daemon, so closing the
// app never loses an in-progress session.

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

class _FpsSessionDetailScreen extends StatelessWidget {
  const _FpsSessionDetailScreen({required this.session, required this.label});

  final FpsSession session;
  final String label;

  String _csv() {
    final StringBuffer buffer = StringBuffer()
      ..writeln('package,${session.package}')
      ..writeln('started,${session.startedAt.toIso8601String()}')
      ..writeln('duration_s,${session.duration.inMilliseconds / 1000}')
      ..writeln('samples,${session.samples}')
      ..writeln('avg_fps,${_fpsCsvNumber(session.average)}')
      ..writeln('low_1pct_fps,${_fpsCsvNumber(session.low1)}')
      ..writeln('min_fps,${_fpsCsvNumber(session.minimum)}')
      ..writeln('max_fps,${_fpsCsvNumber(session.maximum)}')
      ..writeln('slow_percent,${_fpsCsvNumber(session.slowPercent)}')
      ..writeln('seconds,fps');
    for (int index = 0; index < session.series.length; index += 1) {
      final double seconds = index * session.seriesStep.inMilliseconds / 1000;
      buffer.writeln(
        '${seconds.toStringAsFixed(1)},'
        '${_fpsCsvNumber(session.series[index])}',
      );
    }
    return buffer.toString();
  }

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    final ColorScheme colors = theme.colorScheme;
    return Scaffold(
      appBar: AppBar(
        title: Text(label, overflow: TextOverflow.ellipsis),
        actions: <Widget>[
          IconButton(
            tooltip: 'Copy CSV',
            icon: const Icon(Icons.copy_all_rounded),
            onPressed: () async {
              await Clipboard.setData(ClipboardData(text: _csv()));
              if (!context.mounted) return;
              DashHaptics.tap();
              ScaffoldMessenger.of(context).showSnackBar(
                const SnackBar(content: Text('Session copied as CSV')),
              );
            },
          ),
        ],
      ),
      body: ListView(
        padding: const EdgeInsets.fromLTRB(16, 12, 16, 32),
        children: <Widget>[
          SurfaceCard(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Row(
                  crossAxisAlignment: CrossAxisAlignment.end,
                  children: <Widget>[
                    Text(
                      _fpsCsvNumber(session.average),
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
                        _fpsSessionDate(session.startedAt),
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
                const SizedBox(height: 14),
                _FpsGraph(session: session),
                const SizedBox(height: 6),
                Text(
                  '${_fpsDurationText(session.duration)} · '
                  '${session.samples} samples · '
                  'one point every ${session.seriesStep.inMilliseconds} ms',
                  style: TextStyle(
                    fontSize: 10.5,
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 14),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: <Widget>[
              _FpsStatTile(
                label: '1% low',
                value: '${_fpsCsvNumber(session.low1)} fps',
              ),
              _FpsStatTile(
                label: 'Minimum',
                value: '${_fpsCsvNumber(session.minimum)} fps',
              ),
              _FpsStatTile(
                label: 'Maximum',
                value: '${_fpsCsvNumber(session.maximum)} fps',
              ),
              _FpsStatTile(
                label: 'Slow samples',
                value: '${_fpsCsvNumber(session.slowPercent)} %',
              ),
              _FpsStatTile(
                label: 'Duration',
                value: _fpsDurationText(session.duration),
              ),
              _FpsStatTile(label: 'Samples', value: '${session.samples}'),
            ],
          ),
          const SizedBox(height: 14),
          SurfaceCard(
            child: Text(
              'The 1% low averages the slowest one percent of samples, so one '
              'stray reading cannot define it. A sample counts as slow when it '
              'runs more than ten percent under the session median.',
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
  const _FpsStatTile({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
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

class _FpsGraph extends StatelessWidget {
  const _FpsGraph({required this.session});

  final FpsSession session;

  @override
  Widget build(BuildContext context) {
    final ThemeData theme = Theme.of(context);
    return SizedBox(
      height: 168,
      width: double.infinity,
      child: CustomPaint(
        painter: _FpsGraphPainter(
          series: session.series,
          average: session.average,
          accent: DashAppearanceScope.of(context).activeAccent,
          gridColor: theme.colorScheme.outlineVariant.withValues(alpha: 0.4),
          labelColor: theme.colorScheme.onSurfaceVariant,
        ),
      ),
    );
  }
}

/// Hand-painted frame-rate graph: no chart package, one line, a soft fill and
/// a dashed average so a dip is readable without a legend.
class _FpsGraphPainter extends CustomPainter {
  _FpsGraphPainter({
    required this.series,
    required this.average,
    required this.accent,
    required this.gridColor,
    required this.labelColor,
  });

  final List<double> series;
  final double average;
  final Color accent;
  final Color gridColor;
  final Color labelColor;

  static const double _leftGutter = 30;
  static const double _rightGutter = 6;
  static const double _topGutter = 10;
  static const double _bottomGutter = 16;

  @override
  void paint(Canvas canvas, Size size) {
    if (series.isEmpty || size.width <= _leftGutter + _rightGutter + 8) {
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
          text: value.round().toString(),
          style: TextStyle(fontSize: 9, color: labelColor),
        )
        ..layout();
      label.paint(
        canvas,
        Offset(plot.left - label.width - 5, lineY - label.height / 2),
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
        oldDelegate.accent != accent ||
        oldDelegate.gridColor != gridColor;
  }
}
