# Changelog

All notable changes are documented here.

The sections below `## 1.18.5` are the upstream Rodin Essential release
history, kept verbatim as the record of the project this tree is a port of.
The port's own changes are at the top.

## 1.18.16

Pre-release.

### Changed

- The vendor's own EB DVFS policy is taken into account. The daemon reads the
  EB policy of the device, switches it off for a tuned profile, and puts the
  vendor's policy back when the profile is released. The health report says
  whether the vendor's policy is in force, so a profile that stays slower
  than the profile asked for has an answer.
- The touch sampling rate is read from the node more carefully. The node
  answers with a partial line now and then, so the daemon reads it several
  times and only takes a rate that the node repeats. A rate that is out of
  the range the panel takes is not used.
- The display features are asked for in the module's own context as well. A
  service that is not there is told apart from one that is before anything
  is sent to it, and the transaction is asked for again in the private form
  of the vendor route. The health report says which route the daemon used,
  and the report says so as well.

### Fixed

- The high sampling rate is applied through the node when the node takes it,
  and through the ROM's own setting when the node does not take it. The
  setting the user had is put back when the profile is released.
- A binder that is not alive is dropped instead of being sent to. The
  completion for such a binder has been seen to fault, and it faults on a
  binder thread, which took the whole daemon with it.

## 1.18.15

Pre-release.

### Changed

- The Telegram announcement is short. It says what the build is and what
  changed in it, and the whole changelog stays on the release page.
- A dropped connection to the Telegram API no longer fails the release. The
  release is published before the announcement is sent, so a failure there is
  reported and is not taken as a failed build.

### Fixed

- The changelog of the port's own versions has its headings back. Each new
  version had replaced the heading of the previous one, so the entries of
  1.18.9 to 1.18.13 sat under the heading of the version above them.

## 1.18.14

Pre-release.

### Fixed

- A crash in the daemon is written to a crash log that is kept open. The report
  is written with write() only, so it cannot deadlock in a fault that happened
  while a lock was held. A daemon that never dies is a device with no controls
  at all, which is worse than a daemon that restarts.
- The exported report carries the crash log. The answer to "where did it crash"
  is in the report the user already knows how to make.

## 1.18.13

Pre-release.

### Fixed

- The global profile is restored before the per-app profile is applied. The two
  write the same controls, so a per-app profile that was applied first and
  then overwritten by the global profile is a per-app profile that never takes
  effect. The order matters most after a daemon restart, which is when the
  global profile is restored.
- A per-app profile is written again when the platform takes its controls back.
  The power HAL publishes its own GPU state and the stock ROM re-asserts the
  touch rate, so a profile that was only written once was not a profile that
  was in effect.

### Changed

- The health report says which app holds which controls. It also says whether
  the hardware still has them, because a profile that is listed but that the
  platform has taken back is not in effect, and the difference between the two
  is what could not be seen before.

## 1.18.12

Pre-release.

### Fixed

- The daemon only asks the vendor services when it can reach them. The module's
  daemon runs in the module's own context and does not see the vendor service
  manager, so a vendor service it asked for came back as a pending binder
  whose completion has been seen to fault and take the daemon with it. The
  ROM-native daemon, which runs as a system service, can reach them and still
  asks.
- A crash in the daemon now says where it happened. The daemon is the root of
  every control on the device, and a fault that only showed up as the module
  restarting it could not be found.

## 1.18.11

Pre-release.

### Fixed

- The daemon no longer dies on this platform. The daemon runs in the module's
  own context and cannot reach the vendor services, so a service it asks for
  comes back as a pending binder, and the NDK's completion for a pending
  binder has been seen to fault on a binder thread. Every vendor interaction
  now happens in a forked child, and a service that is only looked up is
  looked up without a class, so nothing can be dispatched into the daemon.
- The Novatek NT38771 panel is recognised from the touch bridge. It is
  recognised from the touch bridge's own class device and from the Novatek
  firmware strings, so the panel is known even when the firmware version file
  does not name it.
- The touch report rate is read back until it agrees. The node's output is
  produced per read, and a read that lands while the value changes returns a
  part of it, which was read as a failed write.

## 1.18.10

Pre-release.

### Fixed

