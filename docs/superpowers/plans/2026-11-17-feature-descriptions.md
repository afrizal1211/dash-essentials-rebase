# Feature Descriptions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every control feature in Dash Essential shows a one-sentence description under its card title and an info icon that opens a sheet with a longer explanation.

**Architecture:** One `part`-of-main registry file maps stable feature ids to `{title, short, long}` copy; one header widget renders title + info icon + short line and opens the standard bottom sheet. Screens pass ids through their existing card/section helpers; unknown ids render exactly like today.

**Tech Stack:** Flutter (part-of-`main.dart` library pattern), existing `showModalBottomSheet` sheet shape, plain Dart const map, `flutter_test`.

**Spec:** `docs/superpowers/specs/2026-11-17-feature-descriptions-design.md`

## Global Constraints

- No daemon changes; UI-only. No new dependencies.
- All UI files are `part of 'main.dart'`; register new file in `main.dart` part list.
- English strings only; match existing app tone (short, factual, warns where freezes are possible).
- Existing code style: trailing commas, `_`-prefixed privates, `const` constructors everywhere possible.
- No Flutter SDK on this machine — `flutter test` runs in CI ("Flutter CI" / module-build workflows). Local verification is code review + `git grep` checks; CI is the gate.

## Review Focus

- Unknown feature id passed by a screen must render the plain title (no crash, no missing-widget exception). Test: Task 1 registry test pins the full id set instead; the widget's fallback branch covers future ids.
- Info sheet must open on a card inside a scrolling page without overflowing on small screens (sheet content must be scroll-safe: `mainAxisSize.min` + single `Text`). Pinned by review, not test.
- Every id referenced in Dart code must exist in `kFeatureDescriptions`. Test: Task 1's `kReferencedFeatureIds` test list must equal the ids wired in Tasks 2–5 — final review greps `featureCardHeader(`, `featureId: '` and diff against the registry.
- Copy accuracy: descriptions must match daemon behavior (journaled restore, mt6991 F/V refusal, freeze risks). Reviewer diff against `runtime/daemon-rust/src/mtk_control.rs` doc comments.

---

### Task 1: Registry, sheet, and header widget

**Files:**
- Create: `ui/flutter/lib/feature_descriptions.dart`
- Modify: `ui/flutter/lib/main.dart` (part list, after `part 'mtk_control.dart';`)
- Test: `ui/flutter/test/feature_descriptions_test.dart`

**Interfaces:**
- Produces: `class FeatureDescription { final String title, short, long; const FeatureDescription(this.title, this.short, this.long); }`, `const Map<String, FeatureDescription> kFeatureDescriptions`, `void showFeatureDescriptionSheet(BuildContext context, String id)`, `Widget featureCardHeader(String id, {String? title})` — all library-level (part of main), consumed by later tasks.

- [ ] **Step 1: Write the failing test**

`ui/flutter/test/feature_descriptions_test.dart`:

```dart
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ui/flutter && flutter test test/feature_descriptions_test.dart` (no local SDK — this step executes in CI after the branch push; locally verify by review only).
Expected: FAIL — `kFeatureDescriptions` is not defined.

- [ ] **Step 3: Implement registry + widget in `ui/flutter/lib/feature_descriptions.dart`**

`part of 'main.dart';` at top. Content, with copy **exactly** as below:

```dart
part of 'main.dart';

class FeatureDescription {
  const FeatureDescription(this.title, this.short, this.long);
  final String title;
  final String short;
  final String long;
}

