import 'package:dash_essential_ui/main.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('appearance remembers the overlay auto-show settings', () {
    const DashAppearanceConfig defaults = DashAppearanceConfig.defaults();
    expect(defaults.fpsOverlayAutoOnly, isFalse);
    expect(defaults.fpsOverlayAutoPackages, isEmpty);

    final DashAppearanceConfig restored = DashAppearanceConfig.fromJson(
      defaults
          .copyWith(
            fpsOverlayAutoOnly: true,
            fpsOverlayAutoPackages: const <String>[
              'com.game.one',
              'com.game.two',
            ],
          )
          .toJson(),
    );
    expect(restored.fpsOverlayAutoOnly, isTrue);
    expect(restored.fpsOverlayAutoPackages, <String>[
      'com.game.one',
      'com.game.two',
    ]);
  });

  test('a file written before auto-show reads as off and empty', () {
    const DashAppearanceConfig defaults = DashAppearanceConfig.defaults();
    final Map<String, Object?> legacy = defaults.toJson()
      ..remove('fpsOverlayAutoOnly')
      ..remove('fpsOverlayAutoPackages');
    final DashAppearanceConfig old = DashAppearanceConfig.fromJson(legacy);
    expect(old.fpsOverlayAutoOnly, isFalse);
    expect(old.fpsOverlayAutoPackages, isEmpty);
  });

  test('non-string package entries are dropped', () {
    const DashAppearanceConfig defaults = DashAppearanceConfig.defaults();
    final Map<String, Object?> raw = defaults.toJson()
      ..['fpsOverlayAutoPackages'] = <Object?>[1, 'com.game', null];
    expect(
      DashAppearanceConfig.fromJson(raw).fpsOverlayAutoPackages,
      <String>['com.game'],
    );
  });
}