- A fault in the vendor services no longer takes the daemon with it. The vendor
  touch service transaction was faulting inside the daemon, and the daemon was
  restarted in a loop; every restart re-applied the global profile, so a
  per-app profile appeared and disappeared. Dash's stock ROM drives the touch
  feature modes, so the daemon drives only the report rate and leaves the
  modes to the ROM that owns them.
- A vendor transaction that does fault is reported as a failed feature. The
  transaction runs in a forked child and only its result is taken back,
  instead of the daemon being restarted in a loop.
- The global guard leaves the controls a per-app profile is holding alone. The
  guard restores the global profile, so it was writing over the per-app
  profile's GPU, CPU and touch settings, and the per-app profile was applied
  and then immediately overwritten.
- The touch report rate is read back correctly. The node reads `report_rate:
  240`, so the value is taken from after the label.

## 1.18.9

Pre-release.

### Added

- Export the diagnostics report to a text file. The daemon writes it to
  `Downloads/Dash-Essential`, or to its own state directory when the storage
  is not there, and the path is shown and copied.
- Credits on the support screen and on the release page.

### Changed

- The support links point at this port's repository and at the @PocoX8ProMaxID
  Telegram community.
- The release page reads as a page. It says what the build is, what changed, how
  to install it, how to check the download, and who made it.

### Fixed

- The Telegram announcement is cut to the size the Telegram API accepts. A long
  changelog section no longer fails the release.

## 1.18.8

Pre-release.

### Fixed

- A per-app profile is applied control by control. A control the hardware
  refuses no longer takes the whole profile down with it, so a profile asking
  for a GPU setting and a touch setting no longer loses the GPU setting
  because the touch setting could not be taken. The refused controls are
  reported by name and reason, in the per-app status and in the logs, and a
  control whose original could not be read is never written.
- Touch profiles drive the panel through Dash's own high touch sampling rate
  knob, `/proc/xm_htc_report_rate`, the node the stock ROM's power HAL and
  Settings toggle use. The Novatek panel has no THP timing block, so the
  search for one could only fail, and that failure used to roll the per-app
  profile back.
- The touch feature modes Dash's own ROM uses, game and active, are the
  required ones. The response calibration and super touch modes are Xiaomi
  panel extras that this panel may not implement, so they are requested but
  not required, and the ones the HAL refuses are reported.
- The effective GPU ceiling is read from the driver in hertz, from the
  thermal summary as well as from the power policy status.

## 1.18.7

Pre-release.

### Added

- A Health & Logs screen. The daemon keeps a bounded, ordered history of
  every control it applies, every command the app issues and every failure,
  and the screen shows it with the live state of each feature. Nothing on
  the screen changes a setting.
- A per-feature health report: the GPU profile and whether it is applied,
  armed or capped, the platform's own GPU ceiling and the service that set
  it, the state of the per-app controls, and what the platform's game
  services are holding.
- The platform's game services are reported. MediaTek's game driver and
  Xiaomi's task turbo publish their own GPU limits, so a per-app profile and
  a game in the foreground compete for the same nodes. The conflict is now
  recorded with the command that ran into it.

### Fixed

- A profile the platform holds below its target is no longer reported as a
  failure. The thermal service holds the GPU at 1430 MHz while the ladder's
  top is 1612 MHz, so the Extreme Beast profile never verified, was re-armed
  on every guard tick, and kept the GPU profile from being changed away from
  it. A profile whose own writes landed is now reported as armed and capped,
  with the service that caps it named, and the guard leaves it alone.
- A per-app profile that selects Extreme Beast is applied instead of
  silently falling back to the global profile, for the same reason.
- The background guard only writes the GPU nodes that have actually drifted,
  and it records what it corrected. Rewriting the whole profile on every tick
  fought the platform's own services and hid which write had mattered.
- Foreground detection for per-app profiles no longer waits on the
  framework. A resume event names the app that took the foreground, and it
  is used directly, so a profile applies as the app opens. The framework's
  own summary is still read when an event cannot describe what changed.

## 1.18.6

Pre-release.

Codename: Astraea. The first port of Rodin Essential v1.18.5 to the
MediaTek MT6991 `dash` platform (POCO X8 Pro Max / REDMI Turbo 5 Max).

