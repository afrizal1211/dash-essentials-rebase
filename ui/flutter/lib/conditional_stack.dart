part of 'main.dart';

// Conditional profiles, the layer between the per-app profiles and the global one.
//
// A conditional answers a state of the device instead of an app: while the
// device is charging, while the screen is off. The stack is ordered so the
// answer is never ambiguous: a charging condition outranks the screen state,
// and the global profile is only what the stack falls back to. A foreground
// app keeps its own profile either way.

class ConditionalStackScreen extends StatefulWidget {
  const ConditionalStackScreen({required this.onBack, super.key});

  final VoidCallback onBack;

  @override
  State<ConditionalStackScreen> createState() => _ConditionalStackScreenState();
}

class _ConditionalStackScreenState extends State<ConditionalStackScreen> {
  List<Map<String, dynamic>> _items = <Map<String, dynamic>>[];
  Map<String, dynamic>? _caps;
  bool _liveCharging = false;
  bool _liveScreenOn = false;
  String? _error;
  bool _busy = false;

  bool _editing = false;
  bool _draftEnabled = true;
  final TextEditingController _name = TextEditingController();
  int _chargingChoice = 0; // 0 any, 1 charging, 2 on battery
  int _screenChoice = 0; // 0 any, 1 on, 2 off
  int? _touch;
  int? _gpu;
  int? _refreshRate;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _refresh());
  }

  @override
  void dispose() {
    _name.dispose();
    super.dispose();
  }

  static const List<String> _anyChargingOnBattery = <String>[
    'Any state',
    'While charging',
    'While on battery',
  ];

  static const List<String> _anyOnOff = <String>[
    'Any state',
    'While the screen is on',
    'While the screen is off',
  ];

  Future<void> _refresh() async {
    if (_busy) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final dynamic stack = await PerAppBackend.instance.command('GET conditional.list');
      final dynamic caps = await PerAppBackend.instance.command('GET app.capabilities');
      if (!mounted) return;
      final Map<String, dynamic> map = stack is Map ? Map<String, dynamic>.from(stack) : <String, dynamic>{};
      final List<dynamic> raw = map['conditionals'] is List ? map['conditionals'] as List : <dynamic>[];
      setState(() {
        _items = raw.whereType<Map>().map((Map item) => Map<String, dynamic>.from(item)).toList();
        _liveCharging = map['charging'] is bool ? map['charging'] as bool : false;
        _liveScreenOn = map['screenOn'] is bool ? map['screenOn'] as bool : false;
        _caps = caps is Map ? Map<String, dynamic>.from(caps) : null;
      });
    } catch (error) {
      if (mounted) setState(() => _error = '$error');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Map<String, dynamic> _draftPayload() {
    final Map<String, dynamic> profile = <String, dynamic>{'enabled': _draftEnabled};
    if (_touch != null) profile['touch'] = _touch;
    if (_gpu != null) profile['gpu'] = _gpu;
    if (_refreshRate != null) profile['refresh'] = _refreshRate;
    return <String, dynamic>{
      'name': _name.text.trim(),
      'enabled': _draftEnabled,
      'charging': _chargingChoice == 1 ? true : _chargingChoice == 2 ? false : null,
      'screen_on': _screenChoice == 1 ? true : _screenChoice == 2 ? false : null,
      'profile': profile,
    };
  }

  bool get _draftValid => _name.text.trim().isNotEmpty && (_touch != null || _gpu != null || _refreshRate != null);

  Future<void> _save() async {
    if (!_draftValid) {
      setState(() => _error = 'Give the profile a name and at least one control.');
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await PerAppBackend.instance.command('SET conditional.save ${jsonEncode(_draftPayload())}');
      if (!mounted) return;
      setState(() => _editing = false);
      await _refresh();
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Conditional profile saved')),
      );
    } catch (error) {
      if (mounted) setState(() => _error = '$error');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _delete(String name) async {
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await PerAppBackend.instance.command('ACTION conditional.delete $name');
      await _refresh();
    } catch (error) {
      if (mounted) setState(() => _error = '$error');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  void _edit(Map<String, dynamic>? existing) {
    setState(() {
      _editing = true;
      _error = null;
      _name.text = existing != null ? '${existing['name'] ?? ''}' : '';
      _draftEnabled = existing == null || ((existing['enabled'] as bool?) ?? true);
      final Object? charging = existing?['charging'];
      _chargingChoice = charging is bool ? (charging ? 1 : 2) : 0;
      final Object? screen = existing?['screen_on'];
      _screenChoice = screen is bool ? (screen ? 1 : 2) : 0;
      final Map<String, dynamic>? profile = existing?['profile'] is Map
          ? Map<String, dynamic>.from(existing!['profile'] as Map)
          : null;
      _touch = profile != null && profile['touch'] is int ? profile['touch'] as int : null;
      _gpu = profile != null && profile['gpu'] is int ? profile['gpu'] as int : null;
      _refreshRate = profile != null && profile['refresh'] is int ? profile['refresh'] as int : null;
    });
  }

  String _describe(Map<String, dynamic> item) {
    final List<String> parts = <String>[];
    final Object? charging = item['charging'];
    final Object? screen = item['screen_on'];
    if (charging is bool) parts.add(charging ? 'charging' : 'on battery');
    if (screen is bool) parts.add(screen ? 'screen on' : 'screen off');
    if (parts.isEmpty) parts.add('any state');
    return parts.join(', ');
  }

  Widget _chips(List<String> labels, int selected, ValueChanged<int> onTap) {
    return Wrap(
      spacing: 8,
      runSpacing: 8,
      children: <Widget>[
        for (int index = 0; index < labels.length; index += 1)
          ChoiceChip(
            label: Text(labels[index]),
            selected: selected == index,
            onSelected: _busy
                ? null
                : (bool picked) {
                    if (picked) onTap(index);
                  },
          ),
      ],
    );
  }

  Widget _levelPicker(String label, List<int> levels, int? value, ValueChanged<int?> onChanged) {
    final List<DropdownMenuEntry<int?>> entries = <DropdownMenuEntry<int?>>[
      const DropdownMenuEntry<int?>(value: null, label: 'Keep default'),
      for (final int level in levels) DropdownMenuEntry<int?>(value: level, label: 'Level $level'),
    ];
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: DropdownMenu<int?>(
        enabled: !_busy && levels.isNotEmpty,
        initialSelection: levels.contains(value) ? value : null,
        label: Text(label),
        dropdownMenuEntries: entries,
        onSelected: onChanged,
      ),
    );
  }

  Widget _editorCard(ColorScheme colors) {
    final List<int> touchLevels = _caps?['touch'] is List
        ? (_caps!['touch'] as List).whereType<int>().toList()
        : <int>[];
    final List<int> gpuLevels = _caps?['gpu'] is List
        ? (_caps!['gpu'] as List).whereType<int>().toList()
        : <int>[];
    final List<int> refreshLevels = _caps?['refresh'] is List
        ? (_caps!['refresh'] as List).whereType<int>().toList()
        : <int>[];
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
            _editing ? 'Edit or add a conditional profile' : 'Add a conditional profile',
            style: TextStyle(fontSize: 13.5, fontWeight: FontWeight.w600, color: colors.onSurfaceVariant),
          ),
          const SizedBox(height: 10),
          TextField(
            controller: _name,
            enabled: !_busy,
            decoration: const InputDecoration(
              labelText: 'Name',
              hintText: 'charging-boost',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 12),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('Enabled'),
            value: _draftEnabled,
            onChanged: _busy ? null : (bool value) => setState(() => _draftEnabled = value),
          ),
          const SizedBox(height: 4),
          Text('Charger', style: TextStyle(fontSize: 13.5, fontWeight: FontWeight.w600, color: colors.onSurfaceVariant)),
          const SizedBox(height: 8),
          _chips(_anyChargingOnBattery, _chargingChoice, (int index) => setState(() => _chargingChoice = index)),
          const SizedBox(height: 10),
          Text('Screen', style: TextStyle(fontSize: 13.5, fontWeight: FontWeight.w600, color: colors.onSurfaceVariant)),
          const SizedBox(height: 8),
          _chips(_anyOnOff, _screenChoice, (int index) => setState(() => _screenChoice = index)),
          const SizedBox(height: 14),
          Text('Controls', style: TextStyle(fontSize: 13.5, fontWeight: FontWeight.w600, color: colors.onSurfaceVariant)),
          const SizedBox(height: 6),
          _levelPicker('Touch', touchLevels, _touch, (int? value) => setState(() => _touch = value)),
          _levelPicker('GPU', gpuLevels, _gpu, (int? value) => setState(() => _gpu = value)),
          _levelPicker('Refresh rate', refreshLevels, _refreshRate, (int? value) => setState(() => _refreshRate = value)),
          Row(
            children: <Widget>[
              Expanded(
                child: IgnorePointer(
                  ignoring: _busy,
                  child: Opacity(
                    opacity: _busy ? 0.45 : 1.0,
                    child: _HubControlRow(
                      title: 'Save',
                      subtitle: 'Store the conditional profile',
                      icon: Icons.save_rounded,
                      accent: const Color(0xFFFFB74D),
                      onTap: _save,
                    ),
                  ),
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: _HubControlRow(
                  title: 'Cancel',
                  subtitle: 'Close the editor',
                  icon: Icons.close_rounded,
                  accent: const Color(0xFFFFB74D),
                  onTap: () => setState(() => _editing = false),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _itemCard(ColorScheme colors, Map<String, dynamic> item) {
    final String name = '${item['name'] ?? ''}';
    final bool enabled = item['enabled'] is bool ? item['enabled'] as bool : false;
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
        decoration: BoxDecoration(
          color: colors.surfaceContainerHighest.withValues(alpha: 0.5),
          borderRadius: BorderRadius.circular(16),
        ),
        child: Row(
          children: <Widget>[
            Expanded(
              child: GestureDetector(
                onTap: _busy ? null : () => _edit(item),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: <Widget>[
                    Text(name, style: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600)),
                    const SizedBox(height: 4),
                    Text(_describe(item), style: TextStyle(fontSize: 12.5, color: colors.onSurfaceVariant)),
                    const SizedBox(height: 4),
                    Text(
                      enabled ? 'Enabled' : 'Disabled',
                      style: TextStyle(
                        fontSize: 12.5,
                        color: enabled ? const Color(0xFF74E6C6) : colors.onSurfaceVariant,
                      ),
                    ),
                  ],
                ),
              ),
            ),
            IconButton(
              icon: const Icon(Icons.delete_outline_rounded),
              onPressed: _busy ? null : () => _delete(name),
            ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final ColorScheme colors = Theme.of(context).colorScheme;
    return DashScrollPage(
      children: <Widget>[
        DetailHeader(title: 'Conditional Profiles', onBack: widget.onBack),
        const SizedBox(height: 4),
        Text(
          'Profiles that answer the state of the device instead of an app. Charging outranks the screen state; a foreground app still keeps its own profile.',
          style: TextStyle(fontSize: 13.5, color: colors.onSurfaceVariant),
        ),
        const SizedBox(height: 14),
        SurfaceCard(
          child: featureCardHeader(context, 'conditionals.stack', title: 'How it works'),
        ),
        const SizedBox(height: 14),
        Container(
          padding: const EdgeInsets.all(12),
          decoration: BoxDecoration(
            color: colors.surfaceContainerHighest.withValues(alpha: 0.4),
            borderRadius: BorderRadius.circular(14),
          ),
          child: Text(
            'Now: ${_liveCharging ? 'charging' : 'on battery'} · ${_liveScreenOn ? 'screen on' : 'screen off'}',
            style: TextStyle(fontSize: 12.5, color: colors.onSurfaceVariant),
          ),
        ),
        const SizedBox(height: 14),
        for (final Map<String, dynamic> item in _items) _itemCard(colors, item),
        if (_items.isEmpty)
          Padding(
            padding: const EdgeInsets.only(bottom: 12),
            child: Text('No conditional profiles yet. Add one below.', style: TextStyle(fontSize: 13, color: colors.onSurfaceVariant)),
          ),
        const SizedBox(height: 6),
        if (_editing) _editorCard(colors) else _HubControlRow(
            title: 'Add a conditional profile',
            subtitle: 'Choose the state and the controls',
            icon: Icons.add_rounded,
            accent: const Color(0xFFFFB74D),
            onTap: () => _edit(null),
          ),
        const SizedBox(height: 12),
        _HubControlRow(
          title: 'Refresh',
          subtitle: 'Read the stack and the live state back',
          icon: Icons.refresh_rounded,
          accent: const Color(0xFFFFB74D),
          onTap: _refresh,
        ),
        if (_error != null) ...<Widget>[
          const SizedBox(height: 12),
          Text('$_error', style: TextStyle(fontSize: 13, color: colors.error)),
        ],
        const SizedBox(height: 24),
      ],
    );
  }
}