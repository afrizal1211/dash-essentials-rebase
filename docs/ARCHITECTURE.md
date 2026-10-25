# Architecture

Dash Essential separates the visual application from all privileged hardware
operations. The separation is identical in AOSP and root-module deployments; only
the daemon launcher and state directory change.

## Runtime components

```text
Flutter AOT interface
        │ Dart FFI
        ▼
Rust NativeActivity host
        │ authenticated local IPC, protocol 13.6
        ▼
Rust dash_daemon
        ├── sysfs / procfs / block controls
        ├── Xiaomi touch and display AIDL services
        ├── MediaTek GED and Mali devfreq
        ├── Android SettingsProvider / native palette readback
        └── persistent state, drift guards, and power-supply events
```

### Android package

`android/package/AndroidManifest.xml` declares `android.app.NativeActivity` and
`android:hasCode="false"`. The APK contains no `classes.dex`; its executable
payload is ARM64 native code:

- `libdash_essential_host.so`
- `libflutter_engine.so`
- `libapp.so`

The APK uses a regular application UID. It has no shared UID, root launcher,
privileged permission allowlist, or direct sysfs write path.

In an AOSP integration, its certificate-bound `dash_app` domain retains the
standard `untrusted_app_all` base policy. The dedicated label narrows daemon
socket ownership; it does not turn the APK into a privileged or system app.

### Native host

`runtime/host-rust` owns:

- NativeActivity lifecycle, Android window, input queue, and back dispatch.
- EGL/OpenGL/Skia Flutter embedder lifecycle and choreographer frame pacing.
- JNI bridges for permissions, haptics, theme, and URL launch.
- Nonblocking Dart FFI calls and cached daemon telemetry.
- Command serialization over the daemon socket.

Hardware I/O is never performed on Flutter's UI thread.

### Flutter interface

`ui/flutter` owns presentation and interaction state. It reads immutable cached
snapshots and submits commands to the native backend. Temporary optimistic state
is replaced by daemon readback as soon as the hardware transaction completes.

### Privileged daemon

`runtime/daemon-rust` owns all root-required operations. It validates commands,
writes hardware nodes, calls vendor AIDL services, verifies readback where the
driver exposes it, persists accepted settings, and reasserts settings when a
vendor component rewrites them.

Charging profiles use Dash's kernel FCC and SIC interfaces directly. A
power-supply uevent listener restores a fixed profile after a cable reconnect;
the daemon does not poll or repeatedly overwrite the charger control loop.

The daemon listens on a Linux abstract Unix socket. In an AOSP build, SELinux
allows only the dedicated `dash_app` domain to connect in production. The
`dash_ctl` client is allowed only on `userdebug` and `eng` builds.

Unix peers validate Linux credentials with `SO_PEERCRED`: the app accepts only
UID 0, and the daemon accepts only UID 0 or the exact UID Android assigned to
Dash Essential. Some root-manager SELinux domains block Unix `connectto` in
both directions, so the module also binds a root-privileged IPv4 loopback port.
The app refuses that path unless a neighbouring low-port bind is denied from
its own UID/SELinux sandbox, and the daemon maps the live client tuple through
`/proc/net/tcp` to the package UID before accepting a command. A daemon-initiated
Unix path remains as a compatibility fallback. The root module never patches
SELinux. AOSP builds use the direct path with a certificate-bound dedicated
`dash_app` domain and do not enable the module-only loopback listener.

## Persistence

The daemon selects its state directory from `DASH_STATE_DIR`:

- AOSP init service: `/data/system/dash-essential`
- KernelSU Next or Magisk service: `/data/adb/dash-essential`
- Development fallback: `/data/adb/dash-essential`

State updates are transactional. The daemon writes `state.conf.tmp` with mode
0600, synchronizes it, renames it over `state.conf`, and synchronizes the state
directory before replacing its in-memory state. A command cannot report a
durable success if that sequence fails. The daemon loads the file once at
startup, restores the hardware domains it owns after Android boot completes,
and keeps running independently of the application process. Removing the app
from recents or force-stopping it therefore does not stop the active backend
configuration.

Persisted domains include performance profile, GPU bounds/governor/GED/power
policy, CPU governors and ranges, online-core mask, UFS scheduler, touch
profile, display settings, charging, and ZRAM.

System Colors is deliberately separate: Android's per-user SettingsProvider
persists the palette and SystemUI generates the tonal overlays. There is no
palette entry in `state.conf` and no palette reassertion timer. The host caches
its verified result independently of hardware state. See
[System Colors](SYSTEM_COLORS.md) for transactions, compatibility, and policy.

## Touch paths

