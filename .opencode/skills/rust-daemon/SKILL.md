---
name: rust-daemon
description: Use when working on the dash-essential-daemon Rust codebase (lib.rs, app_runtime, app_controls, diagnostics, service_control, vendor_binder, foreground_app). Covers async patterns, unsafe FFI, signal handling, IPC, and native testing.
---

# Rust Daemon Development (dash-essential-daemon)

## Codebase Structure
```
runtime/daemon-rust/src/
├── lib.rs              # Main daemon (IPC, health, commands; vendor_binder is cfg-gated)
├── app_runtime.rs      # Per-app profile logic (Adapter, ownership, borrowing)
├── app_controls.rs     # Control bitmasks, ownership, drift handling
├── diagnostics.rs      # Event log, GPU thermal limits, vendor limits
├── service_control.rs  # Journaling, persistence, restoration
├── vendor_binder.rs    # NDK binder FFI (guarded_call, AIBinder_isAlive) — target_os = "android" only
├── subsystem_clocks.rs # GPU/CPU clock management
├── touch_resampler.rs  # Touch rate reading with deduplication
├── system_colors.rs    # Wallpaper color extraction
├── foreground_app.rs   # Foreground app tracking via AccessibilityService events
└── bypass_policy.rs    # Bypass charging logic
```

`mod vendor_binder` is gated `#[cfg(target_os = "android")]`; the non-Android stub in lib.rs returns `false` from every call.

## Key Patterns

### Vendor Binder (vendor_binder module)
```rust
// Always check aliveness before transacting
fn find_service(service: &'static [u8]) -> Option<*mut AIBinder> {
    let binder = service_manager_check_service(service.as_ptr().cast());
    if binder.is_null() || !unsafe { AIBinder_isAlive(binder) } {
        unsafe { if !binder.is_null() { AIBinder_decStrong(binder) } }
        return None;
    }
    Some(binder)
}

// All vendor calls in guarded child
fn guarded_call(call: impl FnOnce() -> bool) -> bool {
    // fork → child does call → parent waits 2s → kills child on timeout
}
```

### Crash Reporter
- `open_crash_log()` opens `/data/adb/dash-essential/crash.log` at startup
- `install_crash_reporter()` installs SA_SIGINFO|SA_ONSTACK handlers for 6 signals
- Handler writes `DASH_CRASH signal=N code=... addr=0x... pc=0x... lr=0x... sp=0x...` via `write()` only
- Pre-opened FD avoids alloc in fault handler

### Per-App Profile System
- `Adapter::apply()` - writes hardware for borrowed profile (no persist)
- `Adapter::restore()` - writes hardware + persists global after `Temporary::begin()`
- `reconcile()` / `reconcile_with()` - keep ownership on foreground read failure
- `global_may_write_gpu()` - GPU owned by global if no per-app profile owns it

### State Persistence (service_control)
- `remember(key, value)` - journals original value (never overwrites)
- `remember_captured(key, value)` - captures current hardware state
- `write(path, value)` - writes sysfs/node, verifies
- `forget(key)` - removes journal entry
- `original(key)` / `replace(key, value)` / `has_original(key)`

### Diagnostics
```rust
// Areas: AREA_GPU, AREA_TOUCH, AREA_DISPLAY, AREA_PER_APP, AREA_CHARGING, AREA_CPU, AREA_SERVICE, AREA_CONFLICT
diagnostics::record(area, level, what, detail);
// History: bounded ring buffer, exported via GET logs / ACTION logs.export
```

### IPC Protocol (TCP 127.0.0.1:732)
- Request: `@<len>\n<body>` (e.g. `@13\nGET health`)
- Response: `OK {json}` / `ERR message`
- Commands: `GET health|logs|snapshot`, `SET <feature> <value>`, `ACTION <feature>.<action>`

## Build & Test
```bash
# Native
export PATH=$PATH:/home/ubuntu/.cargo/bin
cargo build --release --target aarch64-unknown-linux-gnu -p dash-essential-daemon --lib --tests
DASH_STATE_DIR=/tmp/repro/state target/.../dash_essential_backend --test-threads=1

# Android target (no NDK)
cargo check --release --target aarch64-linux-android -p dash-essential-daemon --tests

# Specific test
cargo test --release --target aarch64-unknown-linux-gnu -p dash-essential-daemon reads_the_vendor_eb_dvfs_mode_from_the_node_report -- --test-threads=1
```

## Common Tasks
- Add new SET command: match in `handle_command_inner`, add to `diagnostics_area`
- Add new health field: extend `diagnostics_health_json()`
- New vendor call: add to `vendor_binder` module, use `guarded_call`
- New per-app control: add bit to `ControlBit`, update `global_may_write_*`, `owns_*`
- New diagnostic area: add const to `diagnostics.rs`, use in command routing