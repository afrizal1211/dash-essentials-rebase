# Minimal FPS overlay with auto-show for selected apps — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the FPS overlay chip into one compact line — FPS plus CPU, GPU, RAM and battery — and show it only while a selected game is in front.

**Architecture:** The daemon's `GET app.fps` snapshot gains the sensor fields it already samples for FPS recording. The Java overlay service renders the new line and gates its window attach/detach on the foreground package, using a selection pushed once through a new FFI/JNI call and persisted in the service's own prefs. Flutter adds two appearance fields and the settings controls that push them.

**Tech Stack:** Rust (daemon `telemetry`, `foreground_fps`), Java (platform service, no new DEX classes), Flutter (Dart, no new packages), JNI FFI bridge.

**Spec:** `docs/superpowers/specs/2026-11-20-fps-overlay-minimal-design.md` (approved).

## Global Constraints

- No new Flutter dependencies, no new permissions, no new platform DEX classes.
- A sensor the ROM does not expose is omitted from the payload and from the chip; never print a fake zero.
- FPS keys (`fps`, `min`, `low1`, `dropped`, `valid`, `node`, `package`) keep their names and meaning.
- `DashAppearanceConfig` JSON stays backward-compatible: old files read `false` / empty list.
- Overlay auto-show must work with Dash closed: the service reads its own prefs, not the UI.
- All UI text is English.
- `cargo fmt --check`, `cargo clippy -p dash-essential-daemon --all-targets -- -D warnings`, and the existing release-sync contract tests must stay clean.
- Work on the existing branch `feature-fps-overlay-minimal` in `~/dash-essentials-rebase`.
- No Flutter or Android SDK on this machine: Rust runs locally; Flutter/Java compile in CI.

## Review Focus

- `overlay_json` must stay testable without sysfs — the sensor merge takes a `Telemetry` value, not a live sample.
- RAM percent divides only when `ramTotalMb > 0`; battery and RAM are shown only when the snapshot has the key.
- Auto-show moves no window when the answer does not change, and never restarts the sampling thread: detach the view, keep the 500 ms poll alive.
- An empty selection with auto mode on hides the chip (no fallback to "show everywhere").
- `setEnabled(false)` still stops the service; auto mode only narrows visibility.
- A service that is already hidden must re-attach at the game's remembered position, not at the default corner.
- The new FFI buffer path must free the malloc'd copy on every return, including the null-buffer and zero-package cases.

---

### Task 1: Emit the extra sensors in the daemon overlay snapshot

**Files:**
- Modify: `runtime/daemon-rust/src/telemetry.rs` (struct + `sample`)
- Modify: `runtime/daemon-rust/src/foreground_fps.rs` (`overlay_json`, delete `cpu_mhz`, delete `battery_temp_deci_c`)
- Modify: `runtime/daemon-rust/src/app_runtime.rs` (~line 2259, snapshot key test)

**Interfaces:**
- Consumes: `telemetry::sample(package)` — already returns `cpu_load`, `gpu_load`, `gpu_mhz`, `cpu_mhz`, `battery_temp_dc`, `battery_level`, `ram_used_mb`.
- Produces:
  - `Telemetry.ram_total_mb: i32` (GiB-scale total in MiB, `0` when `/proc/meminfo` is unreadable)
  - `fn overlay_json_with(reading: &FpsReading, package: Option<&str>, telemetry: Telemetry) -> serde_json::Value`
  - Snapshot keys added when greater than zero: `cpuLoad`, `ramUsedMb`, `ramTotalMb`, `batteryLevel`; existing `gpuUtil`, `gpuMHz`, `cpuMHz`, `tempDeciC` now come from the same `Telemetry` value.

- [ ] **Step 1: Write the failing tests**

In `foreground_fps.rs`, extend the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn the_overlay_carries_every_sensor_the_sample_has() {
    let telemetry = Telemetry {
        cpu_load: 42,
        gpu_load: 55,
        gpu_mhz: 890,
        cpu_mhz: 2200,
        battery_temp_dc: 430,
        battery_level: 85,
        ram_used_mb: 6144,
        ram_total_mb: 12288,
        ..Default::default()
    };
    let value = overlay_json_with(&FpsReading::unavailable(), Some("com.game"), telemetry);
    assert_eq!(value["package"], "com.game");
    assert_eq!(value["cpuLoad"], 42);
    assert_eq!(value["gpuUtil"], 55);
    assert_eq!(value["gpuMHz"], 890);
    assert_eq!(value["cpuMHz"], 2200);
    assert_eq!(value["tempDeciC"], 430);
    assert_eq!(value["ramUsedMb"], 6144);
    assert_eq!(value["ramTotalMb"], 12288);
    assert_eq!(value["batteryLevel"], 85);
    // The frame reading keys still travel with the sensors.
    assert!(value.get("valid").is_some());
    assert!(value.get("fps").is_some());
}