const Map<String, FeatureDescription> kFeatureDescriptions = <String, FeatureDescription>{
  'mtk.frequency': FeatureDescription(
    'Fixed GPU frequency',
    'Locks the GPU to one frequency from its own OPP table until you release it.',
    'Picks the row matching your frequency from the GPU OPP table and writes it to the driver\'s fix node, so the GPU stops switching between steps. The pin outranks the maximum-frequency limit and is remembered and restored when you release it. The voltage-locked variant is refused by this phone\'s kernel, so use the plain pin. A wrong voltage can freeze the GPU; only a reboot recovers it.',
  ),
  'mtk.thermal_gpt': FeatureDescription(
    'Thermal governor',
    'Turns the kernel\'s GPU thermal limiter (`/sys/kernel/thermal/gpt`) off or on.',
    'The kernel normally throttles the GPU when it heats up. Disabling the governor removes that protection, letting the GPU hold its clocks longer under load at the cost of heat and battery. The original setting is journaled and restored with OEM Reset. Use together with a fixed GPU pin for benchmarking or sustained gaming, and re-enable it afterwards.',
  ),
  'mtk.fpsgo': FeatureDescription(
    'FPSGO',
    'MediaTek\'s frame booster: speeds the GPU and CPU up per frame to hold your target FPS.',
    'FPSGO (Frame Pacing Governor) watches frame times and raises clocks when frames run long, so games keep their target frame rate without locking frequencies. Leave it on for normal gaming; turning it off stops the driver\'s automatic frame boosts and gives you quieter, cooler behavior only if you control clocks manually.',
  ),
  'mtk.fbt_ceiling': FeatureDescription(
    'FBT ceiling',
    'Frame Boost Technology\'s cap on how far FPSGO may raise the GPU per frame.',
    'FBT sets a ceiling table: how high the GPU may climb for a heavy frame. With the ceiling off, FPSGO can push the GPU to its highest steps, improving peak frame rate at the cost of heat and battery. With it on, boosts stay moderate. This complements FPSGO — change them together if you tune frame pacing.',
  ),
  'mtk.perfmgr': FeatureDescription(
    'Perfmgr',
    'MediaTek\'s performance manager: applies per-app boost hints for the foreground app.',
    'Perfmgr requests CPU/GPU headroom when a demanding app takes the foreground, similar to a game-mode daemon. Turning it off removes those automatic hints; clocks then follow whatever else you set. Restorable with OEM Reset.',
  ),
  'mtk.ged_kpi': FeatureDescription(
    'GED KPI',
    'The GPU driver\'s frame-quality metric (KPI) feed used for boost decisions.',
    'GED is MediaTek\'s GPU extension driver; its KPI module tracks per-frame timing so the boost stack can react to real frame times. Leave it on unless you are testing whether a boost source misbehaves — most other GPU boosts depend on this data.',
  ),
  'mtk.dcs': FeatureDescription(
    'DCS',
    'GPU driver display-scaling mode: trades frame count for frequency.',
    'DCS modes tell the GPU how aggressively to drop to lower frequency/frame-count combinations when load allows. The menu lists the modes this kernel offers; pick one or Disable. The selected code is journaled and restored with OEM Reset.',
  ),
  'mtk.ddr': FeatureDescription(
    'Fixed DDR OPP',
    'Forces the memory (DRAM) to one speed step and holds it there.',
    'Writes the DVFS source controller\'s force node so DRAM stops scaling and stays at the chosen operating point — useful for memory benchmarks or stability tests. The node is write-only on some kernels, so the current value cannot be shown. Releasing restores the journaled value or the driver\'s off sentinel. A wrong step can freeze the device; only a reboot recovers it.',
  ),
  'clocks.memory': FeatureDescription(
    'LPDDR5X RAM',
    'Sets the DRAM frequency range, or locks an exact speed with a trial.',
    'Clamp the range the memory governor may use, or run an exact lock: the daemon applies it, watches stability for the trial period, and restores the previous state if you reject it. Everything is journaled — OEM Reset puts the driver\'s own values back.',
  ),
  'clocks.storage': FeatureDescription(
    'UFS 4.0 Storage',
    'Sets the storage controller\'s clock range the same way as RAM.',
    'Same range-and-lock mechanism as the memory section, applied to the UFS storage clock. Useful when testing I/O performance ceilings. Journaled and restorable with OEM Reset.',
  ),
  'perapp.preferences': FeatureDescription(
    'Profile preferences',
    'Switches that decide how this app\'s profile behaves.',
    'Per-app toggles such as boosting while the app is in front. The profile only applies while its app is the foreground app, and the daemon reverts everything the moment you leave the app.',
  ),
  'perapp.touch_display': FeatureDescription(
    'Touch & display',
    'Touch sampling rate and display settings for this app.',
    'Raises the touch panel\'s report rate and adjusts display behavior while the app runs, for lower input latency in games. Original values are restored when the app closes.',
  ),
  'perapp.mali_gpu': FeatureDescription(
    'Mali GPU',
    'GPU frequency bounds, governor and boost for this app.',
    'Sets the Mali GPU\'s minimum/maximum frequency and governor while this app is in front — for example a fixed floor for consistent frame pacing. Restored automatically on app exit.',
  ),
  'perapp.cpu_cores': FeatureDescription(
    'CPU cores',
    'Which CPU cores stay online and at what floor for this app.',
    'Keeps selected cores from sleeping and raises their minimum frequency while the app runs, so it always has warm cores available. Restored automatically when you leave the app.',
  ),
  'perapp.scheduler_clamps': FeatureDescription(
    'Scheduler clamps',
    'Scheduler uclamp minimums for CPU capacity while this app runs.',
    'uclamp raises the scheduler\'s perceived capacity of the app\'s tasks so they land on faster cores sooner. A softer alternative to pinning frequencies. Restored on app exit.',
  ),
  'perapp.frame_scheduler': FeatureDescription(
    'Frame scheduler',
    'FPS metering and frame-time targets for this app.',
    'Watches this app\'s frame times and can steer the boost stack toward its frame-rate target. Combine with FPSGO for game frame pacing. Restored on app exit.',
  ),
  'dexopt.scheduler': FeatureDescription(
    'DexOpt Scheduler',
    'Runs Android\'s background AOT app compilation when you choose, not when the OS feels like it.',
    'DEX optimization compiles apps to native code; stock devices do it at random times and steal performance from what you are doing. This scheduler runs the same work under your control (time window, cadence, per-package opt-out) so compilation happens while the phone idles instead of while you use it.',
  ),
  'conditionals.stack': FeatureDescription(
    'Conditional stack',
    'Rules that switch profiles automatically, like "when charging" or "when this app is in front".',
    'Conditionals sit on top of per-app profiles: when a rule matches, its profile takes over until the rule stops matching. The daemon keeps exactly one active profile and reverts the previous one on every switch.',
  ),
  'diagnostics.logs': FeatureDescription(
    'Diagnostics',
    'Captures daemon and system logs into one bundle for bug reports.',
    'Collects the daemon\'s own log, kernel messages and selected system dumps into exportable files. Export the bundle before filing a bug — it shows which node failed and why.',
  ),
};

