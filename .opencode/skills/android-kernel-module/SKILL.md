---
name: android-kernel-module
description: Use when working on Android kernel modules (KernelSU/Magisk), device drivers, or vendor HAL interactions. Covers module packaging, service.sh patterns, vendor binder calls, SELinux, and device-specific sysfs nodes.
---

# Android Kernel Module Development

## Module Structure
- `android/kernelsu-next/` - KernelSU Next module source
- `android/kernelsu-next/service.sh` - Module entrypoint (runs as root via su)
- `android/kernelsu-next/module.prop` - Module metadata (id, name, version, versionCode)
- `android/kernelsu-next/bin/` - Daemon binaries (dash_daemon, dash_ctl)
- `tools/build-kernelsu-next-module.sh` - Build script (rejects sepolicy.rule)

## Key Patterns

### Service.sh Lifecycle
```bash
# Runs as root in su context
# Waits for boot_completed
# Starts dash_daemon in foreground loop
# Monitors pidof dash_daemon
```

### Vendor Binder Access
- Daemon runs in su context (uid 0, permissive SELinux)
- Vendor services visible only from init/system_server contexts
- Use `AIBinder_isAlive()` before transacting
- Guard all vendor calls in forked child (`guarded_call`)

### Common Sysfs Nodes (MT6991 dash)
- `/sys/kernel/ged/hal/eb_dvfs_policy` - GPU EB DVFS control
- `/proc/gpufreqv2/limit_table` - GPU thermal ceilings
- `/proc/xm_htc_report_rate` - Touch sampling rate (1-6 bytes, bare number)
- `/sys/devices/platform/goodix_ts.0/switch_report_rate` - Goodix fallback

### SELinux
- Module CANNOT patch SELinux (build script rejects sepolicy.rule)
- Device is permissive; su context has broad access
- Vendor service access requires `binder_call` + `service_manager find` rules (AOSP only)

## Device-Specific Notes (MT6991 dash / POCO X8 Pro Max)
- GPU: Mali-G715, 52-step ladder (1612→338 MHz)
- Thermal: THERMAL_AP (id 4) caps at 1430 MHz, THERMAL_EB (id 5) static at 7
- Touch: Novatek panel, `/proc/xm_htc_report_rate` racy (partial reads)
- Display: vendor.xiaomi.hardware.displayfeature_aidl.IDisplayFeature/default
- Crash logs: `/data/adb/dash-essential/crash.log` (DASH_CRASH signal=...)

## Debugging Commands
```bash
# Health check
python3 /tmp/opencode/dash_query_tcp.py "GET health"

# Force daemon restart (from adb)
adb shell su -c 'killall dash_daemon'

# View crash log
cat /data/adb/dash-essential/crash.log
```

## Build Commands
```bash
# Native build + test
export PATH=$PATH:/home/ubuntu/.cargo/bin
cargo build --release --target aarch64-unknown-linux-gnu -p dash-essential-daemon --lib --tests
sudo DASH_STATE_DIR=/tmp/repro/state target/.../dash_essential_backend --test-threads=1

# Android target check (no NDK)
cargo check --release --target aarch64-linux-android -p dash-essential-daemon --tests

# Module build (CI only)
./tools/build-kernelsu-next-module.sh
```

## Release Process
1. Bump version in 6 files: Cargo.toml, Cargo.lock, pubspec.yaml, module.prop, AndroidManifest.xml, release_identity.dart
2. Add CHANGELOG section (before previous)
3. Commit, tag vX.Y.Z-dash, push
4. CI builds 4 assets, publishes GitHub release (pre-release if CHANGELOG says "Pre-release")
5. Telegram announcement auto-sent