#[test]
fn a_sensor_that_did_not_answer_is_omitted_not_zero() {
    let value = overlay_json_with(
        &FpsReading::unavailable(),
        None,
        Telemetry::default(),
    );
    for key in [
        "gpuUtil",
        "gpuMHz",
        "cpuMHz",
        "tempDeciC",
        "cpuLoad",
        "ramUsedMb",
        "ramTotalMb",
        "batteryLevel",
    ] {
        assert!(value.get(key).is_none(), "{key} must be omitted");
    }
    assert!(value.get("package").is_some_and(serde_json::Value::is_null));
}
```

- [ ] **Step 2: Run the tests and watch them fail**

Run: `cargo test -p dash-essential-daemon foreground_fps`
Expected: compile error — `Telemetry` has no `ram_total_mb`, `overlay_json_with` does not exist.

- [ ] **Step 3: Add the total-RAM field**

In `telemetry.rs`, add to `Telemetry` after `ram_used_mb`:

```rust
    pub ram_total_mb: i32,
```

In `sample`, after `ram_used_mb`:

```rust
        ram_total_mb: memory.total_mb,
```

No other constructor needs editing: `profile_manager.rs` builds `Telemetry` with `..Default::default()`.

- [ ] **Step 4: Rewrite the overlay merge**

In `foreground_fps.rs`, add the import near the other module imports:

```rust
use crate::telemetry::Telemetry;
```

Replace the body of `overlay_json` with:

```rust
pub(super) fn overlay_json() -> serde_json::Value {
    let (reading, package) = read_with_package();
    // No package argument: the chip shows the device RAM share, and a package
    // would run `pidof` on the IPC thread for a value nobody reads.
    let telemetry = crate::telemetry::sample(None);
    overlay_json_with(&reading, package.as_deref(), telemetry)
}

