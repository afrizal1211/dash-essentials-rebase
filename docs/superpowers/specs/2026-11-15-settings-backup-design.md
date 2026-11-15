# Settings backup and restore — design

## Intent and why

Carry all restorable Dash Essential settings from one Dash phone to another —
a ROM reflash, a replacement unit, or a move between a root module install and a
ROM-native install. Import **replaces everything**: it is a clean transfer, not a
merge and not a sync.

## Goals

- One JSON file holds every restorable setting the user can change.
- Export and Import entries live in the existing Settings screen.
- Import replaces all current settings, app-side UI state included.
- Import refuses a bundle that is not from this platform; every hardware value
  is re-validated by the same code path a manual change takes; nothing reaches
  a sysfs node without that validation.

## Non-goals and anti-goals

- No merge, no diff, no cloud, no automatic background transfer.
- Not included: System Colors (the platform's SettingsProvider owns the palette
  and SystemUI generates the overlays), FPS record sessions (bulk binary data),
  the aurora mini-game high score, diagnostic logs.
- Anti-goal: import must never copy `state.conf`, `app-profiles.json`,
  `conditionals.json` or `dexopt.json` over their live counterparts. Raw file
  copy would bypass every check that keeps imported values honest on a kernel
  whose OPP tables may differ.

## Constraints

- No new Flutter dependencies (`pubspec.yaml` holds `flutter`,
  `material_color_utilities: 0.13.0`, and `flutter_test` in dev), so no
  file-picker or share plugin. File selection goes through the Android Storage
  Access Framework or MediaStore, driven from `DashActivity`.
- `android/platform` may contain only the five existing classes, enforced by
  `tools/check-platform-dex.sh`, which also caps `classes.dex` at 40,960 bytes.
  New methods on `DashActivity` survive the class check (inner classes `$` are
  allowed), but the 40 KB DEX budget is a real bound on how much Java may be
  added.
- Wire protocol, two different bounds:
  - inbound: framed `@<len>\n<command>` with **len ≤ 4096 bytes**
    (`runtime/daemon-rust/src/lib.rs:9149`); legacy reads cap at 4095. An
    oversized frame is dropped without a reply, so the Dart send path must
    treat a connection reset as "command too large". An import bundle cannot
    fit one command and must be uploaded in bounded chunks.
  - outbound: the daemon writes replies with no cap of its own; the **host
    bridge** is what reads at most 786,432 bytes
    (`runtime/host-rust/src/backend_bridge.rs:550-553`) and fails the JSON
    parse on anything larger. Dart requests 1 MiB (`per_app_backend.dart:78`),
    so 786,432 is the binding bound. Export must therefore answer an explicit
    "bundle too large" error from a size check, not rely on the daemon's reply
    being refused downstream.
- Behaviour must be identical for the AOSP integration and the root module:
  everything new rides the existing authenticated daemon protocol.
- This machine has no cargo or Flutter SDK and the working tree cannot be built
  locally. Verification is by re-reading, unit tests in the daemon's existing
  `#[cfg(test)]` suites, Flutter widget tests, and CI
  (`rust-check.yml`, `flutter-tests.yml`, `build-dash-essential-module.yml`).

## Approaches

- **A (chosen): host-assembled bundle, SAF picker, daemon replay.** Dart
  collects the UI prefs, the daemon returns its own durable state through a new
  read command, the host merges them into one JSON and `DashActivity` writes or
  reads it with `ACTION_CREATE_DOCUMENT` / `ACTION_OPEN_DOCUMENT`. Import plays
  back through the existing validated command paths. Every privileged file
  stays where it already lives; the daemon never touches user-visible storage.
- **B (rejected): root daemon owns the file.** Daemon reads and writes
  `/storage/emulated/0/Download` directly. Less UI code, but the daemon would
  reach into public user storage as root — new SELinux rules for the AOSP
  `dash_daemon` domain, which its `dash_daemon.rc` groups do not grant — and
  behaviour would diverge between deployments.
- **C (rejected): per-file manual copy.** Four stores, no validation, and a
  stale `state.conf` is exactly the dangerous case.

One sub-decision inside A: how a large bundle crosses the 4096-byte command cap.
Raising the framed cap globally weakens the daemon's read bound for every
command, and the daemon cannot know the command before it reads the body, so
the cap stays. The bundle is uploaded as bounded chunks followed by an apply
command (chunked upload, below).

## How it is built

### Bundle format

```json
{
  "schema": 1,
  "app": { "version": "1.23.12", "platform": "dash" },
  "ui": {
    "appearance": { "…": "DashAppearanceConfig.toJson() today" },
    "interaction": { "motionSpeed": 1.0, "haptics": true }
  },
  "hardware": {
    "state": ["key=value", "…one entry per PERSISTED_FIELDS row"],
    "profiles": { "…app-profiles.json contents…": "" },
    "conditionals": { "…conditionals.json contents…": "" },
    "dexopt": { "…dexopt.json contents…": "" }
  }
}
```