This is a pre-release: it is built from this tree and published so the port
can be exercised on hardware. It carries a development signing identity, so
it is not updatable over a release-signed installation and is not offered as
the latest stable build.

### Added

- The GPU operating-point ladder is read from the driver at
  `/proc/gpufreqv2/gpu_working_opp_table`. Dash's ladder is unevenly spaced
  and its index space is not the Mali devfreq table's, so the upstream 26 MHz
  index formula was replaced with a resolution against the published ladder.
  A request the ladder cannot satisfy is reported instead of written as a
  guessed index.
- The Mali cooling device is resolved by its cooling type
  (`devfreq-48000000.mali`) instead of a fixed `cooling_device3` index,
  which is probe-order dependent.
- The ladder's 24 middle slots all report 1430 MHz, so a 1430 MHz floor
  resolves to the last of them and a 1430 MHz ceiling to the first. The
  MediaTek firmware's `THERMAL_EB` row caps at the first of those slots, so
  the firmware's thermal ceiling is 1430 MHz while the ladder's top
  operating point is 1612 MHz.

### Changed

- GPU bounds retargeted to Dash's ladder: 338-1612 MHz with a 598 MHz Battery
  Saver ceiling, against the upstream target's 260-1300 MHz and 598 MHz.
- The GPU devfreq node is `48000000.mali`, the UFS devfreq node is
  `16810000.ufshci`, and the DVFSRC devfreq node lives under
  `/devices/platform/1c013000.dvfsrc` rather than under `/soc`.
- Panel backlight is read from `/sys/class/leds/lcd-backlight`; Dash's
  backlight class is empty.
- Native display baseline is 1280 x 2772 at 520 dpi, against the upstream
  target's 1220 x 2712.
- The Novatek NT38771 panel is recognised alongside the upstream target's
  Goodix and FocalTech panels.
- The application package, root module id, SELinux types and state
  directories moved into the `dash_*` namespace.

### Known issues

- Dash's stock kernel publishes no bypass-charging node, so the
  bypass-charging feature reports itself as unsupported. The charge-current
  limit still works.
- The charge-current tiers keep the upstream calibration. Their absolute
  ceiling comes from the vendor charger configuration, which is not part of
  the public ROM dump, so it needs confirming on the target hardware. The
  daemon verifies every write against its readback and reports a clamped or
  rejected value rather than pretending it applied.

## 1.18.5

Codename: Aurora.

### Added

- Per-App Controls for individual touch response, refresh rate, CPU core/range/
  governor and GPU settings. Profiles default to Follow global; the master
  switch starts off. Saved profiles work independently of the app and restore
  previous global/ROM controls when leaving the foreground app or disabling
  overrides. Includes app icons, search, optional system-app visibility and
  individual/reset-all actions.
- Memory DVFS & UFS hub with LPDDR5X/UFS 4.0 driver readings, Dynamic Range,
  Exact Lock, supported-frequency sliders and OEM Reset. Session controls
  survive app closure but restore originals on daemon restart or reboot.
- Bypass Charging Quick Settings/Control Center tile with matching icon,
  on/off state feedback and long-press access to Charging Control.
- Connected page transitions, predictive Back previews on supported Android,
  touch-following lighting, spring press feedback and consistent haptics.
- Rotating colour-accented Control Hubs headlines.

### Improved

- Home/Control Hubs layout, glass headers, light-mode card boundaries, dividers,
  icons, typography and readable descriptions throughout the interface.
- Dock dragging/tab switching, page opening/Back motion, gesture cancellation,
  slider value indicators, feedback-card clearance and keyboard/navigation insets.
- Per-app foreground ownership and global/ROM restoration, transient CPU core
  write handling, bounded asynchronous control requests and Android rendering
  surface lifecycle handling.
- AOSP and unpacked-ROM integration with scoped charging-parent traversal,
  Per-App Controls and subsystem policy, current framework-adapter validation
  and updated porter instructions.

### Removed

- Double Tap to Wake controls and daemon handling. ROM ownership is unchanged.

Full user-facing release notes: [v1.18.5 — Aurora](docs/releases/v1.18.5.md).

## 1.18.4

Released on 4 October 2026.

### Added