void showFeatureDescriptionSheet(BuildContext context, String id) {
  final FeatureDescription? entry = kFeatureDescriptions[id];
  if (entry == null) return;
  showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    builder: (BuildContext context) {
      final ColorScheme colors = Theme.of(context).colorScheme;
      return SafeArea(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(22, 8, 22, 22),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: <Widget>[
              Text(
                entry.title,
                style: const TextStyle(fontSize: 17, fontWeight: FontWeight.w800),
              ),
              const SizedBox(height: 10),
              Text(
                entry.short,
                style: TextStyle(fontSize: 13.5, color: colors.onSurfaceVariant),
              ),
              const SizedBox(height: 14),
              Text(entry.long, style: const TextStyle(fontSize: 14, height: 1.45)),
            ],
          ),
        ),
      );
    },
  );
}

/// Title row with an info sheet, plus the always-visible short line.
/// Unknown ids fall back to the plain title so screens never crash on a
/// registry gap.
Widget featureCardHeader(String id, {String? title}) {
  final FeatureDescription? entry = kFeatureDescriptions[id];
  if (entry == null) return Text(title ?? id, style: const TextStyle(fontWeight: FontWeight.w800));
  return Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: <Widget>[
      Row(
        children: <Widget>[
          Expanded(
            child: Text(
              title ?? entry.title,
              style: const TextStyle(fontWeight: FontWeight.w800),
            ),
          ),
          IconButton(
            visualDensity: VisualDensity.compact,
            icon: const Icon(Icons.info_outline_rounded, size: 19),
            color: Theme.of(globalNavigatorContext).colorScheme.onSurfaceVariant,
            tooltip: 'About ${entry.title}',
            onPressed: () => showFeatureDescriptionSheet(globalNavigatorContext, id),
          ),
        ],
      ),
      const SizedBox(height: 4),
      Text(
        entry.short,
        style: TextStyle(fontSize: 11.5, color: Theme.of(globalNavigatorContext).colorScheme.onSurfaceVariant),
      ),
    ],
  );
}
```

`globalNavigatorContext` does not exist — **do not use it.** Use the build-context of the card: change the signature to `Widget featureCardHeader(BuildContext context, String id, {String? title})` and use `context` in both `Theme.of(context)` and the `onPressed` callback. Adjust the code above accordingly; the copy map and sheet are unchanged.

- [ ] **Step 4: Register the part file**

In `ui/flutter/lib/main.dart`, after `part 'mtk_control.dart';` add `part 'feature_descriptions.dart';`.

- [ ] **Step 5: Run test to verify it passes**

Run: `flutter test test/feature_descriptions_test.dart` (CI on branch push).
Expected: PASS (both tests).

- [ ] **Step 6: Commit**

```bash
git add ui/flutter/lib/feature_descriptions.dart ui/flutter/lib/main.dart ui/flutter/test/feature_descriptions_test.dart
git commit -m "feat(ui): feature description registry, sheet and card header"
```

### Task 2: Wire MediaTek Control screen

**Files:**
- Modify: `ui/flutter/lib/mtk_control.dart` (`_card` helper ~line 114, call sites 163, 230, 251, 275, 313)

**Interfaces:**
- Consumes: `featureCardHeader(context, id)` from Task 1.
- Produces: `_card({required BuildContext context, required String featureId, String? title, required Widget child})` used by every card on this screen.

- [ ] **Step 1: Extend the `_card` helper**

`_card` gains `BuildContext context` + `String featureId` + optional `String? title` and renders `featureCardHeader(context, featureId, title: title)` in place of the plain `Text(title, ...)`. Update the five call sites to pass ids: `mtk.frequency` (Fixed frequency), `mtk.thermal_gpt` (Thermal governor), `mtk.fpsgo` / `mtk.fbt_ceiling` / `mtk.perfmgr` / `mtk.ged_kpi` (switch loop — pass the loop's `name` mapped through the same switch the card title already uses), `mtk.dcs`, `mtk.ddr`. The switch-loop call site may keep `title: null` so registry titles win.

- [ ] **Step 2: Verify by grep**

Run: `git grep -n "mtk\." ui/flutter/lib/mtk_control.dart | wc -l` — expect ≥ 8 id references; run `git grep -c "featureCardHeader" ui/flutter/lib/mtk_control.dart` — expect 1 (helper) + 5 (call sites).

- [ ] **Step 3: Commit**

```bash
git add ui/flutter/lib/mtk_control.dart
git commit -m "feat(ui): descriptions on MediaTek Control cards"
```

### Task 3: Wire Memory DVFS & UFS screen

**Files:**
- Modify: `ui/flutter/lib/subsystem_clocks.dart` (`_SubsystemFrequencyCard` build, title Row ~line 403; `_section` stays untouched — its HeroCard already has a subtitle)

**Interfaces:**
- Consumes: `featureCardHeader(context, id)`, `kFeatureDescriptions` from Task 1.

- [ ] **Step 1: Add the description inside the frequency card**

`_SubsystemFrequencyCard` gains a required `String featureId` field; in its build, between the title `Row` and the first control, insert `featureCardHeader(context, widget.featureId, title: null)` followed by `const SizedBox(height: 8)`. Pass `featureId: 'clocks.memory'` from the `memory` `_section(...)` call and `featureId: 'clocks.storage'` from the `storage` one (thread the id through `_section` as a new parameter).

- [ ] **Step 2: Verify by grep**

Run: `git grep -n "clocks\." ui/flutter/lib/subsystem_clocks.dart` — expect the two ids.

- [ ] **Step 3: Commit**

```bash
git add ui/flutter/lib/subsystem_clocks.dart
git commit -m "feat(ui): descriptions on Memory DVFS & UFS cards"
```

### Task 4: Wire Per-App profile sections

**Files:**
- Modify: `ui/flutter/lib/per_app_controls.dart` (`_PerAppSectionTitle` widget, ~line 1020 area; section call sites at titles 'Profile preferences', 'Touch & display', 'Mali GPU', 'CPU cores', 'Scheduler clamps', 'Frame scheduler')

**Interfaces:**
- Consumes: `featureCardHeader(context, id)` from Task 1.

- [ ] **Step 1: Extend `_PerAppSectionTitle`**

Add optional `String? featureId`. When set, render the info `IconButton` beside the existing title row and the registry's `short` line under it (reuse `featureCardHeader`'s sheet via `showFeatureDescriptionSheet(context, id)`; keep the existing icon + accent look). Add `featureId` to the six call sites: `perapp.preferences`, `perapp.touch_display`, `perapp.mali_gpu`, `perapp.cpu_cores`, `perapp.scheduler_clamps`, `perapp.frame_scheduler`.

- [ ] **Step 2: Verify by grep**

Run: `git grep -c "perapp\." ui/flutter/lib/per_app_controls.dart` — expect 6.

- [ ] **Step 3: Commit**

```bash
git add ui/flutter/lib/per_app_controls.dart
git commit -m "feat(ui): descriptions on per-app profile sections"
```

### Task 5: Wire DexOpt, Conditional stack, Diagnostics

**Files:**
- Modify: `ui/flutter/lib/dexopt_scheduler.dart` (after `DetailHeader` ~line 232), `ui/flutter/lib/conditional_stack.dart` (screen header), `ui/flutter/lib/diagnostics_logs.dart` (screen header)

**Interfaces:**
- Consumes: `featureCardHeader(context, id)` from Task 1.

- [ ] **Step 1: Insert one description card per screen**

On each of the three screens, directly under the existing header/subtitle block, add:

```dart
SurfaceCard(child: featureCardHeader(context, '<id>'))
```

with ids `dexopt.scheduler`, `conditionals.stack`, `diagnostics.logs`. Match each screen's existing spacing (`SizedBox(height: 14)` around cards). If a screen's header block already carries the muted subtitle text, keep it — the card adds the info icon and longer copy.

- [ ] **Step 2: Verify by grep**

Run: `git grep -n "dexopt.scheduler\|conditionals.stack\|diagnostics.logs" ui/flutter/lib/` — expect one hit per file.

- [ ] **Step 3: Commit**

```bash
git add ui/flutter/lib/dexopt_scheduler.dart ui/flutter/lib/conditional_stack.dart ui/flutter/lib/diagnostics_logs.dart
git commit -m "feat(ui): descriptions on DexOpt, conditionals and diagnostics"
```

### Task 6: CI test list + branch verification

**Files:**
- Modify: `.github/workflows/build-dash-essential-module.yml` (Flutter tests step, `run:` list ~line 203)

**Interfaces:**
- Consumes: `ui/flutter/test/feature_descriptions_test.dart` from Task 1.

- [ ] **Step 1: Add the new test to CI**

Append `test/feature_descriptions_test.dart \` to the `flutter test \` file list.

- [ ] **Step 2: Grep-audit id wiring**

Run: `git grep -oh "'[a-z]*\.[a-z_]*'" ui/flutter/lib/mtk_control.dart ui/flutter/lib/subsystem_clocks.dart ui/flutter/lib/per_app_controls.dart ui/flutter/lib/dexopt_scheduler.dart ui/flutter/lib/conditional_stack.dart ui/flutter/lib/diagnostics_logs.dart | sort -u` — every id that looks like a feature id must exist in `kFeatureDescriptions` (Task 1's test list is the contract).

- [ ] **Step 3: Commit, push, open PR**

```bash
git add .github/workflows/build-dash-essential-module.yml
git commit -m "ci: run the feature description registry test"
git push -u origin feature-descriptions
gh pr create --fill
```

- [ ] **Step 4: Verify CI green**

Run: `gh pr checks --watch` (or `gh run list` until completed).
Expected: Flutter CI and module build both `success`, `feature_descriptions_test.dart` included in the Flutter test step log.

---

## Self-review notes

- Spec coverage: registry + sheet + header (Task 1), all six screens (Tasks 2–5), CI test (Tasks 1+6). Non-goals respected (no daemon edits).
- Copy lives in Task 1's code block verbatim — the deliverable content is pinned, not left open.
- `featureCardHeader` signature decided as `(BuildContext, String id, {String? title})` — the plan flags the context requirement explicitly since the draft used a nonexistent global.
- Task 3 keeps `_section`'s HeroCard subtitle (existing pattern) and adds the registry copy inside the frequency card — no duplicated text on one card.