`state` is an array of the exact `key=value` lines `persisted_state_text`
emits, so import reuses `parse_persisted_state` and its `PERSISTED_FIELDS`
validation field by field: an unknown or unparsable line keeps the current
value instead of replacing a good default, exactly as it does at boot today.

### Export

1. Dart calls the framed exchange with `GET save.bundle`.
2. Daemon handler builds `hardware`: `state` from `persisted_state_text` on the
   live state, `profiles` from `state_dir()/app-profiles.json`,
   `conditionals` from `conditionals.json`, `dexopt` from `dexopt.json`. Each
   missing file is reported as absent rather than failing the export. If the
   assembled JSON would exceed a 700,000-byte budget — set below the host
   bridge's 786,432-byte inbound read (`backend_bridge.rs:550-553`, itself
   below Dart's 1 MiB buffer) — the command answers
   `ERR export bundle too large` instead of letting the host truncate the reply
   and fail the JSON parse.
3. Dart merges in `ui` (existing `DashAppearanceConfig.toJson` plus the
   interaction settings the class already persists) and the header, then passes
   the text to a new host FFI `dash_host_backup_export(ptr, len)`.
4. `DashActivity` writes the file through the existing **MediaStore.Downloads**
   path — the same mechanism `saveFpsExport` already uses, with no picker
   (`ACTION_CREATE_DOCUMENT` was rejected to keep the local save zero-friction)
   — suggested name
   `dash-essential-backup-<version>-<YYYYMMDDHHMMSS>.json`, and reports the
   outcome through `nativeBackupResult`.
5. The completion UI attaches a share action: `ACTION_SEND` for
   `application/json` through `Intent.createChooser`, so the file can go to
   Drive or any other transport when moving to another device.
6. The host keeps a small status slot; Dart polls the new
   `dash_host_backup_state()`, the same polling pattern the photo permission
   flow already uses, and shows the result plus the share action.

### Import

1. `dash_host_backup_import()` makes `DashActivity` open `ACTION_OPEN_DOCUMENT`
   for `application/json`, read the bytes, and pass them back through
   `nativeBackupResult`.
2. Dart polls for readiness, pulls the text out through a bounded
   copy-out FFI modelled on `dash_backend_app_controls_exchange`, checks
   `schema == 1` and `app.platform == "dash"` locally (reject early, before any
   daemon traffic), splits `ui` off, and uploads `hardware` in chunks:
   `SET import.chunk <index> <part>` (chunk ≈ 3000 bytes so the whole command
   stays under 4096), then `ACTION import.apply`.
3. The daemon accumulates chunks into a bounded (1 MiB) pending buffer — index
   0 resets any abandoned partial upload, and a stale buffer is dropped when a
   new upload starts — and `ACTION import.apply`. Because
   `handle_command_inner` already holds `GATE.read()` for everything that
   reaches `handle_enabled_command` (`lib.rs:8764`), `import.apply` must be a
   new top-level dispatch branch in `handle_command_inner` that takes
   `GATE.write()` first — the same structure as `app_runtime::command` and
   `subsystem_clocks::command` — never an arm inside
   `handle_enabled_command`, which would deadlock against the held read lock.
   Under that write lock it:
   a. re-validates schema and platform, rejecting a mismatch outright;
   b. releases per-app control and resets per-app profiles through the existing
      `app_runtime::reset_all()`, then parses the imported `profiles` with the
      existing `Config` parser, and *both* saves it as `app-profiles.json` and
      sets the live runtime's in-memory `r.config` to it. `reset_all()` sets
      `r.config` to the default and nothing reloads it afterwards
      (`app_runtime.rs:1562-1570`), so a step that writes only the file would
      leave the live foreground watcher applying the old config until the
      daemon restarts;
   c. replaces the conditional stack through the existing parse path, then sets
      the live `r.stack` the same way, **and** validates every imported
      conditional against the live capabilities the way `upsert` does
      (`app_runtime.rs:1437-1440`) — `Stack::load` silently drops bad entries
      (`conditional.rs:121-139`), so a replace built straight from
      `Conditional::parse` would let the store hold controls this device
      cannot take, contradicting the revalidation goal;
   d. applies the imported `dexopt` schedule through the existing schedule
      parser;
   e. builds `PersistedState` by parsing the imported `state` lines with the
      existing `parse_persisted_state` (so every field is validated), saves it
      with `save_persisted_state`, then replays the boot-restore path
      (`restore_persisted_state()`, `reassert_runtime_state(true)`,
      `reassert_persisted_governors()`), and finally applies the imported
      charging profile through the same call `SET charging` uses
      (`apply_charging_profile`, `lib.rs:8885`). This last call is required:
      `restore_persisted_state` applies charging only behind the boot
      one-shot `CHARGING_BOOT_RESTORE_DONE` (`lib.rs:5141-5148`), so without
      it the imported charging setting stays dormant until the next boot;
   f. drops the pending buffer and answers with a per-domain result
      `OK {"state":{"ok":true,"rejected":["bad_line"]},"profiles":{"ok":true},…}`,
      where the rejected-line list comes from a new parse variant that reports
      unknown or unparsable lines — `parse_persisted_state` skips them
      silently (`lib.rs:4197-4230`) and cannot name them today.