- Kernel-supported bypass charging with Immediate and 20/40/80/90% threshold
  choices, persisted independently of the app, and measured status reporting.
- Whole-app hardware-control pause and reset controls.
- OEM Control choices for touch sampling, display colour and temperature.
  Fresh or missing settings leave those features under ROM ownership; existing
  explicit choices remain saved.
- System Colors: system-wide Material You palettes with custom seed colours,
  wallpaper-based colours, palette styles, fine-tuning and reset controls.
- An unpacked-ROM integration package with partition copy destinations,
  filesystem metadata, certificate-bound application policy and a full porter
  guide. It contains Rodin-only policy additions and a merge tool using the
  porter's own target ROM policies, not whole-ROM replacement policies.
- AOSP integration instructions and a scoped platform-policy preparation helper
  for the native touch timing interface.

### Fixed

- Touch ownership release is serialized with wake and boot restoration so an
  older queued restore cannot resurrect a fixed profile after selecting OEM.
- Display OEM Control stops future overrides without forcing a guessed stock
  colour mode. The current appearance may remain until the ROM reapplies it.
- Hardware reset invalidates applied-state caches, and bypass verification is
  retained across page changes without redundant unchanged kernel requests.
- Dock clearance and card text wrapping improve readability. App-facing
  descriptions use the full Rodin Essential name.
- Refined Home, Control Hubs, Support, charging cards, confirmation dialogs and
  haptics, including a continuous bypass-threshold selector.
- Root-module migration handles the signing-key rotation for user-installed
  copies while preserving daemon settings. Application-local preferences reset
  during this one-time migration; differently signed ROM-native updates remain
  rejected rather than bypassing Android package identity.
- ROM application policy uses certificate-bound IPC, normal application MCS
  isolation and scoped daemon hardware permissions. Policy tests no longer
  treat a runtime-mode compile as a strict neverallow pass.
- App, daemon/host packages, module metadata and About label are synchronized
  at 1.18.4 / code 11804. The matching IPC protocol remains 13.6.
- System Colors now writes the version-correct Android Monet contract: the
  legacy palette/accent pair on Android 12 and palette/style fields on Android
  13 and newer.
- Wallpaper mode now restores Android's explicit `home_wallpaper` source so
  the palette follows later wallpaper changes.
- Custom palette changes require verified native resource regeneration.
  Settings-only changes on incomplete OEM implementations are rolled back and
  reported as unsupported instead of being presented as applied.
- Palette readback uses a longer bounded settle window for slower OEM overlay
  transactions while preserving conflict detection and rollback safety.

## 1.18.3

### Fixed

- Restored the complete v1.18.0 Touch Response pipeline: native 240/480 Hz
  timing for the 250/500 profiles and the verified one-millisecond Android
  output scheduler for the 1000 profile.
- Preserved the authenticated reverse and privileged-loopback transports,
  bounded framework startup, transactional persistence, and daemon-first boot
  ordering that prevent a healthy KernelSU Next or Magisk backend from
  appearing Offline.
- ROM-native 1000 Hz output now opens the dedicated Rodin input device directly
  and no longer requests `SYS_PTRACE`, `DAC_OVERRIDE`, or `DAC_READ_SEARCH`.
  This removes the Android 17 platform-neverallow conflicts without granting
  the application root or privileged Android identity.
- AOSP policy validation now distinguishes pre-existing OEM platform-policy
  diagnostics from violations introduced by the Rodin integration while still
  rejecting any Rodin neverallow failure.
- Application, Rust workspace, KernelSU/Magisk module, manifest, interface, and
  release metadata are synchronized at version 1.18.3 / code 11803.
- The KernelSU/Magisk installer now supports signature-compatible ROM-native
  installations as a reversible update layer. It preserves native state, stops
  the init-owned daemon before module takeover, and restores the ROM APK and
  native service when the module is removed. Cross-signature updates still fail
  safely without deleting application data.

## 1.18.2

### Fixed

- KernelSU Next and Magisk communication no longer depends on an
  `untrusted_app` process connecting into a root-manager SELinux domain. The
  daemon now initiates an authenticated reverse socket to the normal APK, and
  both sides verify the peer UID with `SO_PEERCRED`.
