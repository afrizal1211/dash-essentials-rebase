# Feature Descriptions — Design (small change)

Date: 2026-11-17
Status: awaiting approval

## Intent

Users can see what every Dash Essential feature actually does without
guessing from a bare title like "FPSGO" or leaving the app.

## What was decided with the user

- Presentation: **both** — one always-visible short line under each
  feature card title, plus an info icon that opens a bottom sheet with a
  longer explanation (what it does, when to use it, what happens on
  release / what can go wrong).
- Scope: **every control screen**, one consistent description set.

## Screens and features covered

| Screen (file) | Features described |
|---|---|
| MediaTek Control (`mtk_control.dart`) | Fixed GPU frequency, Thermal governor (gpt), FPSGO, FBT ceiling, Perfmgr, GED KPI, DCS, Fixed DDR OPP |
| Memory DVFS & UFS (`subsystem_clocks.dart`) | Memory DVFS section, UFS/storage section, exact-lock trial, OEM reset |
| Per-App profile (`per_app_controls.dart`) | Profile preferences, Touch & display, Mali GPU, CPU cores, Scheduler clamps, Frame scheduler |
| DexOpt Scheduler (`dexopt_scheduler.dart`) | What background dexopt scheduling does |
| Conditional stack (`conditional_stack.dart`) | What conditionals are |
| Diagnostics (`diagnostics_logs.dart`) | What the log capture collects |

## Approach (picked)

One copy registry + one reusable header widget; screens only reference
feature ids.

- New `ui/flutter/lib/feature_descriptions.dart`: a single
  `const Map<String, FeatureDescription>` keyed by stable ids
  (`mtk.fpsgo`, `clocks.memory`, `perapp.cpu_cores`, …). Each entry has
  `title`, `short` (one sentence), `long` (a short paragraph).
- New small widget (in the same file): renders title row with info icon;
  icon opens the standard modal bottom sheet with the long text. Screens'
  existing `_card` / `_section` helpers take an optional feature id;
  unknown id renders exactly like today (graceful, extendable).
- Copy source: the daemon's own doc comments (`mtk_control.rs`,
  `subsystem_clocks.rs`) + MediaTek GED behavior, written in plain
  English. Reversibility (journal/restore) and freeze risks called out
  where they apply.

## Approaches considered

- Inline long text on every card — rejected: screens become walls of
  text.
- Help/wiki screen listing all features — rejected: forces navigation
  away from the control; user asked for per-feature description.
- Registry + short line + info sheet (picked): one place to keep copy,
  consistent look, zero daemon coupling.

## Non-goals

- No daemon changes, no new permissions, no localization framework
  (English strings, same as the rest of the app).
- No illustrations in the sheets.

## Test

- `test/feature_descriptions_test.dart`: every id referenced by the
  screens exists in the registry; every registry entry has non-empty
  short and long text. Add the file to the Flutter test list in
  `.github/workflows/build-dash-essential-module.yml`.
