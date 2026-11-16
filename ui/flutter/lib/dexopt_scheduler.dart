part of 'main.dart';

// The background dex recompiler scheduler.
//
// Android keeps app dex code in a compiled form that drifts out of date as
// the system updates and apps change. Background dexopt rebuilds it, but the
// platform only runs it when it decides to, so games still pay an interpreter
// tax on the first levels of a session. A schedule that retriggers the
// compiler while the screen is dark and the device is charging costs nothing
// the user can feel, which is why the daemon gates it on exactly those
// conditions.

class DexOptSchedulerScreen extends StatefulWidget {
  const DexOptSchedulerScreen({required this.onBack, super.key});

  final VoidCallback onBack;

  @override
  State<DexOptSchedulerScreen> createState() => _DexOptSchedulerScreenState();
}

class _DexOptSchedulerScreenState extends State<DexOptSchedulerScreen> {
  Map<String, dynamic>? _status;
  String? _error;
  bool _busy = false;
  String _cadence = '';
  int _day = 0;
  final TextEditingController _optOut = TextEditingController();
  final FocusNode _optOutFocus = FocusNode();

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _refresh());
  }

  @override
  void dispose() {
    _optOut.dispose();
    _optOutFocus.dispose();
    super.dispose();
  }

  Future<void> _refresh() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final dynamic status =
          await PerAppBackend.instance.command('GET dexopt.status');
      if (!mounted) return;
      final Map<String, dynamic> map = status is Map
          ? Map<String, dynamic>.from(status)
          : <String, dynamic>{};
      final dynamic day = map['day'];
      setState(() {
        _status = map;
        _cadence = (map['cadence'] ?? '').toString();
        _day = day is int ? day : int.tryParse('$day') ?? 0;
        final Object? optOut = map['opt_out'];
        _optOut.text = optOut is List
            ? optOut.whereType<String>().join('\n')
            : '';
      });
    } catch (error) {
      if (mounted) setState(() => _error = '$error');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _save() async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final List<String> optOut = _optOut.text
          .split(RegExp(r'\s+'))
          .where((String entry) => entry.isNotEmpty)
          .toList(growable: false);
      final String payload = jsonEncode(<String, Object>{
        'cadence': _cadence,
        'day': _day,
        'opt_out': optOut,
      });
      await PerAppBackend.instance.command('SET dexopt.schedule $payload');
      await _refresh();
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('DexOpt schedule saved')),
      );
    } catch (error) {
      if (mounted) setState(() => _error = '$error');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  static const List<String> _cadences = <String>['', 'weekly', 'biweekly', 'monthly'];

  static const List<String> _cadenceLabels = <String>[
    'Off',
    'Weekly',
    'Every two weeks',
    'Monthly',
  ];

  static const List<String> _weekdays = <String>[
    'Sunday',
    'Monday',
    'Tuesday',
    'Wednesday',
    'Thursday',
    'Friday',
    'Saturday',
  ];

  String _dayLabel() {
    if (_cadence == 'monthly') {
      return 'Day ${_day.clamp(1, 28)} of the month';
    }
    return _weekdays[_day.clamp(0, 6)];
  }

  Widget _cadenceChips(ColorScheme colors) {
    return Wrap(
      spacing: 8,
      runSpacing: 8,
      children: <Widget>[
        for (int index = 0; index < _cadences.length; index += 1)
          ChoiceChip(
            label: Text(_cadenceLabels[index]),
            selected: _cadence == _cadences[index],
            onSelected: _busy
                ? null
                : (bool selected) {
                    if (!selected) return;
                    setState(() {
                      _cadence = _cadences[index];
                      if (_cadence == 'monthly' && _day > 28) {
                        _day = 1;
                      } else if (_cadence != 'monthly' && _day > 6) {
                        _day = 4;
                      }
                    });
                  },
          ),
      ],
    );
  }

  Widget _dayPicker(ColorScheme colors) {
    final bool monthly = _cadence == 'monthly';
    final int count = monthly ? 28 : 7;
    final List<DropdownMenuEntry<int>> entries = <DropdownMenuEntry<int>>[
      for (int value = monthly ? 1 : 0; value < count + (monthly ? 1 : 0); value += 1)
        DropdownMenuEntry<int>(
          value: value,
          label: monthly ? 'Day $value' : _weekdays[value],
        ),
    ];
    return DropdownMenu<int>(
      enabled: _cadence.isNotEmpty && !_busy,
      initialSelection: _day.clamp(monthly ? 1 : 0, monthly ? 28 : 6),
      label: const Text('Run day'),
      dropdownMenuEntries: entries,
      onSelected: (int? value) {
        if (value != null) setState(() => _day = value);
      },
    );
  }

  Widget _statusCard(ColorScheme colors) {
    final Map<String, dynamic>? status = _status;
    if (status == null) return const SizedBox.shrink();
    final int? lastRun = status['last_run'] is int
        ? status['last_run'] as int
        : int.tryParse('${status['last_run']}');
    final String lastError = '${status['last_error'] ?? ''}';
    return Container(
      padding: const EdgeInsets.all(14),
      decoration: BoxDecoration(
        color: colors.surfaceContainerHighest.withValues(alpha: 0.5),
        borderRadius: BorderRadius.circular(16),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Text(
            'Current schedule',
            style: TextStyle(
              fontSize: 13.5,
              fontWeight: FontWeight.w600,
              color: colors.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 8),
          Text(
            _cadence.isEmpty
                ? 'Off. The compiler never runs on a schedule.'
                : 'Every ${_cadence == 'monthly' ? 'month' : _cadence == 'weekly' ? 'week' : 'two weeks'}, ${_dayLabel()}.',
            style: TextStyle(fontSize: 13.5, color: colors.onSurfaceVariant),
          ),
          if (lastRun != null && lastRun > 0) ...<Widget>[
            const SizedBox(height: 6),
            Text(
              'Last run: ${DateTime.fromMillisecondsSinceEpoch(lastRun * 1000).toLocal()}',
              style: TextStyle(fontSize: 12.5, color: colors.onSurfaceVariant),
            ),
          ],
          if (lastError.isNotEmpty) ...<Widget>[
            const SizedBox(height: 6),
            Text(
              'Last failure: $lastError',
              style: TextStyle(fontSize: 12.5, color: colors.error),
            ),
          ],
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final ColorScheme colors = Theme.of(context).colorScheme;

    return DashScrollPage(
      children: <Widget>[
        DetailHeader(title: 'DexOpt Scheduler', onBack: widget.onBack),
        const SizedBox(height: 4),
        Text(
          'Rebuild app code in the background. Runs only while the screen is off and the device is charging.',
          style: TextStyle(fontSize: 13.5, color: colors.onSurfaceVariant),
        ),
        const SizedBox(height: 14),
        SurfaceCard(child: featureCardHeader(context, 'dexopt.scheduler')),
        const SizedBox(height: 16),
        Text(
          'Schedule',
          style: TextStyle(
            fontSize: 13.5,
            fontWeight: FontWeight.w600,
            color: colors.onSurfaceVariant,
          ),
        ),
        const SizedBox(height: 10),
        _cadenceChips(colors),
        const SizedBox(height: 16),
        _dayPicker(colors),
        const SizedBox(height: 18),
        Text(
          'Excluded packages',
          style: TextStyle(
            fontSize: 13.5,
            fontWeight: FontWeight.w600,
            color: colors.onSurfaceVariant,
          ),
        ),
        const SizedBox(height: 4),
        Text(
          'One package name per line. These apps keep their compiled code as it is.',
          style: TextStyle(fontSize: 12.5, color: colors.onSurfaceVariant),
        ),
        const SizedBox(height: 10),
        TextField(
          controller: _optOut,
          focusNode: _optOutFocus,
          maxLines: 5,
          minLines: 3,
          enabled: !_busy,
          decoration: const InputDecoration(
            hintText: 'com.example.app',
            border: OutlineInputBorder(),
          ),
        ),
        const SizedBox(height: 16),
        Row(
          children: <Widget>[
            Expanded(
              child: IgnorePointer(
                ignoring: _busy,
                child: Opacity(
                  opacity: _busy ? 0.45 : 1.0,
                  child: _HubControlRow(
                    title: 'Save',
                    subtitle: 'Store the schedule in the daemon',
                    icon: Icons.save_rounded,
                    accent: const Color(0xFF74E6C6),
                    onTap: _save,
                  ),
                ),
              ),
            ),
            const SizedBox(width: 10),
            Expanded(
              child: _HubControlRow(
                title: 'Refresh',
                subtitle: 'Read the schedule back',
                icon: Icons.refresh_rounded,
                accent: const Color(0xFF74E6C6),
                onTap: _refresh,
              ),
            ),
          ],
        ),
        const SizedBox(height: 14),
        _statusCard(colors),
        if (_error != null) ...<Widget>[
          const SizedBox(height: 12),
          Text(
            '$_error',
            style: TextStyle(fontSize: 13, color: colors.error),
          ),
        ],
        const SizedBox(height: 24),
      ],
    );
  }
}