- Module Action distinguishes root-only daemon health from a connection that
  was successfully completed by the Android app.
- Root modules now provide an authenticated privileged-loopback transport for
  ROMs whose SELinux policy blocks Unix `connectto` in either direction. The
  app proves the privileged-port boundary from its own sandbox, and the daemon
  resolves the accepted client's exact package UID from `/proc/net/tcp` before
  serving hardware commands. Cached transports and a temporarily occupied
  listener port recover automatically without relaunching the app or rebooting.
- Root-module installation no longer modifies live SELinux policy or depends on
  root-manager domain names. The script-only package uses no system overlay and
  therefore requires no KernelSU metamodule.
- Root-module updates verify the APK version and official signing certificate.
  A differently signed installed copy is reported clearly and is never removed
  automatically.
- Persisted state is committed transactionally with file and directory sync, so
  an applied command cannot be reported as durable when the state write failed.
- The native daemon binds its socket and enters the accept loop before any
  framework telemetry or restore command can wait on `system_server`. Framework
  command execution is bounded, and SELinux permits only the required reply-pipe
  direction, preventing a healthy native service from appearing Offline.
- Display, GPU, CPU, UFS, charging, and ZRAM commands now reject failed or
  mismatched live readback instead of saving a false success. Touch commands
  require an accepted vendor transaction before persistence is committed.
- TouchFeature setter replies now parse both the AIDL status header and vendor
  result instead of mistaking a delivered Binder transaction for an accepted
  panel command.
- Touch Response retains the proven v1.18.0 250, 500, and 1000 profiles. Native
  modes use the 240/480 Hz timing blocks, while 1000 keeps the 480 Hz source and
  restores the verified one-millisecond Android output scheduler.
- The unused per-application profile subsystem and its package-query surface
  have been removed from the daemon, host, manifest, and interface.
- AOSP integration now preserves an explicit APK signing identity, maps that
  certificate and package to the dedicated `rodin_app` domain, labels only the
  required exact generic Mali and Goodix sysfs paths, and keeps the v1.18.0
  touch permissions isolated to the daemon policy for device-tree review.
- ROM-native policy again includes the narrowly scoped touch-HAL process and
  input access required by the v1.18.0 1000 Hz output path.
- Application, module, daemon protocol, documentation, and release metadata
  are synchronized under a new version so different payloads are no longer
  distributed under the same release identity.

## 1.18.0

### Added

- CPU Core & Frequency hub with independent frequency control for the efficiency,
  performance, and prime policies.
- Dynamic minimum/maximum ranges and exact `min = max` locks at any frequency
  exposed by the active kernel OPP table.
- Separate saved-target and live-effective readback, per-policy drift status,
  verified writes, and an OEM range reset.
- ROM-native logical-density detection and automatically scaled resolution
  presets instead of a fixed 520 DPI baseline.
- Live/selected refresh-rate telemetry and persisted custom resolution state.

### Changed

- CPU frequency requests are validated against the live driver table instead of
  accepting arbitrary values that the kernel may silently round.
- Frequency targets remain fully independent from CPU governors and GPU modes.
- CPU topology labels now identify the Dimensity 8400's three all-Cortex-A725
  groups instead of the previous generic A520/A720/X4 labels.
- Native host/daemon protocol updated to 13.3 for CPU, display, and touch-path
  telemetry.

### Fixed

- KernelSU Next and Magisk installations now generate an app-to-daemon socket
  rule for the root manager's live SELinux domain (`su`, `ksu`, or `magisk`).
  This prevents a healthy daemon from appearing Offline on ROMs that omit the
  corresponding `connectto` allowance.
- Root-module startup now reapplies the detected socket rule to the live policy
  after boot through KernelSU's `ksud` or Magisk's `magiskpolicy`. This covers
  manager builds that skip or race the early `sepolicy.rule` loader, and Module
  Action reports the result and patch engine used.
- The root-module daemon validates every socket peer with `SO_PEERCRED` and
  accepts hardware commands only from UID 0 or the exact Android UID assigned
  to the bundled Rodin Essential package.
- CPU range application now selects Rodin's OEM `thermal-nolimits` policy before
  updating the per-policy Xiaomi thermal ceiling, MediaTek PowerHAL request, and
  cpufreq limits. Exact locks remain at the full 2100/3000/3250 MHz policy
  maxima without changing a governor, and requests fail instead of being saved
  when live verification does not match.
