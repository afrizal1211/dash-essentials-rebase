part of 'main.dart';

// The health and log view.
//
// The daemon knows why a control did not take effect, but that knowledge used
// to reach a logcat line that scrolled away within seconds. A user who cannot
// tell which feature is broken has nothing to act on, so the daemon keeps a
// bounded history and this screen shows it, together with the state of every
// feature the app owns.
//
// Nothing here changes a setting. The screen only reads.

class DiagnosticsLogsScreen extends StatefulWidget {
  const DiagnosticsLogsScreen({required this.onBack, super.key});

  final VoidCallback onBack;

  @override
  State<DiagnosticsLogsScreen> createState() => _DiagnosticsLogsScreenState();
}

class _DiagnosticsLogsScreenState extends State<DiagnosticsLogsScreen> {
  Map<String, dynamic>? _health;
  List<Map<String, dynamic>> _events = <Map<String, dynamic>>[];
  String? _error;
  bool _busy = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _refresh());
  }

  Future<void> _refresh() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final dynamic health = await PerAppBackend.instance.command('GET health');
      final dynamic logs = await PerAppBackend.instance.command('GET logs');
      if (!mounted) return;
      final List<dynamic> events = _eventList(logs);
      setState(() {
        _health = health is Map
            ? Map<String, dynamic>.from(health)
            : <String, dynamic>{};
        _events = events
            .whereType<Map<String, dynamic>>()
            .toList(growable: false);
      });
    } catch (error) {
      if (mounted) setState(() => _error = '$error');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  static String _ownedList(Object? owned) {
    if (owned is! List) return 'nothing';
    final List<String> names = <String>[
      for (final dynamic item in owned) '$item',
    ];
    if (names.isEmpty) return 'nothing';
    return names.join(', ');
  }

  static List<dynamic> _eventList(dynamic logs) {
    if (logs is! Map) return <dynamic>[];
    final Object? events = logs['events'];
    if (events is List) return events;
    return <dynamic>[];
  }

  Future<void> _clear() async {
    try {
      await PerAppBackend.instance.command('ACTION logs.clear');
      await _refresh();
    } catch (error) {
      if (mounted) setState(() => _error = '$error');
    }
  }

  String _plainText() {
    final StringBuffer buffer = StringBuffer();
    if (_health != null) {
      buffer.writeln('== HEALTH ==');
      _health!.forEach((String key, dynamic value) {
        buffer.writeln('$key: $value');
      });
      buffer.writeln();
    }
    buffer.writeln('== LOG ==');
    for (final Map<String, dynamic> event in _events.reversed) {
      buffer.writeln(
        '[${event['seq']}] ${event['area']}/${event['level']} '
        '${event['what']}: ${event['detail']}',
      );
    }
    return buffer.toString();
  }

  Future<void> _copy() async {
    await Clipboard.setData(ClipboardData(text: _plainText()));
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('Diagnostics copied to the clipboard')),
    );
  }

  /// Write the report to a text file. The daemon writes it, because it holds
  /// the state and the history, and it writes it where a person can find it
  /// again without a root shell.
  Future<void> _export() async {
    try {
      final dynamic result = await PerAppBackend.instance.command(
        'ACTION logs.export',
      );
      if (!mounted) return;
      final String path = result is Map ? '${result['path']}' : '$result';
      await Clipboard.setData(ClipboardData(text: path));
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Report written to $path (the path is on the clipboard)'),
          duration: const Duration(seconds: 6),
        ),
      );
    } catch (error) {
      if (!mounted) return;
      setState(() => _error = '$error');
    }
  }

  Map<String, dynamic> _section(String name) {
    final Object? value = _health?[name];
    if (value is Map) return Map<String, dynamic>.from(value);
    return <String, dynamic>{};
  }

  @override
  Widget build(BuildContext context) {
    final ColorScheme colors = Theme.of(context).colorScheme;

    return DashScrollPage(
      children: <Widget>[
        DetailHeader(title: 'Health & Logs', onBack: widget.onBack),
        const SizedBox(height: 4),
        Text(
          'What the daemon is doing, and why a control did not take effect',
          style: TextStyle(fontSize: 13.5, color: colors.onSurfaceVariant),
        ),
        const SizedBox(height: 14),
        Row(
          children: <Widget>[
            Expanded(
              child: _HubControlRow(
                title: 'Refresh',
                subtitle: 'Re-read the daemon state',
                icon: Icons.refresh_rounded,
                accent: const Color(0xFF74E6C6),
                onTap: _refresh,
              ),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: _HubControlRow(
                title: 'Copy',
                subtitle: 'Copy the report and the log',
                icon: Icons.copy_all_rounded,
                accent: const Color(0xFF74E6C6),
                onTap: _copy,
              ),
            ),
          ],
        ),
        const SizedBox(height: 10),
        _HubControlRow(
          title: 'Export to a text file',
          subtitle: 'Write the report and the log to a .txt file',
          icon: Icons.description_outlined,
          accent: const Color(0xFF74E6C6),
          onTap: _export,
        ),
        const SizedBox(height: 10),
        _HubControlRow(
          title: 'Clear history',
          subtitle: 'Forget the recorded events',
          icon: Icons.delete_sweep_rounded,
          accent: const Color(0xFFFF8A65),
          onTap: _clear,
        ),
        if (_error != null) ...<Widget>[
          const SizedBox(height: 12),
          _DiagnosticsNote(
            color: const Color(0xFFFF6B6B),
            title: 'The daemon did not answer',
            detail: _error!,
          ),
        ],
        const SizedBox(height: 16),
        _buildGpuCard(),
        const SizedBox(height: 12),
        _buildConflictCard(),
        const SizedBox(height: 12),
        _buildPerAppCard(),
        const SizedBox(height: 12),
        _buildLogCard(colors),
        const SizedBox(height: 24),
      ],
    );
  }

  Widget _buildGpuCard() {
    final Map<String, dynamic> gpu = _section('gpu');
    final bool verified = gpu['verified'] == true;
    final bool armed = gpu['armed'] == true;
    final Object? capped = gpu['cappedBy'];

    final String state = verified
        ? 'Applied'
        : (armed
              ? (capped == null ? 'Armed' : 'Armed, capped by $capped')
              : 'Not applied');

    return SurfaceCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          _DiagnosticsTitle(
            icon: Icons.speed_rounded,
            title: 'GPU',
            accent: const Color(0xFF74E6C6),
            trailing: state,
          ),
          const SizedBox(height: 10),
          _DiagnosticsRow('Profile', 'profile ${gpu['profile'] ?? '-'}'),
          _DiagnosticsRow(
            'Target / live',
            '${gpu['targetMHz'] ?? '-'} MHz requested, '
                '${gpu['liveMHz'] ?? '-'} MHz running',
          ),
          _DiagnosticsRow(
            'Range',
            '${gpu['minMHz'] ?? '-'} - ${gpu['maxMHz'] ?? '-'} MHz, '
                'governor ${gpu['governor'] ?? '-'}',
          ),
          _DiagnosticsRow(
            'Ladder',
            '${gpu['ladderFloorMHz'] ?? '-'} - ${gpu['ladderTopMHz'] ?? '-'} MHz',
          ),
          _DiagnosticsRow(
            'Power / boost',
            'policy ${gpu['powerPolicy'] ?? '-'}, '
                'GED boost ${gpu['gedBoost'] ?? '-'}, '
                'DVFS ${gpu['dvfsEnabled'] ?? '-'}',
          ),
          _DiagnosticsRow(
            'Vendor ceiling',
            gpu['vendorCeilingBy'] == null
                ? 'none published'
                : '${gpu['vendorCeilingBy']} (index '
                    '${gpu['vendorCeilingIndex']})',
          ),
          if (capped != null) ...<Widget>[
            const SizedBox(height: 8),
            _DiagnosticsNote(
              color: const Color(0xFFFFB84D),
              title: 'A platform service is holding the GPU below the target',
              detail:
                  '$capped publishes a ceiling under the requested clock. The '
                  'profile is applied; the platform decides how fast the GPU '
                  'may run. This is not an error.',
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildConflictCard() {
    final Map<String, dynamic> game = _section('gameServices');
    final bool active = game['active'] == true;

    return SurfaceCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          _DiagnosticsTitle(
            icon: Icons.sports_esports_rounded,
            title: 'Game services',
            accent: const Color(0xFFFFB84D),
            trailing: active ? 'Active' : 'Idle',
          ),
          const SizedBox(height: 10),
          _DiagnosticsRow('State', '${game['summary'] ?? 'idle'}'),
          if (active) ...<Widget>[
            const SizedBox(height: 8),
            const _DiagnosticsNote(
              color: Color(0xFFFFB84D),
              title: 'These services own the same controls',
              detail:
                  'The MediaTek game driver and the task turbo publish their '
                  'own GPU limits and change the scheduling of the '
                  'foreground app. While a game is in the foreground they '
                  'compete with a per-app profile for the same nodes.',
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildPerAppCard() {
    final Map<String, dynamic> perApp = _section('perApp');
    final Object? foreground = perApp['foreground'];
    final Object? owned = perApp['ownedControls'];
    final Object? error = perApp['error'];

    return SurfaceCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          _DiagnosticsTitle(
            icon: Icons.apps_rounded,
            title: 'Per-app controls',
            accent: const Color(0xFF74E6C6),
            trailing: perApp['enabled'] == true ? 'On' : 'Off',
          ),
          const SizedBox(height: 10),
          _DiagnosticsRow(
            'Foreground',
            foreground is Map<String, dynamic>
                ? '${foreground['package']}'
                : 'none',
          ),
          _DiagnosticsRow(
            'Detected by',
            '${perApp['source'] ?? 'none'}',
          ),
          _DiagnosticsRow(
            'Saved profiles',
            '${perApp['configuredProfiles'] ?? 0}',
          ),
          _DiagnosticsRow('Owned by an app', _ownedList(owned)),
          _DiagnosticsRow(
            'Watching events',
            perApp['watching'] == true ? 'yes' : 'no',
          ),
          if (error != null) ...<Widget>[
            const SizedBox(height: 8),
            _DiagnosticsNote(
              color: const Color(0xFFFF6B6B),
              title: 'The per-app runtime reported a problem',
              detail: '$error',
            ),
          ],
        ],
      ),
    );
  }

  Widget _buildLogCard(ColorScheme colors) {
    return SurfaceCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          _DiagnosticsTitle(
            icon: Icons.receipt_long_rounded,
            title: 'History',
            accent: const Color(0xFF74E6C6),
            trailing: '${_events.length} events',
          ),
          const SizedBox(height: 8),
          if (_events.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 10),
              child: Text(
                'Nothing recorded yet. Change a setting and come back.',
                style: TextStyle(fontSize: 12.5, color: colors.onSurfaceVariant),
              ),
            ),
          for (final Map<String, dynamic> event in _events)
            _DiagnosticsEvent(event: event),
        ],
      ),
    );
  }
}

