import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:dash_essential_ui/main.dart';

Map<String, dynamic> status({bool gedKpi = false, bool ddr = false}) =>
    <String, dynamic>{
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
      'ddr': ddr
          ? <String, dynamic>{
              'present': true,
              'readable': false,
              'opps': <Map<String, dynamic>>[
                <String, dynamic>{'index': 44, 'vcore': 760000, 'mbps': 8528},
                <String, dynamic>{'index': 86, 'vcore': 685000, 'mbps': 3094},
              ],
            }
          : <String, dynamic>{'present': false, 'readable': false, 'opps': <int>[]},
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

  testWidgets('no frequency is selected before the user picks one', (tester) async {
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
    // The highest OPP must never look preselected: one stray Apply tap
    // would pin it.
    expect(find.text('1612000 kHz'), findsNothing);
    expect(find.text('Select frequency'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('apply sends the pin for the picked frequency', (tester) async {
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
    await tester.tap(find.byKey(const ValueKey('mtk-gpu-frequency')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('338000 kHz').last);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Apply'));
    await tester.pumpAndSettle();
    expect(commands, contains('ACTION mtk.gpu.pin 338000'));
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('the ddr picker sends the combined-table index', (tester) async {
    final commands = <String>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MtkControlScreen(
            onBack: () {},
            exchange: (command) async {
              commands.add(command);
              return status(ddr: true);
            },
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Fixed DDR OPP'), findsOneWidget);
    await Scrollable.ensureVisible(
      tester.element(find.byKey(const ValueKey('mtk-ddr-opp'))),
      alignment: 0.5,
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('mtk-ddr-opp')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('OPP 86 · 3094 Mbps').last);
    await tester.pumpAndSettle();
    expect(commands, contains('ACTION mtk.ddr.force_opp 86'));
    await tester.pumpWidget(const SizedBox());
  });
}
