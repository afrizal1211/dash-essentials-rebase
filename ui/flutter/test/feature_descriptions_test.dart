import 'package:flutter_test/flutter_test.dart';
import 'package:dash_essential_ui/main.dart';

const List<String> kReferencedFeatureIds = <String>[
  'mtk.frequency', 'mtk.thermal_gpt', 'mtk.fpsgo', 'mtk.fbt_ceiling',
  'mtk.perfmgr', 'mtk.ged_kpi', 'mtk.dcs', 'mtk.ddr',
  'clocks.memory', 'clocks.storage',
  'perapp.preferences', 'perapp.touch_display', 'perapp.mali_gpu',
  'perapp.cpu_cores', 'perapp.scheduler_clamps', 'perapp.frame_scheduler',
  'dexopt.scheduler', 'conditionals.stack', 'diagnostics.logs',
];

void main() {
  test('every referenced feature id has a description', () {
    for (final String id in kReferencedFeatureIds) {
      expect(kFeatureDescriptions.containsKey(id), isTrue, reason: id);
    }
  });
  test('every description has non-empty short and long copy', () {
    kFeatureDescriptions.forEach((String id, FeatureDescription entry) {
      expect(entry.short.trim(), isNotEmpty, reason: id);
      expect(entry.long.trim(), isNotEmpty, reason: id);
    });
  });
}