4. Dart writes the `ui` block into `dash-appearance.json` and
   `dash-interaction.json` through **new public replace entry points**: the
   existing savers are private (`_persistAppearance`, `main.dart:506`;
   `DashInteractionSettings._persist`, `main.dart:16659`) and
   `DashInteractionSettings.load()` returns early once `_loaded` is set
   (`main.dart:16659-16664`), so neither can be reused as-is. The new functions
   write the imported values and force the in-memory config to them, then the
   result sheet reports the per-domain outcome.

### Appearance normalization on import

`DashAppearanceConfig.customPath` is normalized before the `ui` block is
written, because a path carried to a new phone names a file that is not there:

- If the value is empty, keep it.
- Resolve it as it is stored today: a plain filesystem path is checked with
  `File.existsSync()`, and a `content://` value is checked through the
  ContentResolver (the host already bridges the photo-permission and
  `saveFpsExport` flows into `DashActivity`, so the resolver is reachable from
  the same class, not a new one).
- Exists → keep the value verbatim (same-device reinstall or persistent shared
  storage).
- Missing → prune it (empty value) so the background falls back to the default
  style, and record the prune in the import result sheet so the user knows why
  their wallpaper did not come across.

### Partial failure

Apply every valid domain and report exactly which failed and why. Aborting a
replace-everything import midway leaves the phone on a blank slate, which is
worse than a working transfer minus one cluster range. Inside `state`, lines
rejected by the field parser keep the current value and are named in the reply.

### Files touched

- `runtime/daemon-rust/src/lib.rs` — `save_bundle` (with the 700 KB budget),
  `parse_persisted_state` variant that reports rejected lines, the chunk
  accumulator, the `import_apply` handler behind a new **top-level**
  `GATE.write` branch in `handle_command_inner`, and the per-domain result
  builder.
- `runtime/daemon-rust/src/app_runtime.rs` — reuse `reset_all`, `Config`
  load/save and the capability validation `upsert` performs; the import path
  must also set the live `r.config` and `r.stack`, not only write the files.
- `runtime/daemon-rust/src/conditional.rs`, `dexopt.rs` — replace-from-object
  helpers built on their existing parsers, with imported conditionals checked
  against live capabilities before they enter the store.
- `runtime/daemon-rust/src/diagnostics.rs` — one new area constant alongside
  the existing eight (`AREA_GPU` … `AREA_CONFLICT`, `diagnostics.rs:29-36`),
  e.g. `AREA_BACKUP = "backup"`. There is no `command.*` precedent, so the
  successful-import journal entry follows the area pattern instead.
- `runtime/host-rust/src/backend_bridge.rs` — `GET save.bundle`,
  `SET import.chunk`, `ACTION import.apply` whitelist entries; backup status
  slot and the small FFI surface for export/import state and payload copy-out.
- `runtime/host-rust/src/platform_bridge.rs` — JNI bridge to the new
  `DashActivity` methods.
- `android/platform/io/github/neeschal/dashessential/DashActivity.java` — SAF
  create/open methods and the `nativeBackupResult` callback. No new
  components, and everything added must stay inside the 40,960-byte DEX cap
  `tools/check-platform-dex.sh` enforces.
- `ui/flutter/lib/main.dart` — Settings section, UI-pref collection, polling,
  chunked upload, result sheet, and the public replace entry points for
  appearance and interaction settings.
- Daemon unit tests: bundle round-trip; platform mismatch refusal; corrupt
  `state` line isolation with the rejected-line report; oversized chunk
  rejection; abandoned-upload reset. Flutter widget test for the Settings
  section. Existing tests must keep passing; the platform DEX whitelist check
  must keep passing unchanged.

## Left to the builder

- Confirm the exact `Config` parser entry points in `app_controls.rs` accept a
  JSON object (they serialize to `profiles.json`-shaped JSON today) and reuse
  them rather than re-parsing.
- Decide chunk size from measured bundle size once profiles exist; the live
  phone's bundle is already tiny (state.conf 731 B, app-profiles.json 347 B),
  so 3000-byte chunks keep the whole command under 4096 with a single chunk in
  the common case.
- Wiring the import result sheet into the existing
  `diagnostics_logs`/feedback presentation rather than a bespoke dialog, if the
  existing one fits.
- New Settings labels are Dart literals in `main.dart`, like every other
  Settings label; `android/package/res/values/strings.xml` holds only
  `app_name` and the accessibility strings, so there is no Java string-table
  convention to follow.
- Whether to record a successful import in the daemon diagnostics journal
  (recommended: a `backup`-area entry, consistent with the area pattern every
  other control uses).