Dash Essential first uses Xiaomi's vendor `ITouchFeature` AIDL service, which
abstracts the supported panels. Dash ships a Novatek NT38771 panel; the Goodix
and FocalTech panels of the upstream target are recognised as well. A Goodix
sysfs path is used only as a fallback on ports that omit the AIDL service.

- 250 mode requests the native 240 Hz timing block.
- 500 mode requests the native 480 Hz timing block.
- 1000 mode keeps the physical source at 480 Hz and emits the v1.18.0 verified
  one-millisecond Android event stream. Root modules prefer the duplicated
  vendor event descriptor and retain the direct-device fallback; ROM-native
  integration uses its dedicated input-device label directly.

The ROM-native path does not trace or inspect the touch HAL and therefore does
not require `SYS_PTRACE`, `DAC_OVERRIDE`, or `DAC_READ_SEARCH`. Raw input access
remains restricted to the dedicated daemon domain.

## Performance ownership

GPU profile changes are isolated transactions: GPU bounds, governor, GED state,
power policy, and the Mali cooling constraint are applied together.

The GPU bounds are published against the ladder the driver reports at
`/proc/gpufreqv2/gpu_working_opp_table`, not against a formula. Dash's
operating-point ladder is unevenly spaced and its index space is not the Mali
devfreq table's, so a floor resolves to the ladder position of the slowest
step that is still fast enough and a ceiling to the fastest step that is still
slow enough. A request the ladder cannot satisfy is an error, not a guessed
index. The Mali cooling device is resolved by its cooling type rather than by a
fixed `cooling_device` index, because that index is probe-order dependent. CPU
governors, CPU frequency ranges, and CPU core state are controlled independently.
A background guard compares persisted intent with live readback and reapplies
only drifted values owned by each subsystem.

CPU frequency choices come from each policy's live
`scaling_available_frequencies` table, with `stats/time_in_state` as the kernel
compatibility fallback. Range and exact-lock requests are applied atomically,
verified against `scaling_min_freq` and `scaling_max_freq`, and persisted without
changing the policy governor. On Xiaomi Dash builds, the daemon updates the
same per-policy `thermal_message/cpu_limits` request used by `mi_thermald` and
the three-field `powerhal_cpu_ctrl/perfserv_freq` request before writing
cpufreq. A request is accepted only when the final live range matches. The
interface reports saved targets separately from effective live limits.

Dash's normal Xiaomi thermal configuration periodically republishes CPU QoS
ceilings, so a reapply timer alone cannot provide a sustained exact lock. Before
the first custom CPU range, the daemon records `thermal_message/sconfig` and
selects the vendor-defined mode `6` (`thermal-nolimits.conf`). This removes the
competing Xiaomi CPU policy while leaving `mi_thermald`, AOSP `thermald`, and the
MediaTek thermal HAL running. The saved mode is restored after the last custom
range is reset. Startup restoration establishes this ownership before applying
cpufreq, and the guard verifies both the mode and each saved range. The kernel's
fresh-boot `sconfig=-1` sentinel is normalized to the default normal mode `0`
before ownership is recorded.

Gaming Dynamic and Extreme Beast profiles borrow the vendor's `thermal_ap`
configuration (mode `MI_THERMAL_AP_MODE`) instead of the `thermal-nolimits`
mode CPU ranges use. The borrow is disabled by default (`MI_THERMAL_AP_MODE = -1`);
enable it after on-device verification by setting the mode id. GPU thermal
borrow applies only when no CPU range is saved — CPU range ownership holds
the stricter `nolimits` mode, which takes precedence. The borrow is journaled
via the service control journal and restored on release.

A background guard (`cpu_frequency_guard`) self-heals a stuck `sconfig=6`
(nolimits) left by a daemon restart or a lost save: when no saved range or
thermal mode owner exists and the live mode reads `6`, it immediately writes
`0` (normal) and logs a warning. The fresh-boot `-1` sentinel is normalized
to `0` by `normalize_mi_thermal_config_mode`, so clean boots never false-trigger.

Vendor CPU and platform thermal services remain running in every GPU mode.
Gaming Dynamic and Extreme Beast retain ownership of the Mali cooling constraint
required by their requested GPU policy without stopping those global services.

## Build invariants

Every release must satisfy:

1. No DEX entries in the APK.
2. `android:hasCode="false"` in the manifest.
3. ARM64-only native payload.
4. 16 KB APK and ELF alignment.
5. Flutter framework and engine revision compatibility.
6. No UI-thread filesystem, Binder, socket, or daemon work.
7. No root or privileged Android identity for the APK.
8. A dedicated SELinux daemon domain for AOSP integration.