class _DiagnosticsTitle extends StatelessWidget {
  const _DiagnosticsTitle({
    required this.icon,
    required this.title,
    required this.accent,
    required this.trailing,
  });

  final IconData icon;
  final String title;
  final Color accent;
  final String trailing;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: <Widget>[
        Icon(icon, size: 18, color: accent),
        const SizedBox(width: 8),
        Expanded(
          child: Text(
            title,
            style: TextStyle(
              fontSize: 14,
              fontWeight: FontWeight.w800,
              color: Theme.of(context).colorScheme.onSurface,
            ),
          ),
        ),
        Text(
          trailing,
          style: TextStyle(
            fontSize: 11.5,
            fontWeight: FontWeight.w700,
            color: accent,
          ),
        ),
      ],
    );
  }
}

class _DiagnosticsRow extends StatelessWidget {
  const _DiagnosticsRow(this.label, this.value);

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    final ColorScheme colors = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          SizedBox(
            width: 116,
            child: Text(
              label,
              style: TextStyle(fontSize: 12, color: colors.onSurfaceVariant),
            ),
          ),
          Expanded(
            child: Text(
              value,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w600,
                color: colors.onSurface,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _DiagnosticsNote extends StatelessWidget {
  const _DiagnosticsNote({
    required this.color,
    required this.title,
    required this.detail,
  });

  final Color color;
  final String title;
  final String detail;

  @override
 Widget build(BuildContext context) {
    final ColorScheme colors = Theme.of(context).colorScheme;
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(10),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.10),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: color.withValues(alpha: 0.35)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            title,
            style: TextStyle(
              fontSize: 12.5,
              fontWeight: FontWeight.w800,
              color: color,
            ),
          ),
          const SizedBox(height: 4),
          Text(
            detail,
            style: TextStyle(fontSize: 11.5, color: colors.onSurfaceVariant),
          ),
        ],
      ),
    );
  }
}

