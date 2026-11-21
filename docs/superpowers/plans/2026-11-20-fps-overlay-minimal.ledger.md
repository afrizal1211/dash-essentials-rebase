# FPS overlay plan — execution ledger (inline, executing-plans)

Plan: `docs/superpowers/plans/2026-11-20-fps-overlay-minimal.md`
Spec: `docs/superpowers/specs/2026-11-20-fps-overlay-minimal-design.md`
Branch: `feature-fps-overlay-minimal` in `~/dash-essentials-rebase`

## Status

- [x] Task 1: daemon snapshot sensors — commit `8a9c24c`
- [x] Task 2: one-line chip render — commit `6486fa9`
- [x] Task 3: gate window on selected apps — commit `573aec1`
- [x] Task 4: FFI/JNI/Dart selection carry — commit `e6fa86c` (partial) + `d72413d`, `4b42f5f` (clippy/style fixes)
- [x] Task 5: appearance fields `fpsOverlayAutoOnly`/`fpsOverlayAutoPackages` + setters + test
- [x] Task 6: settings switch + app picker (+ `backup_settings_test.dart` constructor update)
- [x] Task 7: CI test list + local gates (Flutter tests are CI-only)
- [x] Task 8: version bump 1.23.17/12317 + changelog; contract tests pass (push/tag deferred)

## Notes

- Task 4 landed `_pushFpsOverlayAuto` calling `DashBackend.instance.setFpsOverlayAuto`, but
  `DashAppearanceConfig` still lacks the two fields, so the tree does not compile under Flutter.
  Task 5 must add: constructor + defaults initializers, final fields, copyWith params/body,
  toJson keys, fromJson parse, plus the two setters `_setFpsOverlayAutoOnly` /
  `_setFpsOverlayAutoPackages` and the `fps_overlay_auto_test.dart` file (TDD: run test first, watch fail).
- Known unrelated pre-existing failure: `app_runtime::tests::a_conditional_profile_takes_over_when_no_app_is_in_front`
  (device-only, Permission denied on touch node). 229 passed / 1 failed as of the last full daemon run.
- Local gates: `cargo fmt --check` clean; clippy `-D warnings` clean (no `let` chains in `touch_resampler.rs`).
- Release contracts 2026-11-21: `test-release-sync.sh` and `test-root-module-contract.sh` both PASS at 1.23.17/12317.
- Flutter SDK unavailable locally (`flutter`/`dart` not installed); Flutter tests run in CI only. `fps_overlay_auto_test.dart` added to CI list.
- Auto push runs via `_pushFpsOverlayStyle`, which already calls `_pushFpsOverlayAuto(config)` (Task 4), covering all style call sites and boot.
- Backup bug investigated separately; see `docs/superpowers/notes/2026-11-21-backup-missing-file-diagnosis.md`.

## Rulings

- Ruling: stay inline per executing-plans (user said "resume"); plan already chose this mode earlier.
- Ruling: Task 8 release steps (push/tag) deferred until user confirms; pushing is a side effect outside the worktree.
