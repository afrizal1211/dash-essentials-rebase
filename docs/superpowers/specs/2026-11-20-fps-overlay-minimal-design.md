# FPS overlay: minimal layout and auto-show for selected apps — design

Date: 2026-11-20
Status: approved by the project owner before implementation.

## Intent

The FPS overlay chip shows a lot today (FPS, temperature, GPU %, GPU MHz,
CPU MHz, package name) inside a bordered pill. Compared with Scene's
monitor it reads as busy. The owner wants Scene's minimalist feel with a
richer metric set: **FPS, CPU, GPU, RAM and battery**, plus the overlay
should **appear on its own only for chosen games** instead of staying on
everywhere.

## Goals

- Chip content becomes one compact block: FPS large, then a single muted
  line `CPU 42% · GPU 55% · RAM 46% · BAT 85%`.
- The daemon's overlay snapshot gains CPU load percent, RAM used/total,
  and battery level — all already sampled by `telemetry.rs` for FPS
  session recording.
- Temperature and package name leave the chip (session/history screens
  keep them).
- New appearance options: "Only selected apps" switch plus a multi-select
  app picker. When armed, the chip is visible only while the foreground
  package is in the chosen set.
- Selection and the switch survive Dash being closed: the service reads
  them from its own prefs and works without the app running.

## Non-goals

- No FPS graph inside the chip.
- No change to recording, session history, Prism export, or the in-app
  `DashFpsMeter`.
- No new permissions.
- No per-app overlay styling.

## Anti-goals

- The chip must never show a fake zero: a sensor the ROM does not expose
  is omitted, an absent FPS reading renders as a dash.
- No daemon-driven start/stop of the service: foreground detection stays
  inside the already-running service tick.
- No implicit reuse of per-app profile selections; the overlay selection
  is its own list.

## Constraints

- `DashAppearanceConfig` JSON is persisted and importable; new fields must
  default cleanly on old files (`false` / empty list).
- FFI and JNI surfaces are packed-int today. Passing a package list needs
  a UTF-8 buffer; the existing malloc/free pattern from the backup API
  (`_backupCopyOut`) is reused.
- All UI files stay `part of 'main.dart'` where they already are; the
  Java side stays a plain platform service compiled into the APK.
- No Flutter SDK on this machine; CI (`flutter test`, lints, module build)
  is the compile gate.

## Approaches considered

1. **Gate visibility inside `FpsOverlayService` (chosen).** The service
   already receives the foreground package every 500 ms from the daemon
   snapshot. It compares against the persisted selection and attaches or
   removes the window only when the answer changes. No new IPC, no extra
   wakeups, works while Dash is closed.
2. Daemon-driven start/stop of the service. Rejected: adds IPC surface
   and a failure mode where the overlay silently never appears, for no
   battery win at a 2 Hz poll of sysfs reads.
3. Accessibility / broadcast listeners. Rejected: extra permission,
   fragile across ROMs.

## Design

### Data (daemon)

`foreground_fps.rs::overlay_json()` additionally emits, when non-zero:
`cpuLoad` (percent, aggregate), `ramUsedMb`, `ramTotalMb`,
`batteryLevel` (percent). The values come from `telemetry::sample()`
(nothing new is probed; the FPS session recorder already reads them).
`gpuUtil`, `gpuMHz`, `cpuMHz`, `tempDeciC`, `package` and the frame
fields keep their current keys so old consumers and the diagnostics view
do not change. `overlay_json` is split so the sensor merge takes a
`Telemetry` value, keeping it unit-testable without sysfs.

### Chip (Java)

`FpsOverlayService.render()` builds one `SpannableStringBuilder`:

- `118 FPS` — 1.35×, bold, accent.
- on the same line, 0.68× and `0xFFA8B8C4`: `CPU 42% · GPU 55% · RAM 46% · BAT 85%`.

Missing metrics are omitted from the muted run; RAM shows used percent
(`ramUsedMb / ramTotalMb`). The label set is fixed (`CPU`, `GPU`, `RAM`,
`BAT`); no MHz values anymore. The pill background, accent, opacity and
radius stay exactly as configured today. Width stays capped at
`260dp × scale`, so narrow screens may wrap the muted line — accepted.

### Auto-show

- New service prefs: `auto_only` (bool), `auto_packages` (JSON array of
  package names).
- On each sample tick after parsing the snapshot, if `auto_only` is set:
  visible ⇔ foreground package ∈ `auto_packages`. `followForeground()`
  keeps its per-game position memory; the window is detached, not
  destroyed, and re-attached with the remembered position. Changing
  foreground between two selected apps keeps the window up and moves it
  per position memory as today.
- The service keeps running (notification stays) while hidden — that is
  what makes the auto mode reliable; the notification is the off switch.
- Manual `setEnabled` behaviour is unchanged: switch off stops the
  service; switch on arms it. Auto mode only narrows visibility.

### Settings (Flutter)

In the existing FPS overlay appearance section:

- switch **Only selected apps** (default off), disabled visually until
  the overlay switch is on;
- row **Choose apps** opening a multi-select list of launchable apps
  (reusing the installed-app source and row style from the per-app
  profile picker), disabled when the switch is off;
- selection is pushed through a new FFI entry
  `dash_host_fps_overlay_auto(buf, len, autoOnly)` → Java
  `configFpsOverlayAuto(String packagesCsv, int autoOnly)` →
  `FpsOverlayService.applyAutoConfig`, which persists and live-applies.

`DashAppearanceConfig` gains `fpsOverlayAutoOnly` (bool, default false)
and `fpsOverlayAutoPackages` (list, default empty), included in
`toJson`/`fromJson`. Both paths that already push overlay style
(`_loadAppearance`, `applyAppearance`, `_setFpsOverlay`) push the auto
config too.

## Failure behaviour

- Daemon away / overlay permission revoked: unchanged from today's code.
- Empty selection with auto mode on: chip never shows; settings show a
  hint line "No apps selected" under the picker.
- Foreground unknown (launcher/idle): hidden under auto mode.

## Testing

- Rust: unit test for the overlay sensor merge from a `Telemetry`
  struct (keys present/absent), alongside the existing typed-reading
  test.
- Flutter: `DashAppearanceConfig` JSON round-trip with the new fields,
  defaults on legacy JSON; added to the CI `flutter test` list.
- Java: compile-only coverage via the module build; no unit harness
  exists for the service today.
- Manual check on device after release: chip content, auto-show on a
  selected game, hide on launcher, selection survives Dash being closed.

## Left to the builder

- Exact app-picker source to reuse and where the picker screen lives.
- JNI/JString handling detail for the new setter.
- Whether the "No apps selected" hint is inline text or a snackbar.

## Anchors in the current tree

- Settings section: `ui/flutter/lib/main.dart` — the "FPS overlay" row
  around line 3139, inside the appearance screen's card list.
- Style push: `main.dart:_pushFpsOverlayStyle` → `DashBackend.setFpsOverlayConfig`
  → `rust/platform_bridge.rs:dash_host_fps_overlay_config` →
  `DashActivity.configFpsOverlay` → `FpsOverlayService.applyConfig`.
- Chip rendering: `FpsOverlayService.render` (line ~491) and
  `applyStyleTo` (~line 434).
- Sensor source: `runtime/daemon-rust/src/telemetry.rs::sample`
  (`cpu_load`, `ram_used_mb`, `battery_level`); overlay payload built in
  `foreground_fps.rs::overlay_json` (~line 539).
- App list source for pickers: `per_app_controls.dart` app selection
  flow (reuse the backend call it uses, not its widgets).

