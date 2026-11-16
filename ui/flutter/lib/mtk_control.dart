part of 'main.dart';

class MtkControlScreen extends StatefulWidget {
  const MtkControlScreen({required this.onBack, this.exchange, super.key});

  final VoidCallback onBack;
  final Future<dynamic> Function(String)? exchange;

  @override
  State<MtkControlScreen> createState() => _MtkControlScreenState();
}

class _MtkControlScreenState extends State<MtkControlScreen>
    with WidgetsBindingObserver {
  Map<String, dynamic>? _state;
  String? _error;
  String? _busy;
  int? _gpuKhz;
  int? _gpuVolt;
  int? _ddrOpp;
  bool _reading = false;
  bool _visible = true;
  Timer? _refresh;
  Completer<void>? _readCompletion;

  Future<dynamic> _exchange(String command) =>
      (widget.exchange ?? PerAppBackend.instance.command)(command);

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    unawaited(_read());
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _visible = state == AppLifecycleState.resumed;
    _refresh?.cancel();
    if (_visible) unawaited(_read());
  }

  void _schedule() {
    _refresh?.cancel();
    if (mounted && _visible) {
      _refresh = Timer(const Duration(seconds: 2), () => unawaited(_read()));
    }
  }

  Future<void> _read() async {
    if (_reading || _busy != null) {
      _schedule();
      return;
    }
    _reading = true;
    final completion = Completer<void>();
    _readCompletion = completion;
    try {
      final data = await _exchange('GET mtk.control');
      if (mounted) {
        setState(() {
          _state = Map<String, dynamic>.from(data as Map);
          _error = null;
          final gpu = _state!['gpu'] as Map?;
          final pin = gpu?['pin'] as Map?;
          final pinnedKhz = pin?['khz'] as int?;
          // Only a driver-reported pin selects a frequency. Never default to
          // the table head: one stray Apply tap would pin the highest OPP.
          if (_gpuKhz == null && pinnedKhz != null) _gpuKhz = pinnedKhz;
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      _reading = false;
      completion.complete();
      _schedule();
    }
  }

  Future<bool> _action(String command, String key) async {
    if (_busy != null) return false;
    _refresh?.cancel();
    setState(() => _busy = key);
    try {
      await _readCompletion?.future;
      if (!mounted) return false;
      final data = await _exchange(command);
      if (mounted) {
        setState(() {
          _state = Map<String, dynamic>.from(data as Map);
          _error = null;
        });
      }
      DashHaptics.confirm();
      return true;
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
      DashHaptics.reject();
      return false;
    } finally {
      if (mounted) setState(() => _busy = null);
      _schedule();
    }
  }

  @override
  void dispose() {
    _refresh?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  Widget _card({
    required BuildContext context,
    required String featureId,
    String? title,
    required Widget child,
  }) => SurfaceCard(
    padding: const EdgeInsets.all(14),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: <Widget>[
        featureCardHeader(context, featureId, title: title),
        const SizedBox(height: 8),
        child,
      ],
    ),
  );

  Widget _oemReset(String name) => TextButton.icon(
    onPressed: _busy == null ? () => _action('ACTION mtk.reset $name', name) : null,
    icon: const Icon(Icons.restart_alt_rounded, size: 17),
    label: const Text('OEM Reset'),
  );

  @override
  Widget build(BuildContext context) {
    final data = _state;
    final gpu = (data?['gpu'] as Map?)?.cast<String, dynamic>() ?? {};
    final switches = (data?['switches'] as Map?)?.cast<String, dynamic>() ?? {};
    final dcs = (data?['dcs'] as Map?)?.cast<String, dynamic>() ?? {};
    final ddr = (data?['ddr'] as Map?)?.cast<String, dynamic>() ?? {};
    final opps = (gpu['opps'] as List?) ?? const <dynamic>[];
    final List<Widget> cards = <Widget>[];

    if (data?['supported'] != true) {
      if (data != null) {
        cards.add(SurfaceCard(
          child: Text(data['reason'] as String? ?? 'MediaTek GPU control is unavailable.'),
        ));
      }
    } else {
      if (opps.isNotEmpty) {
        Map? selectedRow;
        for (final value in opps) {
          final row = value as Map;
          if (row['khz'] == _gpuKhz) {
            selectedRow = row;
            break;
          }
        }
        final volt = selectedRow?['volt'] as int?;
        final frequencies = opps
            .map((row) => (row as Map)['khz'] as int)
            .toSet()
            .toList();
        cards.add(_card(
          context: context,
          featureId: 'mtk.frequency',
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              const Text('Fixed frequency outranks the maximum frequency and cannot be combined with it.'),
              DropdownButton<int>(
                key: const ValueKey('mtk-gpu-frequency'),
                isExpanded: true,
                value: _gpuKhz,
                hint: const Text('Select frequency'),
                items: frequencies.map((khz) {
                  return DropdownMenuItem<int>(value: khz, child: Text('$khz kHz'));
                }).toList(),
                onChanged: _busy == null
                    ? (value) => setState(() {
                        _gpuKhz = value;
                        _gpuVolt = null;
                      })
                    : null,
              ),
              if (_gpuKhz != null && volt != null)
                DropdownButton<int?>(
                  key: const ValueKey('mtk-gpu-voltage'),
                  isExpanded: true,
                  value: _gpuVolt,
                  items: <DropdownMenuItem<int?>>[
                    const DropdownMenuItem<int?>(value: null, child: Text('Driver voltage')),
                    DropdownMenuItem<int?>(value: volt, child: Text('$volt')),
                  ],
                  onChanged: _busy == null
                      ? (value) => setState(() => _gpuVolt = value)
                      : null,
                ),
              Wrap(
                alignment: WrapAlignment.end,
                spacing: 8,
                children: <Widget>[
                  FilledButton.tonalIcon(
                    onPressed: _busy != null || _gpuKhz == null ? null : () => _action(
                      _gpuVolt == null
                          ? 'ACTION mtk.gpu.pin $_gpuKhz'
                          : 'ACTION mtk.gpu.pin_volt $_gpuKhz $_gpuVolt',
                      'gpu-pin',
                    ),
                    icon: const Icon(Icons.check_rounded),
                    label: const Text('Apply'),
                  ),
                  TextButton.icon(
                    onPressed: _busy == null ? () => _action('ACTION mtk.gpu.pin 0', 'gpu-pin') : null,
                    icon: const Icon(Icons.restart_alt_rounded),
                    label: const Text('Release'),
                  ),
                ],
              ),
              const Text(
                'A wrong voltage can freeze the GPU. Only a reboot recovers it.',
                style: TextStyle(fontSize: 11),
              ),
              _oemReset('gpu'),
            ],
          ),
        ));
      }

      final thermal = (gpu['thermal_gpt'] as Map?)?.cast<String, dynamic>() ?? {};
      if (thermal['present'] == true) {
        cards.add(_card(
          context: context,
          featureId: 'mtk.thermal_gpt',
          child: Column(children: <Widget>[
            SwitchListTile(
              key: const ValueKey('mtk-thermal-gpt'),
              contentPadding: EdgeInsets.zero,
              value: thermal['disabled'] != true,
              title: const Text('Thermal governor enabled'),
              onChanged: _busy != null ? null : (enabled) => _action(
                enabled ? 'ACTION mtk.reset thermal_gpt' : 'ACTION mtk.thermal_gpt disable',
                'thermal-gpt',
              ),
            ),
            _oemReset('thermal_gpt'),
          ]),
        ));
      }

      for (final name in <String>['fpsgo', 'fbt_ceiling', 'perfmgr', 'ged_kpi']) {
        final item = (switches[name] as Map?)?.cast<String, dynamic>();
        if (item == null || item['present'] != true) continue;
        cards.add(_card(
          context: context,
          featureId: 'mtk.$name',
          title: switch (name) {
            'fpsgo' => 'FPSGO',
            'fbt_ceiling' => 'FBT ceiling',
            'perfmgr' => 'Perfmgr',
            _ => 'GED KPI',
          },
          child: Column(children: <Widget>[
            SwitchListTile(
              key: ValueKey('mtk-switch-$name'),
              contentPadding: EdgeInsets.zero,
              value: item['on'] == true,
              title: const Text('Enabled'),
              onChanged: _busy != null ? null : (value) => _action(
                'ACTION mtk.switch $name ${value ? 1 : 0}', name),
            ),
            _oemReset(name),
          ]),
        ));
      }

      if (dcs['present'] == true) {
        final menu = (dcs['menu'] as List?) ?? const <dynamic>[];
        final code = dcs['code'] as int?;
        cards.add(_card(
          context: context,
          featureId: 'mtk.dcs',
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: <Widget>[
            if (code == null) const Text('Enabled (code not reported)'),
            DropdownButton<int>(
              key: const ValueKey('mtk-dcs'),
              isExpanded: true,
              value: code != null &&
                      <int>[
                        0,
                        ...menu.map((row) => (row as List)[0] as int),
                      ].contains(code)
                  ? code
                  : null,
              hint: const Text('Enabled (code not reported)'),
              items: <DropdownMenuItem<int>>[
                const DropdownMenuItem<int>(value: 0, child: Text('Disable')),
                ...menu.map((row) {
                  final entry = row as List;
                  return DropdownMenuItem<int>(
                    value: entry[0] as int,
                    child: Text(entry[1] as String),
                  );
                }),
              ],
              onChanged: _busy != null
                  ? null
                  : (value) {
                      if (value != null) _action('ACTION mtk.dcs $value', 'dcs');
                    },
            ),
            _oemReset('dcs'),
          ]),
        ));
      }

      if (ddr['present'] == true) {
        final ddrOpps = (ddr['opps'] as List?) ?? const <dynamic>[];
        cards.add(_card(
          context: context,
          featureId: 'mtk.ddr',
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: <Widget>[
            const Text('Some kernels expose this node write-only, so the current value cannot be shown. A wrong value can freeze the device; only a reboot recovers it.'),
            if (ddrOpps.isNotEmpty)
              DropdownButton<int>(
                key: const ValueKey('mtk-ddr-opp'),
                isExpanded: true,
                value: _ddrOpp,
                hint: const Text('Select DDR OPP'),
                items: List<DropdownMenuItem<int>>.generate(
                  ddrOpps.length,
                  (position) {
                    final row = ddrOpps[position] as Map;
                    final index = row['index'] as int;
                    return DropdownMenuItem<int>(
                      value: index,
                      child: Text('OPP $index · ${row['mbps']} Mbps'),
                    );
                  },
                ),
                onChanged: _busy != null ? null : (index) {
                  if (index == null) return;
                  setState(() => _ddrOpp = index);
                  _action('ACTION mtk.ddr.force_opp $index', 'ddr');
                },
              ),
            Wrap(alignment: WrapAlignment.end, children: <Widget>[
              TextButton.icon(
                onPressed: _busy == null ? () => _action('ACTION mtk.ddr.force_opp off', 'ddr') : null,
                icon: const Icon(Icons.restart_alt_rounded),
                label: const Text('Release'),
              ),
              _oemReset('ddr'),
            ]),
          ]),
        ));
      }
    }

    return DashScrollPage(children: <Widget>[
      DetailHeader(title: 'MediaTek Control', onBack: widget.onBack),
      const SizedBox(height: 4),
      Text('GPU pin, voltage, thermal and DDR', style: TextStyle(
        fontSize: 13.5,
        color: Theme.of(context).colorScheme.onSurfaceVariant,
      )),
      const SizedBox(height: 14),
      if (_error != null) SurfaceCard(child: Text(_error!)),
      if (data == null && _error == null)
        const Padding(padding: EdgeInsets.all(24), child: Center(child: CircularProgressIndicator())),
      ...cards,
      TextButton.icon(
        onPressed: _busy == null ? _read : null,
        icon: const Icon(Icons.refresh_rounded),
        label: const Text('Refresh readings'),
      ),
    ]);
  }
}