/// Merges one sample's sensors into the frame reading. Taking the sample as a
/// value keeps the key contract testable without sysfs.
fn overlay_json_with(
    reading: &FpsReading,
    package: Option<&str>,
    telemetry: Telemetry,
) -> serde_json::Value {
    let mut object = match serde_json::to_value(reading) {
        Ok(serde_json::Value::Object(object)) => object,
        _ => serde_json::Map::new(),
    };
    object.insert(
        "package".into(),
        package.map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    // Zero is "the node said zero" for loads, but a missing node also reads as
    // zero. The sensors are omitted in that case; `valid` already carries the
    // frame-reading signal.
    for (key, value) in [
        ("gpuUtil", telemetry.gpu_load),
        ("gpuMHz", telemetry.gpu_mhz),
        ("cpuMHz", telemetry.cpu_mhz),
        ("tempDeciC", telemetry.battery_temp_dc),
        ("cpuLoad", telemetry.cpu_load),
        ("ramUsedMb", telemetry.ram_used_mb),
        ("ramTotalMb", telemetry.ram_total_mb),
        ("batteryLevel", telemetry.battery_level),
    ] {
        if value > 0 {
            object.insert(key.into(), serde_json::json!(value));
        }
    }
    serde_json::Value::Object(object)
}
```

Delete `fn cpu_mhz()` and `fn battery_temp_deci_c()` — the telemetry sample replaces both and leaving them would trip `dead_code`.

- [ ] **Step 5: Widen the command-level key test**

In `app_runtime.rs`, change the fixed key list to:

```rust
        for key in [
            "gpuUtil",
            "gpuMHz",
            "cpuMHz",
            "tempDeciC",
            "cpuLoad",
            "ramUsedMb",
            "ramTotalMb",
            "batteryLevel",
        ] {
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p dash-essential-daemon foreground_fps && cargo test -p dash-essential-daemon foreground_fps_answers_with_a_typed_reading`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add runtime/daemon-rust/src/telemetry.rs runtime/daemon-rust/src/foreground_fps.rs runtime/daemon-rust/src/app_runtime.rs
git commit -m "feat(daemon): emit cpu, ram and battery in the overlay snapshot"
```

---

### Task 2: Draw the one-line chip

**Files:**
- Modify: `android/platform/io/github/neeschal/dashessential/FpsOverlayService.java` (`render`)
- Modify: `docs/superpowers/specs/2026-11-20-fps-overlay-minimal-design.md` (prefs format note, only if Task 3 stores CSV)

**Interfaces:**
- Consumes: snapshot keys from Task 1.
- Produces: `render(TextView view, String payload)` draws `118 FPS · CPU 42% · GPU 55% · RAM 46% · BAT 85%`; headline keeps 1.35× bold accent, the metric segment is one 0.68× `0xFFA8B8C4` run.

- [ ] **Step 1: Replace the parse block**

Replace the locals and parse body in `render` with:

```java
        long scaled = 0;
        int cpuLoad = 0;
        int gpu = 0;
        int ramUsedMb = 0;
        int ramTotalMb = 0;
        int batteryLevel = 0;
        boolean hasCpuLoad = false;
        boolean hasGpu = false;
        boolean hasRam = false;
        boolean hasBattery = false;
        try {
            JSONObject snapshot = new JSONObject(payload == null ? "{}" : payload);
            scaled = Math.round(snapshot.optDouble("fps", 0.0) * 100.0);
            hasCpuLoad = snapshot.has("cpuLoad");
            hasGpu = snapshot.has("gpuUtil");
            hasRam = snapshot.has("ramUsedMb") && snapshot.has("ramTotalMb");
            hasBattery = snapshot.has("batteryLevel");
            cpuLoad = snapshot.optInt("cpuLoad", 0);
            gpu = snapshot.optInt("gpuUtil", 0);
            ramUsedMb = snapshot.optInt("ramUsedMb", 0);
            ramTotalMb = snapshot.optInt("ramTotalMb", 0);
            batteryLevel = snapshot.optInt("batteryLevel", 0);
        } catch (JSONException malformed) {
            // A daemon that is away answers "{}"; the chip shows a dash.
        }
```

- [ ] **Step 2: Replace the text building**

After the headline spans, delete the temperature, MHz and package blocks. Append one metric segment:

```java
        StringBuilder metrics = new StringBuilder();
        if (hasCpuLoad) {
            metrics.append("CPU ").append(cpuLoad).append('%');
        }
        if (hasGpu) {
            if (metrics.length() > 0) metrics.append(" \u00b7 ");
            metrics.append("GPU ").append(gpu).append('%');
        }
        if (hasRam && ramTotalMb > 0) {
            if (metrics.length() > 0) metrics.append(" \u00b7 ");
            metrics.append("RAM ")
                .append(Math.round(ramUsedMb * 100f / ramTotalMb))
                .append('%');
        }
        if (hasBattery) {
            if (metrics.length() > 0) metrics.append(" \u00b7 ");
            metrics.append("BAT ").append(batteryLevel).append('%');
        }
        if (metrics.length() > 0) {
            text.append(" \u00b7 ");
            int detailStart = text.length();
            text.append(metrics);
            text.setSpan(new RelativeSizeSpan(0.68f), detailStart, text.length(), SPAN);
            text.setSpan(
                new ForegroundColorSpan(0xFFA8B8C4), detailStart, text.length(), SPAN);
        }
```

The headline stays:

```java
        int start = text.length();
        text.append(scaled > 0
            ? Math.round(scaled / 100.0) + " FPS" : "FPS \u2014");
```

`applyStyleTo` is unchanged: pill fill, stroke, padding, radius and the `260dp × scale` width cap stay, so a narrow screen wraps the metric segment.

- [ ] **Step 3: Check for leftovers**

Run: `grep -n "tempDeciC\|gpuMhz\|cpuMhz\|pkg" android/platform/io/github/neeschal/dashessential/FpsOverlayService.java`
Expected: no hits inside `render`; `packageOf` and `followForeground` still use the package string.

- [ ] **Step 4: Commit**

```bash
git add android/platform/io/github/neeschal/dashessential/FpsOverlayService.java
git commit -m "feat(overlay): render fps, cpu, gpu, ram and battery on one line"
```

---

### Task 3: Gate the window on the selected apps

**Files:**
- Modify: `android/platform/io/github/neeschal/dashessential/FpsOverlayService.java`
- Modify: `docs/superpowers/specs/2026-11-20-fps-overlay-minimal-design.md` (change "JSON array of package names" to comma-separated, matching the wire format)

**Interfaces:**
- Consumes: `packageOf(payload)` per 500 ms tick.
- Produces:
  - prefs `auto_only` (boolean), `auto_packages` (comma-separated package names)
  - `static void applyAutoConfig(Context context, String packagesCsv, boolean autoOnly)`
  - `private void attachWindow(String pkg)`, `private void detachWindow()`

- [ ] **Step 1: Add prefs keys and fields**

```java
    private static final String KEY_AUTO_ONLY = "auto_only";
    private static final String KEY_AUTO_PACKAGES = "auto_packages";
```

```java
    /** Whether the chip is limited to the selected apps. */
    private boolean autoOnly;
    private final java.util.Set<String> autoPackages = new java.util.HashSet<>();
    /** The package the last tick saw in front. */
    private String livePackage = "";
```

- [ ] **Step 2: Persist and push the config**

Add beside `applyConfig`:

```java
    /**
     * Stores the auto-show gate. The selection is a comma-separated package
     * list: package names cannot contain commas, and the service and the wire
     * format then use the same string.
     */
    static void applyAutoConfig(Context context, String packagesCsv, boolean autoOnly) {
        java.util.LinkedHashSet<String> clean = new java.util.LinkedHashSet<>();
        if (packagesCsv != null) {
            for (String pkg : packagesCsv.split(",")) {
                if (!pkg.trim().isEmpty()) clean.add(pkg.trim());
            }
        }
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit()
            .putBoolean(KEY_AUTO_ONLY, autoOnly)
            .putString(KEY_AUTO_PACKAGES, String.join(",", clean))
            .apply();
        FpsOverlayService live = current;
        if (live != null) live.main.post(live::reloadStyle);
    }
```

Extend `loadConfig()`:

```java
        autoOnly = p.getBoolean(KEY_AUTO_ONLY, false);
        autoPackages.clear();
        for (String pkg : p.getString(KEY_AUTO_PACKAGES, "").split(",")) {
            if (!pkg.trim().isEmpty()) autoPackages.add(pkg.trim());
        }
```

- [ ] **Step 3: Split attach from the sampling thread**

Replace `show()` and `hide()` with:

```java
    private void show() {
        if (!canDraw(this) || windowManager == null) {
            stopSelf();
            return;
        }
        if (!autoOnly) {
            attachWindow(prefs().getString(KEY_POS_PACKAGE, ""));
        }
        if (worker == null) {
            worker = new HandlerThread("fps-overlay");
            worker.start();
            workerHandler = new Handler(worker.getLooper());
            workerHandler.post(sample);
        }
    }

    private void attachWindow(String pkg) {
        if (attached || windowManager == null) return;
        if (chip == null) {
            chip = buildChip();
            installDragListener();
        }
        if (!pkg.equals(positionedPackage)) {
            positionedPackage = pkg;
            prefs().edit().putString(KEY_POS_PACKAGE, pkg).apply();
        }
        params = buildParams();
        try {
            windowManager.addView(chip, params);
            attached = true;
        } catch (RuntimeException refused) {
            stopSelf();
        }
    }

    /** Removes only the window; the sampler keeps answering 500 ms ticks. */
    private void detachWindow() {
        if (!attached || chip == null || windowManager == null) return;
        try {
            windowManager.removeView(chip);
        } catch (IllegalArgumentException alreadyGone) {
            // The window manager can drop the view first on process death.
        }
        attached = false;
        params = null;
    }

    private void hide() {
        if (worker != null) {
            worker.quitSafely();
            worker = null;
            workerHandler = null;
        }
        detachWindow();
    }
```

In `buildParams()`, delete the line that reloads the positioned package from prefs (`positionedPackage = prefs().getString(KEY_POS_PACKAGE, "");`); the field is set by `show()`, `attachWindow` and `followForeground` now. Keep the saved-position lookup:

```java
        int[] saved = (!perGame || positionedPackage.isEmpty())
            ? null : perGamePositions.get(positionedPackage);
```

- [ ] **Step 4: Apply visibility on every tick**

Replace the sample's main-thread block:

```java
            final String payload = nativeSnapshot();
            main.post(() -> {
                livePackage = packageOf(payload);
                if (shouldShow(livePackage)) {
                    attachWindow(livePackage);
                    if (chip != null) render(chip, payload);
                    if (attached) followForeground(livePackage);
                } else {
                    detachWindow();
                }
            });
```

Add:

```java
    /** Auto mode narrows visibility; an empty selection hides the chip. */
    private boolean shouldShow(String pkg) {
        return !autoOnly || (pkg != null
            && !pkg.isEmpty() && autoPackages.contains(pkg));
    }
```

`reloadStyle()` already calls `loadConfig()` and `applyStyle()`; append a visibility pass so a settings change takes effect without waiting for the next foreground change:

```java
    private void reloadStyle() {
        loadConfig();
        applyStyle();
        if (shouldShow(livePackage)) attachWindow(livePackage);
        else detachWindow();
    }
```

`followForeground` stays as is: when attached and the package changed, it moves the chip to that game's remembered position.

- [ ] **Step 5: Compile-check by inspection**

Run: `grep -n "show()\|attachWindow\|detachWindow\|hide()" android/platform/io/github/neeschal/dashessential/FpsOverlayService.java`
Expected: `show()` is called only from `onStartCommand`; `hide()` only from `onDestroy`; no other caller starts or stops the worker.

- [ ] **Step 6: Commit**

```bash
git add android/platform/io/github/neeschal/dashessential/FpsOverlayService.java docs/superpowers/specs/2026-11-20-fps-overlay-minimal-design.md
git commit -m "feat(overlay): show the chip only for selected foreground apps"
```

---

### Task 4: Carry the selection across FFI and JNI

**Files:**
- Modify: `runtime/host-rust/src/platform_bridge.rs` (new extern fn near `dash_host_fps_overlay_config`)
- Modify: `android/platform/io/github/neeschal/dashessential/DashActivity.java` (new sink near `configFpsOverlay`)
- Modify: `ui/flutter/lib/backend/dash_backend.dart` (typedefs, field, lookup, method)

**Interfaces:**
- Consumes: `FpsOverlayService.applyAutoConfig` from Task 3.
- Produces:
  - Rust `pub extern "C" fn dash_host_fps_overlay_auto(packages: *const u8, len: i32, auto_only: i32) -> i32`
  - Java `public boolean configFpsOverlayAuto(String packagesCsv, int autoOnly)`
  - Dart `bool setFpsOverlayAuto({required bool autoOnly, required List<String> packages})`

- [ ] **Step 1: Add the JNI sink**

In `DashActivity.java`, under `configFpsOverlay`:

```java
    /** Pushes the auto-show gate from native FFI. */
    public boolean configFpsOverlayAuto(String packagesCsv, int autoOnly) {
        FpsOverlayService.applyAutoConfig(this, packagesCsv, autoOnly != 0);
        return true;
    }
```

- [ ] **Step 2: Add the Rust extern fn**

Follow the existing `dash_host_backup_share` string pattern:

```rust
/// Pushes the overlay auto-show gate (comma-separated packages, flag) into the
/// service prefs. A UTF-8 buffer keeps the selection opaque to this bridge.
#[unsafe(no_mangle)]
pub extern "C" fn dash_host_fps_overlay_auto(
    packages: *const u8,
    len: i32,
    auto_only: i32,
) -> i32 {
    if len < 0 || len as usize > MAX_BACKUP_LEN || (len > 0 && packages.is_null()) {
        return 0;
    }
    let text = if len == 0 {
        std::borrow::Cow::Borrowed("")
    } else {
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(packages, len as usize) })
    };
    i32::from(
        with_activity(|env, activity| {
            let csv = env.new_string(text.as_ref())?;
            env.call_method(
                activity,
                "configFpsOverlayAuto",
                "(Ljava/lang/String;I)Z",
                &[JValue::Object(&csv), JValue::Int(auto_only)],
            )?
            .z()
        })
        .unwrap_or(false),
    )
}
```

- [ ] **Step 3: Add the Dart binding**

Near the other overlay typedefs:

```dart
typedef _OverlayAutoNative = ffi.Int32 Function(
  ffi.Pointer<ffi.Uint8>,
  ffi.Int32,
  ffi.Int32,
);
typedef _OverlayAutoDart = int Function(ffi.Pointer<ffi.Uint8>, int, int);
```

Field beside `_fpsOverlayConfigNative`:

```dart
  _OverlayAutoDart? _fpsOverlayAutoNative;
```

Lookup beside the config symbol:

```dart
      if (lib.providesSymbol('dash_host_fps_overlay_auto')) {
        _fpsOverlayAutoNative = lib
            .lookupFunction<_OverlayAutoNative, _OverlayAutoDart>(
              'dash_host_fps_overlay_auto',
            );
      }
```

Method after `setFpsOverlayConfig`:

```dart
  /// Persists the auto-show gate for the overlay service. The empty case
  /// passes a null buffer with length zero; the native side reads it as "".
  bool setFpsOverlayAuto({
    required bool autoOnly,
    required List<String> packages,
  }) {
    if (!_started) return false;
    final Uint8List bytes = Uint8List.fromList(utf8.encode(packages.join(',')));
    final ffi.Pointer<ffi.Uint8> buffer =
        bytes.isEmpty ? ffi.nullptr : _backupCopyOut(bytes);
    try {
      return _fpsOverlayAutoNative?.call(
            buffer,
            bytes.length,
            autoOnly ? 1 : 0,
          ) ==
          1;
    } finally {
      if (buffer != ffi.nullptr) _backupRelease(buffer);
    }
  }
```

- [ ] **Step 4: Verify symbol names line up**

Run:

```bash
grep -rn "dash_host_fps_overlay_auto" runtime/host-rust/src ui/flutter/lib android/platform | cat
```

Expected: three hits — Rust definition, Dart lookup string, Dart method (lookup string repeated in lookup call).

- [ ] **Step 5: Commit**

```bash
git add runtime/host-rust/src/platform_bridge.rs android/platform/io/github/neeschal/dashessential/DashActivity.java ui/flutter/lib/backend/dash_backend.dart
git commit -m "feat(overlay): push the auto-show selection through ffi"
```

---

### Task 5: Store the auto-show settings in the appearance config

**Files:**
- Modify: `ui/flutter/lib/main.dart` (`DashAppearanceConfig`, state pushes)
- Create: `ui/flutter/test/fps_overlay_auto_test.dart`

**Interfaces:**
- Consumes: `DashBackend.setFpsOverlayAuto` from Task 4.
- Produces:
  - `DashAppearanceConfig.fpsOverlayAutoOnly` (bool, default `false`)
  - `DashAppearanceConfig.fpsOverlayAutoPackages` (List<String>, default `const []`)
  - `_DashShellState._pushFpsOverlayAuto(DashAppearanceConfig)`, `_setFpsOverlayAutoOnly(bool)`, `_setFpsOverlayAutoPackages(List<String>)`

- [ ] **Step 1: Write the failing test**

`ui/flutter/test/fps_overlay_auto_test.dart`:

```dart
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
    expect(DashAppearanceConfig.fromJson(raw).fpsOverlayAutoPackages, <String>[
      'com.game',
    ]);
  });
}
```

- [ ] **Step 2: Add the fields**

In `DashAppearanceConfig`, constructor and defaults after `fpsOverlayPerGamePosition`:

```dart
    this.fpsOverlayAutoOnly = false,
    this.fpsOverlayAutoPackages = const <String>[],
```

```dart
      fpsOverlayAutoOnly = false,
      fpsOverlayAutoPackages = const <String>[],
```

Fields after `fpsOverlayPerGamePosition`:

```dart
  /// When true the overlay is visible only while a selected app is in front.
  final bool fpsOverlayAutoOnly;
  final List<String> fpsOverlayAutoPackages;
```

- [ ] **Step 3: Extend `copyWith`, `toJson`, `fromJson`**

`copyWith` parameters:

```dart
    bool? fpsOverlayAutoOnly,
    List<String>? fpsOverlayAutoPackages,
```

and body:

```dart
      fpsOverlayAutoOnly: fpsOverlayAutoOnly ?? this.fpsOverlayAutoOnly,
      fpsOverlayAutoPackages: fpsOverlayAutoPackages ??
          this.fpsOverlayAutoPackages,
```

`toJson` after `fpsOverlayPerGamePosition`:

```dart
      'fpsOverlayAutoOnly': fpsOverlayAutoOnly,
      'fpsOverlayAutoPackages': fpsOverlayAutoPackages,
```

`fromJson`, next to the per-game line:

```dart
    final bool fpsOverlayAutoOnly =
        raw['fpsOverlayAutoOnly'] as bool? ?? false;
    final List<String> fpsOverlayAutoPackages =
        (raw['fpsOverlayAutoPackages'] as List?)
            ?.whereType<String>()
            .toList(growable: false) ??
        const <String>[];
```

Pass both in the returned config.

- [ ] **Step 4: Push the gate wherever the style is pushed**

Add:

```dart
  void _pushFpsOverlayAuto(DashAppearanceConfig config) {
    DashBackend.instance.setFpsOverlayAuto(
      autoOnly: config.fpsOverlayAutoOnly,
      packages: config.fpsOverlayAutoPackages,
    );
  }
```

Call it immediately after every `_pushFpsOverlayStyle(...)` in `_loadAppearance`, `applyAppearance`, `_setFpsOverlay`, and the style setters (`_setAccentIndex`, `_setFpsOverlayScale`, `_setFpsOverlayOpacity`, `_setFpsOverlayRadius`, `_setFpsOverlayPerGamePosition`). It is idempotent and must run at boot so a stale service pref cannot outlive an import.

Add setters beside the other overlay setters:

```dart
  void _setFpsOverlayAutoOnly(bool value) {
    _appearance.value = _appearance.value.copyWith(fpsOverlayAutoOnly: value);
    _pushFpsOverlayAuto(_appearance.value);
    _persistAppearance();
  }

  void _setFpsOverlayAutoPackages(List<String> packages) {
    _appearance.value = _appearance.value.copyWith(
      fpsOverlayAutoPackages: List<String>.unmodifiable(packages),
    );
    _pushFpsOverlayAuto(_appearance.value);
    _persistAppearance();
  }
```

- [ ] **Step 5: Commit**

```bash
git add ui/flutter/lib/main.dart ui/flutter/test/fps_overlay_auto_test.dart
git commit -m "feat(ui): save the overlay auto-show settings"
```

---

### Task 6: Add the auto-show controls and app picker

**Files:**
- Modify: `ui/flutter/lib/main.dart` (call site of `SettingsScreen` ~line 1013, `SettingsScreen` params ~line 2155, FPS overlay section ~line 3139, `_DashShellState._pickFpsOverlayApps`)
- Create: none

**Interfaces:**
- Consumes: `PerAppBackend.instance.command('GET app.list')` (same source the per-app screen uses; entries carry `package`, `label`, `launchable`).
- Produces:
  - `SettingsScreen` params `ValueChanged<bool> onFpsOverlayAutoOnlyChanged`, `ValueChanged<List<String>> onFpsOverlayAutoPackagesChanged`, `VoidCallback onPickFpsOverlayApps`
  - `Future<void> _pickFpsOverlayApps()` on `_DashShellState`

- [ ] **Step 1: Add the picker method**

Place it near `_setFpsOverlayAutoPackages` in the main state:

```dart
  Future<void> _pickFpsOverlayApps() async {
    final List<Map<String, dynamic>> apps;
    try {
      final dynamic reply = await PerAppBackend.instance.command('GET app.list');
      apps = (reply as List)
          .map((dynamic app) => Map<String, dynamic>.from(app as Map))
          .where((Map<String, dynamic> app) => app['launchable'] != false)
          .toList()
        ..sort(
          (Map<String, dynamic> a, Map<String, dynamic> b) =>
              '${a['label'] ?? a['package']}'.toLowerCase().compareTo(
                '${b['label'] ?? b['package']}'.toLowerCase(),
              ),
        );
    } catch (_) {
      _fpsOverlayNotice(
        'The installed-app list is unavailable. Is the daemon running?',
      );
      return;
    }
    if (!mounted) return;

    final Set<String> selected = <String>{
      ..._appearance.value.fpsOverlayAutoPackages,
    };
    await showDialog<void>(
      context: context,
      builder: (BuildContext context) => StatefulBuilder(
        builder: (BuildContext context, StateSetter setDialogState) {
          return AlertDialog(
            title: const Text('Overlay apps'),
            content: SizedBox(
              width: double.maxFinite,
              height: 420,
              child: ListView.builder(
                itemCount: apps.length,
                itemBuilder: (BuildContext context, int index) {
                  final Map<String, dynamic> app = apps[index];
                  final String package = '${app['package']}';
                  return CheckboxListTile(
                    dense: true,
                    value: selected.contains(package),
                    title: Text(
                      '${app['label'] ?? package}',
                      style: const TextStyle(fontSize: 13),
                    ),
                    subtitle: Text(
                      package,
                      style: const TextStyle(fontSize: 10.5),
                    ),
                    onChanged: (bool? checked) {
                      setDialogState(() {
                        if (checked == true) {
                          selected.add(package);
                        } else {
                          selected.remove(package);
                        }
                      });
                    },
                  );
                },
              ),
            ),
            actions: <Widget>[
              TextButton(
                onPressed: () => Navigator.pop(context),
                child: const Text('Cancel'),
              ),
              FilledButton(
                onPressed: () {
                  Navigator.pop(context);
                  _setFpsOverlayAutoPackages(
                    selected.toList()..sort(),
                  );
                },
                child: const Text('Save'),
              ),
            ],
          );
        },
      ),
    );
  }
```

- [ ] **Step 2: Pass the new callbacks**

In the appearance widget call site (~line 1013):

```dart
                  onFpsOverlayAutoOnlyChanged: _setFpsOverlayAutoOnly,
                  onFpsOverlayAutoPackagesChanged: _setFpsOverlayAutoPackages,
                  onPickFpsOverlayApps: _pickFpsOverlayApps,
```

In the widget constructor and fields (~line 2155):

```dart
    required this.onFpsOverlayAutoOnlyChanged,
    required this.onFpsOverlayAutoPackagesChanged,
    required this.onPickFpsOverlayApps,
```

```dart
  final ValueChanged<bool> onFpsOverlayAutoOnlyChanged;
  final ValueChanged<List<String>> onFpsOverlayAutoPackagesChanged;
  final VoidCallback onPickFpsOverlayApps;
```

- [ ] **Step 3: Add the rows**

After the `Remember position per game` switch, before the `Divider` that opens the recording group:

```dart
              SwitchListTile.adaptive(
                contentPadding: EdgeInsets.zero,
                title: const Text('Only selected apps'),
                subtitle: const Text(
                  'Show the overlay only while one of these apps is in front.',
                ),
                value: config.fpsOverlayAutoOnly,
                onChanged: config.fpsOverlay
                    ? (bool enabled) {
                        widget.onFpsOverlayAutoOnlyChanged(enabled);
                        DashHaptics.tap();
                      }
                    : null,
              ),
              ListTile(
                contentPadding: EdgeInsets.zero,
                enabled: config.fpsOverlay && config.fpsOverlayAutoOnly,
                title: const Text('Overlay apps'),
                subtitle: Text(
                  config.fpsOverlayAutoPackages.isEmpty
                      ? 'No apps selected'
                      : '${config.fpsOverlayAutoPackages.length} selected',
                ),
                trailing: const Icon(Icons.chevron_right_rounded, size: 20),
                onTap: widget.onPickFpsOverlayApps,
              ),
              if (config.fpsOverlay &&
                  config.fpsOverlayAutoOnly &&
                  config.fpsOverlayAutoPackages.isEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 2),
                  child: Text(
                    'The overlay stays hidden until at least one app is '
                    'selected.',
                    style: TextStyle(
                      fontSize: 11.2,
                      color: colors.onSurfaceVariant,
                    ),
                  ),
                ),
```

- [ ] **Step 4: Check the section wiring**

Run:

```bash
grep -n "onFpsOverlayAutoOnlyChanged\|onFpsOverlayAutoPackagesChanged\|onPickFpsOverlayApps\|Only selected apps" ui/flutter/lib/main.dart
```

Expected: each name appears in the call site, the constructor, the field list; `Only selected apps` once in the UI.

- [ ] **Step 5: Commit**

```bash
git add ui/flutter/lib/main.dart
git commit -m "feat(ui): add overlay auto-show switch and app picker"
```

---

### Task 7: Add the test to CI and verify the branch

**Files:**
- Modify: `.github/workflows/build-dash-essential-module.yml` (Flutter tests step, `run:` list ~line 203)

**Interfaces:**
- Consumes: `ui/flutter/test/fps_overlay_auto_test.dart` from Task 5.

- [ ] **Step 1: Add the test to the CI list**

Append `test/fps_overlay_auto_test.dart \` to the `flutter test \` file list.

- [ ] **Step 2: Run the local gates**

```bash
cargo fmt --check
cargo test -p dash-essential-daemon
cargo clippy -p dash-essential-daemon --all-targets -- -D warnings
git diff --check main...HEAD
```

Expected: all clean. Flutter and Java compile is CI's job (no SDK here).

- [ ] **Step 3: Push and open the PR**

```bash
git add .github/workflows/build-dash-essential-module.yml
git commit -m "ci: run the overlay auto-show test"
git push -u origin feature-fps-overlay-minimal
gh pr create --fill
```

- [ ] **Step 4: Watch CI**

Run: `gh pr checks --watch`
Expected: Flutter tests (including `fps_overlay_auto_test.dart`), lints, module build and the platform DEX invariant all `success`.

---

### Task 8: Release 1.23.17-dash

**Files:**
- Modify: `CHANGELOG.md`, `Cargo.toml`, `android/kernelsu-next/module.prop`, `android/package/AndroidManifest.xml`, `ui/flutter/pubspec.yaml`, `ui/flutter/lib/release_identity.dart`

**Interfaces:**
- Consumes: merged Task 1–7 on `main`.
- Produces: tag `v1.23.17-dash`, GitHub release with module asset, Telegram announcement (when `TELEGRAM_ANNOUNCE != '0'`).

- [ ] **Step 1: Merge the PR and return to main**

```bash
gh pr merge --merge --delete-branch
git checkout main
git pull --ff-only
```

- [ ] **Step 2: Write the changelog section**

At the top of the port's own sections in `CHANGELOG.md`:

```markdown
## 1.23.17

### Changed

- The FPS overlay chip is now one compact line: frame rate, then CPU, GPU,
  RAM and battery. Temperature, clocks and the package name leave the chip
  and stay in FPS history.
- The overlay can stay hidden until a selected game is in front. Pick the
  games once; the choice is stored by the overlay service and works with
  Dash closed.
```

- [ ] **Step 3: Bump all six version sources to 1.23.17 / 12317**

`Cargo.toml` `version`, `module.prop` `version` + `versionCode`, `AndroidManifest.xml` `android:versionCode` + `android:versionName`, `pubspec.yaml` `version: 1.23.17+12317`, `release_identity.dart` name + code.

- [ ] **Step 4: Run the contract tests**

```bash
./tools/test-release-sync.sh
./tools/test-root-module-contract.sh
```

Expected: both pass with `1.23.17` / `12317`.

- [ ] **Step 5: Commit, tag, push**

```bash
git add CHANGELOG.md Cargo.toml android/kernelsu-next/module.prop android/package/AndroidManifest.xml ui/flutter/pubspec.yaml ui/flutter/lib/release_identity.dart
git commit -m "release: 1.23.17"
git push origin main
git tag v1.23.17-dash
git push origin v1.23.17-dash
```

- [ ] **Step 6: Verify the release**

```bash
gh run list --workflow build-dash-essential-module.yml --limit 1
gh release view v1.23.17-dash --json assets,body
```

Expected: the tag run succeeds, the release body is the 1.23.17 changelog section, the module asset is attached, and the Telegram announce step reports `success` (or is skipped only when the repository variable is `0`).

---

## Self-review notes

- Spec coverage: daemon sensors (Task 1), chip layout (Task 2), auto-show gate (Task 3), FFI/JNI (Task 4), config persistence and boot push (Task 5), settings UI and picker (Task 6), CI (Task 7), release (Task 8).
- One deviation from the design is pinned: the stored selection is comma-separated, not a JSON array; Task 3 updates the design line so the documents agree.
- Test code is written first for both harnesses that exist (Rust unit tests, Flutter JSON tests). Java has no unit harness in this repo; Tasks 2–3 verify by inspection and the module build.
- `overlay_json_with` deliberately takes `Telemetry` so the key contract is covered without sysfs.
- The picker reuses `PerAppBackend.instance.command('GET app.list')`; no icons are fetched, keeping the dialog to one backend call.
