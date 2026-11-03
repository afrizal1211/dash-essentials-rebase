# Persistent FPS Overlay — implementation plan

## What's needed

Three independent parts:

1. **Daemon FPS source** — read foreground app FPS from kernel nodes, expose via `GET app.fps` command.
2. **Native FFI bridge** — `dash_backend_get_foreground_fps_value()` callable from Android Java.
3. **Android OverlayService** — `SYSTEM_ALERT_WINDOW` permission, floating `TextView` drawn by `WindowManager`, reads FPS via JNI, survives app backgrounding.

The existing in-app `DashFpsMeter` stays as-is (shows Dash's own render FPS). The overlay shows *foreground app* FPS.

---

## 1. Daemon FPS source

**File:** `runtime/daemon-rust/src/foreground_fps.rs` (new)

### Kernel nodes to probe (MediaTek MT6991 / POCO)

```text
/sys/kernel/fpsgo/fstb/fpsgo_enable     # 1 = FPSGO active
/sys/kernel/fpsgo/fps_info              # aggregate: fps, capacity, cpu_time
/sys/kernel/fpsgo/fstb/renderpid_{N}/fps  # per-thread FPS (fallback)
/sys/class/mdss_fb/refresh_rate          # display refresh (not per-app, skip)
```

Probe order: fpsgo/fps_info first, then per-render-thread nodes, then degrade to unavailable.

### API

```rust
// app_runtime.rs — add to is_command + command dispatch
|| cmd == "GET app.fps"

// New handler in command():
if cmd == "GET app.fps" {
    return Ok(serde_json::to_value(foreground_fps::read()).unwrap());
}
```

### Returns

```json
{"fps":120,"min":118,"low1":115,"dropped":0,"node":"fpsgo","valid":true}
```

- `fps` = average FPS
- `min` = worst single-frame FPS
- `low1` = 1% low (worst 1% of frame intervals averaged)
- `dropped` = frames that missed vsync deadline
- `node` = source node name, or `"unavailable"`
- `valid` = true only if data was read

### Module structure

```rust
// runtime/daemon-rust/src/foreground_fps.rs
pub fn read() -> FpsReading { ... }

#[derive(Serialize)]
pub struct FpsReading {
    pub fps: f64,
    pub min: f64,
    pub low1: f64,
    pub dropped: u32,
    pub node: String,
    pub valid: bool,
}
```

The foreground package name is already tracked by `ForegroundAppTracker`. FPSGO's fps_info gives system-wide frame stats. Pairing with foreground package is approximate (FPSGO attributes by render thread → PID → package).

skipped: per-render-thread parsing (add when device exposes renderpid nodes), fallback to GPU counters (add when fpsgo is confirmed absent).

---

## 2. Native FFI bridge (host Rust)

**File:** `runtime/host-rust/src/lib.rs` (existing FFI entry points)

### Add

```rust
/// Returns foreground app FPS * 100 as i64. 0 means unavailable.
#[no_mangle]
pub extern "C" fn dash_backend_get_foreground_fps_value() -> i64 {
    let json = handle_command("GET app.fps");
    serde_json::from_str::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| v.get("fps").and_then(|f| f.as_f64()))
        .map(|fps| (fps * 100.0) as i64)
        .unwrap_or(0)
}

/// Frees a string returned by dash_backend_get_foreground_fps().
#[no_mangle]
pub unsafe extern "C" fn dash_backend_free_fps_string(s: *mut c_char) {
    if !s.is_null() { drop(CString::from_raw(s)); }
}
```

Build: `cargo fmt`, `cargo clippy --lib --all-targets -- -D warnings`, `cargo test --lib`.

skipped: cached value + invalidation on foreground change (overlay polls at its own cadence, no need for daemon-side cache). Add when overhead is measurable.

---

## 3. Android OverlayService

**File:** `android/app/src/main/java/com/redfox/daemon/FpsOverlayService.java` (new)

### Manifest addition

```xml
<!-- android/package/AndroidManifest.xml -->
<uses-permission android:name="android.permission.SYSTEM_ALERT_WINDOW"/>

<service android:name=".FpsOverlayService"
    android:exported="false"
    android:foregroundServiceType="dataSync"/>
```

### Service design

```
FpsOverlayService
  onCreate()
    → WindowManager.addView(overlayView, params)   // TYPE_APPLICATION_OVERLAY

  onStartCommand(intent)
    → extra "visible" == 0 → hide
    → extra "visible" == 1 → show
    → else (START_STICKY) → show if pref enabled

  onDestroy()
    → WindowManager.removeView(overlayView)

  Runnable pollFps  (500 ms Timer)
    → fps = dash_backend_get_foreground_fps_value()  // JNI
    → overlayView.setText(String.format("%.0f FPS", fps / 100.0))
```

### Overlay view

Compact `FrameLayout`/`TextView` styled to match `DashFpsMeter` chip:
- Background: 90% black rounded rect, 1px accent border (matches existing meter)
- Text: accent color, 18 sp, "120 FPS" format
- Position: top-start, 12 dp margin
- Touch blocked (`FLAG_NOT_FOCUSABLE | FLAG_NOT_TOUCHABLE`) so game input passes through
- Tap → launch Dash settings

### Visibility control

Flutter sends Intent:
```java
Intent i = new Intent(context, FpsOverlayService.class);
i.putExtra("visible", visible ? 1 : 0);
context.startService(i);
```

### Permission handling

On toggle ON, check `Settings.canDrawOverlays(context)`. If false, show explanation dialog then open `ACTION_MANAGE_OVERLAY_PERMISSION` settings for the user to grant.

### Settings wiring

Flutter SharedPreferences key: `fps_overlay_enabled`.
Overlay service reads pref on start; Flutter writes pref + sends Intent on toggle.

---

## 4. Flutter integration

**File:** `ui/flutter/lib/backend/dash_backend.dart`

### New FFI symbol in `start()`

```dart
_foregroundFpsNative = lib.lookupFunction<_I64Native, _I64Dart>(
  'dash_backend_get_foreground_fps_value',
);
```

### Poll (500 ms, existing `_poll()` cadence)

```dart
// In _poll():
final int fpsRaw = _foregroundFpsNative();
final double? foregroundFps = fpsRaw > 0 ? fpsRaw / 100.0 : null;
```

Store in `DashBackendSnapshot`:
```dart
final class DashBackendSnapshot {
  // ... existing fields ...
  final double? foregroundFps;  // null = overlay source unavailable / not enabled
}
```

### Existing FPS meter label

In `ui/flutter/lib/fps_meter.dart` settings toggle area, add:

```
⚠️ Shows this app's render pace only.
Enable FPS overlay below to see the foreground game's FPS.
```

### Settings UI

- Toggle: "FPS overlay" → saves `fpsOverlayEnabled` to native SharedPreferences
- On first ON if permission not granted: show dialog → open overlay settings
- Flutter controls overlay via `context.startService(Intent)` through platform channel

---

## Build / CI impact

- **No new Rust dependencies** — serde already in tree.
- **No new Flutter packages.**
- **New Java file** → `android/app/src/main/java/com/redfox/daemon/FpsOverlayService.java`
- **APK size delta:** ~50 KB — negligible.
- **No version change.**
- CI tests unchanged: `cargo test --lib`, `flutter analyze`.

---

## Order of implementation

1. `foreground_fps.rs` + `GET app.fps` in daemon — testable with `cargo test --lib`
2. FFI bridge in host Rust — verify with JNI test
3. OverlayService Java — verify overlay shows on device
4. Flutter backend FFI symbol + `_poll` — verify value flows
5. Settings toggle + permission flow — verify UX

---

## Implemented (uncommitted)

- Daemon: `runtime/daemon-rust/src/foreground_fps.rs`, `GET app.fps` in
  `app_runtime`, 7 unit tests. Aggregate nodes first, then
  `renderpid_*/fps`; counter nodes need two samples; failures report
  `valid:false, node:"unavailable"` instead of a number.
- Host: `dash_backend_foreground_fps_x100()` (i64, x100) and
  `dash_host_fps_overlay`, `_granted`, `_request`; JNI
  `FpsOverlayService_nativeFpsX100`.
- Android: `FpsOverlayService` (specialUse FGS, `TYPE_APPLICATION_OVERLAY`,
  `FLAG_NOT_FOCUSABLE|FLAG_NOT_TOUCHABLE`, 500 ms HandlerThread poll),
  manifest permissions/service, `DashActivity` forwarding methods,
  `ic_fps_overlay` vector. The notification icon is resolved with
  `getIdentifier` because the platform dex has no generated `R` class.
- Flutter: `DashAppearanceConfig.fpsOverlay`, settings toggle with permission
  redirect + resume completion, startup re-assert, and on-demand
  `DashBackend.foregroundFps()` running on a worker isolate. The FPS value is
  deliberately not read in `_poll()`: the FFI call is a daemon round-trip with
  a 3 s ceiling and must not run on the frame thread.
- Both dex allowlists accept `FpsOverlayService`. Verified locally: javac 8 +
  D8 (android-34 jar stand-in) = 19 952 B < 32 768, dex check PASS; aapt2 link
  of manifest + resources PASS (android-37.2 jar stand-in for android-36).
- `flutter analyze` clean; full `flutter test` 188 passed; daemon tests 137
  passed; clippy clean; `cargo check --target aarch64-linux-android` clean for
  both crates; release sync and tool tests pass.
- Not verified on device (no adb target): FPSGO node paths/format and the
  overlay window itself.
