import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:dash_essential_ui/main.dart';

Map<String, dynamic> status({bool gedKpi = false}) => <String, dynamic>{
  'supported': true,
  'reason': null,
  'soc': 'mt6991',
  'gpu': <String, dynamic>{
    'opps': <Map<String, dynamic>>[
      <String, dynamic>{'index': 0, 'khz': 1612000, 'volt': 79500},
      <String, dynamic>{'index': 51, 'khz': 338000, 'volt': 49375},
    ],
    'pin': <String, dynamic>{'index': null, 'khz': null},
    'thermal_gpt': <String, dynamic>{'present': true, 'disabled': false},
  },
  'switches': <String, dynamic>{
    'fpsgo': <String, dynamic>{'present': true, 'on': true},
    'fbt_ceiling': <String, dynamic>{'present': true, 'on': false},
    'perfmgr': <String, dynamic>{'present': true, 'on': false},
    'ged_kpi': <String, dynamic>{'present': gedKpi, 'on': null},
  },
  'dcs': <String, dynamic>{'present': true, 'code': 0, 'menu': <List<dynamic>>[]},
  'ddr': <String, dynamic>{'present': false, 'readable': false, 'opps': <int>[]},
};

void main() {
  testWidgets('absent controls render no card', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MtkControlScreen(
            onBack: () {},
            exchange: (command) async => status(),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('GED KPI'), findsNothing);
    expect(find.text('Fixed DDR OPP'), findsNothing);
    expect(find.text('Fixed frequency'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('a switch sends its action command', (tester) async {
    final commands = <String>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MtkControlScreen(
            onBack: () {},
            exchange: (command) async {
              commands.add(command);
              return status();
            },
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final finder = find.byKey(const ValueKey('mtk-switch-fpsgo'));
    await Scrollable.ensureVisible(tester.element(finder), alignment: 0.5);
    await tester.pumpAndSettle();
    await tester.tap(finder);
    await tester.pumpAndSettle();
    expect(commands, contains('ACTION mtk.switch fpsgo 0'));
    await tester.pumpWidget(const SizedBox());
  });
}