class _DiagnosticsEvent extends StatelessWidget {
  const _DiagnosticsEvent({required this.event});

  final Map<String, dynamic> event;

  Color _levelColor() {
    switch (event['level']) {
      case 'error':
        return const Color(0xFFFF6B6B);
      case 'warn':
        return const Color(0xFFFFB84D);
      default:
        return const Color(0xFF74E6C6);
    }
  }

  @override
  Widget build(BuildContext context) {
    final ColorScheme colors = Theme.of(context).colorScheme;
    final Color level = _levelColor();
    final Object? at = event['at'];
    final String when = at is int
        ? DateTime.fromMillisecondsSinceEpoch(at).toIso8601String().substring(11, 19)
        : '';

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Row(
            children: <Widget>[
              Container(
                width: 7,
                height: 7,
                decoration: BoxDecoration(color: level, shape: BoxShape.circle),
              ),
              const SizedBox(width: 7),
              Expanded(
                child: Text(
                  '${event['area']} · ${event['what']}',
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w700,
                    color: level,
                  ),
                ),
              ),
              if (when.isNotEmpty)
                Text(
                  '$when  #${event['seq']}',
                  style: TextStyle(fontSize: 10.5, color: colors.onSurfaceVariant),
                ),
            ],
          ),
          const SizedBox(height: 2),
          Padding(
            padding: const EdgeInsets.only(left: 14),
            child: Text(
              '${event['detail']}',
              style: TextStyle(fontSize: 11.5, color: colors.onSurfaceVariant),
            ),
          ),
        ],
      ),
    );
  }
}
