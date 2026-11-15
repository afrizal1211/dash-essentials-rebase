import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:dash_essential_ui/main.dart';

void main() {
  group('buildBackupBundle', () {
    test('merges ui and hardware under a dash header', () {
      final Map<String, dynamic> bundle = buildBackupBundle(
        hardware: <String, dynamic>{
          'state': <String>['charging=65'],
        },
        appearance: <String, dynamic>{'customPath': ''},
        interaction: <String, dynamic>{'motionSpeed': 1.0},
        version: '1.23.12',
      );
      expect(bundle['schema'], 1);
      expect(
        bundle['app'],
        <String, dynamic>{'version': '1.23.12', 'platform': 'dash'},
      );
      expect(bundle['ui']['appearance'], <String, dynamic>{'customPath': ''});
      expect(bundle['ui']['interaction'], <String, dynamic>{'motionSpeed': 1.0});
      expect(bundle['hardware']['state'], <String>['charging=65']);
    });
  });

  group('parseDashImport', () {
    final Map<String, dynamic> valid = <String, dynamic>{
      'schema': 1,
      'app': <String, dynamic>{'platform': 'dash'},
      'ui': <String, dynamic>{'appearance': <String, dynamic>{}},
      'hardware': <String, dynamic>{'state': <String>[]},
    };

    test('accepts a dash bundle', () {
      final ({String? error, DashImportRequest? request}) parsed =
          parseDashImport(jsonEncode(valid));
      expect(parsed.error, isNull);
      expect(parsed.request, isNotNull);
      expect(parsed.request!.hardware['state'], <String>[]);
    });

    test('refuses a foreign platform before any daemon traffic', () {
      final Map<String, dynamic> foreign = Map<String, dynamic>.from(valid);
      (foreign['app'] as Map<String, dynamic>)['platform'] = 'rodin';
      final ({String? error, DashImportRequest? request}) parsed =
          parseDashImport(jsonEncode(foreign));
      expect(parsed.error, 'This backup was made for a different platform.');
      expect(parsed.request, isNull);
    });

    test('refuses an unsupported schema', () {
      final Map<String, dynamic> future = Map<String, dynamic>.from(valid);
      future['schema'] = 2;
      expect(parseDashImport(jsonEncode(future)).error, isNotNull);
    });

    test('refuses a bundle without a hardware block', () {
      final Map<String, dynamic> incomplete = Map<String, dynamic>.from(valid);
      incomplete.remove('hardware');
      expect(parseDashImport(jsonEncode(incomplete)).error, isNotNull);
    });

    test('refuses something that is not a bundle', () {
      expect(parseDashImport('not json').error, isNotNull);
    });
  });

  group('normalizeCustomPath', () {
    test('keeps a file that still exists', () {
      final File file = File(
        '${Directory.systemTemp.path}/dash-backup-test-${DateTime.now().microsecondsSinceEpoch}.png',
      );
      file.writeAsBytesSync(<int>[1, 2, 3]);
      addTearDown(file.deleteSync);
      expect(
        normalizeCustomPath(file.path, contentExists: (_) => false),
        file.path,
      );
    });

    test('prunes a path that no longer resolves', () {
      expect(
        normalizeCustomPath('/no/such/file.png', contentExists: (_) => true),
        '',
      );
      expect(
        normalizeCustomPath(
          'content://media/external/file/1',
          contentExists: (_) => false,
        ),
        '',
      );
    });

    test('keeps a content uri that still answers', () {
      expect(
        normalizeCustomPath(
          'content://media/external/file/1',
          contentExists: (_) => true,
        ),
        'content://media/external/file/1',
      );
    });

    test('leaves an empty path alone', () {
      expect(normalizeCustomPath('', contentExists: (_) => false), '');
    });
  });

  group('DashInteractionSettings.replace', () {
    test('sets the imported values and bumps the revision', () async {
      await DashInteractionSettings.load();
      final double previousSpeed = DashInteractionSettings.motionSpeed;
      final bool previousHaptics = DashInteractionSettings.hapticsEnabled;
      final int previousRevision = DashInteractionSettings.revision.value;
      addTearDown(() async {
        await DashInteractionSettings.replace(
          motionSpeed: previousSpeed,
          haptics: previousHaptics,
        );
      });
      await DashInteractionSettings.replace(motionSpeed: 1.2, haptics: false);
      expect(DashInteractionSettings.motionSpeed, 1.2);
      expect(DashInteractionSettings.hapticsEnabled, false);
      expect(DashInteractionSettings.revision.value, previousRevision + 1);
    });
  });

  testWidgets('settings offers backup transfer', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SettingsScreen(
            config: const DashAppearanceConfig.defaults(),
            onThemePreferenceChanged: (_) {},
            onBackgroundStyleChanged: (_) {},
            onCustomBackgroundSelected: (_) {},
            onBlurChanged: (_) {},
            onBlurChangeEnd: (_) {},
            onHorizontalControlActive: (_) {},
            onResetAppearance: () {},
            onAccentChanged: (_) {},
            onCardRadiusChanged: (_) {},
            onCardStyleChanged: (_) {},
            onHeroGlowChanged: (_) {},
            onCoreVisualizerStyleChanged: (_) {},
            onFpsMeterChanged: (_) {},
            onFpsOverlayChanged: (_) {},
            onFpsOverlayScaleChanged: (_) {},
            onFpsOverlayOpacityChanged: (_) {},
            onFpsOverlayRadiusChanged: (_) {},
            onFpsOverlayPerGamePositionChanged: (_) {},
            onFpsRecordChanged: (_) {},
            onFpsAutoChanged: (_) {},
            onOpenFpsHistory: () {},
            onStatusNotificationChanged: (_) {},
            onImportAppearance: (_) {},
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    // Section 8 sits far down a lazily built scroll view, so reach it the way
    // a user does instead of asserting on an offscreen child.
    await tester.scrollUntilVisible(
      find.text('Export settings backup'),
      400,
    );
    await tester.pumpAndSettle();
    expect(find.text('BACKUP & TRANSFER'), findsOneWidget);
    expect(find.text('Export settings backup'), findsOneWidget);
    expect(find.text('Import settings backup'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
  });
}