- The previous Xiaomi thermal configuration is saved before the first custom
  CPU range and restored when the last range is reset. `mi_thermald`, AOSP
  `thermald`, and the MediaTek thermal HAL remain running throughout; the boot
  restore and drift guard also verify the live OEM configuration.
- Xiaomi's fresh-boot `sconfig=-1` sentinel is treated as the normal thermal
  configuration, allowing persisted exact CPU locks to restore before any
  explicit vendor mode has been published.
- Frequency cards show the live governor selected elsewhere in the app, provide
  clearer apply/reset guidance, and emit discrete haptic ticks while sliding.
- KernelSU Next and Magisk startup now waits for Android boot completion, then
  repeats the complete persisted-state transaction across the vendor settling
  window. Saved controls remain daemon-owned when the app is force-stopped.
- Touch persistence verifies the active THP timing instead of trusting cached
  state, so late vendor defaults are detected and corrected automatically.
- Native 250 mode now applies Rodin's dedicated TouchFeature sensitivity latch
  instead of sharing the 500-mode calibration that could silently fall back to
  roughly 135 Hz while the HAL still reported 240 Hz.
- The 1000 Hz output scheduler now discovers the actual multitouch event among
  every TouchFeature service descriptor and can attach directly to the panel
  event when FocalTech firmware does not retain a compatible service handle.
- Repeated boot and watchdog restores no longer reset an already-correct ZRAM
  device; compression, size, and swappiness changes are now idempotent.
- Resolution telemetry refreshes after framework startup, distinguishes live
  adaptive refresh from the ROM-selected rate, and no longer reports a false
  hardcoded 60/120 Hz value.
- Cold launches now present a plain application surface instead of exposing an
  oversized launcher-icon transition, while appearance loading and daemon
  startup no longer block the first application frame.
- Extracted Flutter assets and ICU data now use a deterministic content stamp.
  Unchanged runtime files are reused on later cold starts, while APK updates or
  incomplete cache files force a verified refresh before Flutter starts.

## 1.17.1

### Fixed

- GPU performance modes no longer change CPU governors or CPU frequency
  ranges. Existing CPU settings remain independent when switching GPU modes.
- Legacy CPU values written by earlier GPU profiles are migrated once without
  overwriting separately customized CPU settings.
- Vendor CPU and platform thermal services remain running in every GPU mode;
  Gaming Dynamic and Extreme Beast retain ownership only of the Mali cooling
  constraint required by their GPU policy.
- GPU profile transitions are serialized with the maintenance and dynamic
  guards, preventing the previous mode from reasserting stale values during a
  rapid mode switch.

## 1.17.0

### Added

- Complete Rust privileged daemon and command-line health client.
- Persistent CPU, GPU, touch, display, charging, ZRAM, and UFS state.
- Extreme Beast, Gaming Dynamic, Stock Balanced, and Battery Saver transactions.
- Live Mali frequency/load readback, GED state, governor, and power-policy UI.
- Native 240/480 Hz touch timing and 1 ms Android output mode.
- Goodix and FocalTech detection through the Rodin touch vendor stack.
- UFS scheduler application and verification across detected logical units.
- Motion-feel control and refined application transitions.
- Combined KernelSU Next and Magisk module with bundled unprivileged APK.
- AOSP `/product` integration bundle, init service, and split SELinux policy.

### Changed

- The APK remains an ordinary app UID; all privileged operations are isolated in
  the daemon.
- GED is profile-owned: enabled only for Gaming Dynamic and Extreme Beast.
- Profile, touch, storage, CPU, and memory settings are reapplied after boot and
  guarded against vendor drift.
- Persisted GPU profiles recover from late MediaTek power-service overrides;
  fixed-frequency restoration verifies the live target OPP before locking DVFS.
- Build output is zero-DEX, release-mode, ARM64-only, and verified for 16 KB APK
  and ELF alignment.

### Removed

- Direct live-partition mutation scripts.
- Empty privileged-permission allowlist packaging.
- Obsolete migration audits, phase markers, and proof-only build scripts.
