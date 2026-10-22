---
name: per-app-controls
description: Use when working on the per-app profile system (app_runtime.rs, app_controls.rs). Covers profile borrowing, ownership transfer, foreground detection, drift handling, and contract testing.
---

# Per-App Controls System

## Architecture

### Core Types
```rust
// Control bitmask (bit = 1 means per-app owns this control)
const CONTROL_GPU: u32 = 1 << 0;
const CONTROL_REFRESH: u32 = 1 << 1;
const CONTROL_CPU_CORES: u32 = 1 << 2;
const CONTROL_CPU_GOVERNOR: u32 = 1 << 3;
const CONTROL_CPU_RANGE: u32 = 1 << 4;
const CONTROL_ZRAM: u32 = 1 << 5;      // Global only
const CONTROL_TOUCH: u32 = 1 << 6;
const CONTROL_DISPLAY: u32 = 1 << 7;
const CONTROL_CHARGING: u32 = 1 << 8;
const CONTROL_BYPASS: u32 = 1 << 9;
const ALL_CONTROLS: u32 = (1 << 10) - 1;
```

### Adapter States
```rust
enum Ownership { Global, App(u32) }  // which profile owns the control
struct Adapter {                      // one per control type
    ownership: Ownership,
    persisted: PersistedState,        // global preference
    transient: Option<TransientState> // active borrowed profile
}
```

## Borrowing Protocol

### App Enters Foreground
1. `Adapter::apply(borrowed_profile)` called
2. Writes hardware **inside** `Temporary::begin()` (no persist)
3. Sets `ownership = App(profile_id)`
4. Logs `per-app/info/profile.borrowed` + `profile.applied`

### App Loses Foreground / Fails to Answer
```rust
fn reconcile() {
    if !foreground_responds() {
        // KEEP ownership, re-apply borrowed profile
        // Don't revert to global until new foreground takes over
    }
}
```

### Explicit Release (user disables app profile)
```rust
fn release() {
    // Write hardware back to global persisted value
    // Log profile.applied with refused = [controls returned to global]
    // ownership = Global
}
```

### Per-App Profile Config
```json
{
  "enabled": true,
  "notify": false,
  "profiles": [
    {
      "package": "com.example.game",
      "profile": { "enabled": true, "gpu": 1, "refresh": 120 },
      "user": 0
    }
  ]
}
```

## Key Invariants

1. **ZRAM/swap always global** - never borrowed
2. **PERFORMANCE_STATE** = effective profile (borrowed while active)
3. **Drift handling**: hardware state mismatch → re-apply owned profile
4. **Foreground read failure** → keep borrowed profile active (don't revert)
5. **Global preference persists** across borrows (saved in state.conf)
6. **Release order**: GPU → Touch → CPU → Charging → Display → ZRAM

## Adapter Methods

### `apply(borrowed_profile)`
- Writes hardware via `Temporary::begin()`
- Does **not** call `mutate_persisted_state`
- Logs `profile.borrowed`

### `restore()`
- Writes hardware inside `Temporary::begin()`
- Then persists global preference via `mutate_persisted_state`
- Logs `profile.restored`

### `reconcile()` / `reconcile_with(foreground_pkg)`
- `reconcile()`: called periodically, keeps ownership if foreground same
- `reconcile_with()`: called when foreground changes, transfers ownership

### `global_may_write_gpu()`
```rust
fn global_may_write_gpu() -> bool {
    !per_app_owns_gpu()  // true if no per-app profile owns GPU
}
```

## Ownership Checks (app_controls.rs)
```rust
fn owns_gpu() -> bool { ownership == App(_) && bit(CONTROL_GPU) }
fn owns_touch() -> bool { ownership == App(_) && bit(CONTROL_TOUCH) }
fn owns_cpu_cores() -> bool { ... }
fn owns_cpu_governor() -> bool { ... }
fn owns_cpu_range() -> bool { ... }
```

## Transition Logic (transition_service)
```rust
fn transition_service(cmd: &str) {
    if cmd == "SET service.enabled 1" {
        // Enable: restore persisted state, wake app_runtime
    } else {
        // Disable: capture resume, set release_pending, restore_released_controls
    }
}
```

## Testing (MockHardware / FakeHardware)
```rust
// app_runtime.rs tests
#[test]
fn the_applied_log_names_the_controls_that_are_in_effect() { ... }
#[test]
fn a_borrowed_write_does_not_change_the_state_of_the_device() { ... }
#[test]
fn a_foreground_that_does_not_answer_keeps_the_profile_of_the_app() { ... }
#[test]
fn a_per_app_profile_keeps_the_gpu_from_the_global_writers() { ... }

// app_controls.rs tests
#[test]
fn a_per_app_profile_keeps_the_gpu_from_the_global_writers() { ... }
```

## Common Scenarios

### User opens game → borrows GPU+Refresh
1. App launches → app_runtime detects foreground
2. `Adapter::apply(profile=1)` for GPU+Refresh
3. Hardware set to 1612 MHz + 120 Hz
4. Logs: `profile.borrowed` + `profile.applied controls=[Gpu, Refresh]`

### User switches to another app without per-app profile
1. Old app loses foreground
2. `reconcile()` sees no new profile → keeps borrowed profile active
3. Hardware stays at borrowed settings
4. Logs: `profile.borrowed` continues

### User disables per-app for that game
1. `release()` called
4. Hardware reverts to global persisted (profile 2 = 598 MHz)
5. Logs: `profile.restored` with `refused=[Gpu, Refresh]`

### Global profile change while app borrows
1. User changes global perf profile via UI
2. `global_may_write_gpu()` = false (app owns GPU)
3. Global change **deferred** until app releases
4. When app releases → global change applied

## Debugging
```bash
# Check current ownership
python3 /tmp/opencode/dash_query_tcp.py "GET health" | jq '.perApp'

# Expected: ownedControls shows which controls borrowed
# "ownedControls": ["Gpu", "Refresh"]
```

## Common Bugs
- **Drift**: hardware state doesn't match ownership → check `reconcile()` timing
- **Stuck borrow**: app closed but profile still active → `reconcile()` not called
- **Global overwritten**: `mutate_persisted_state` called during borrow → check `transient_write()`
- **ZRAM borrowed**: `CONTROL_ZRAM` never in per-app bitmask (hardcoded global)