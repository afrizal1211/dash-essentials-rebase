# Settings Backup and Restore Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export all restorable Dash Essential settings to one JSON file and import it on another Dash phone, replacing everything.

**Architecture:** Host-assembled bundle. The daemon exposes `GET save.bundle` (its durable state as JSON) and a chunked import upload (`SET import.chunk` … `ACTION import.apply`) that replays through the existing validated code paths; Dart collects the UI prefs, merges them into the bundle, and `DashActivity` writes or reads the file through MediaStore / Storage Access without any new Flutter dependency or DEX class.

**Tech Stack:** Rust (daemon + host crates), JNI, Java (one existing Activity class), Dart/Flutter (existing single-`main.dart` app).

**Spec:** `docs/superpowers/specs/2026-11-15-settings-backup-design.md`

## Global Constraints

- No new Flutter dependencies; `pubspec.yaml` stays `flutter`, `material_color_utilities: 0.13.0`, `flutter_test`.
- `android/platform` keeps exactly the five classes `tools/check-platform-dex.sh` enforces, and `classes.dex` stays ≤ 40,960 bytes: all new Java is methods on `DashActivity`.
- Daemon commands are capped at 4096 bytes inbound (framed `@<len>\n<cmd>`, `lib.rs:9149`); a larger frame is dropped without a reply. The host bridge reads at most 786,432 bytes of a reply (`backend_bridge.rs:550-553`).
- `ACTION import.apply` must be a top-level `GATE.write` branch in `handle_command_inner` (the existing `handle_enabled_command` runs under a held `GATE.read`, `lib.rs:8764`, and would deadlock).
- Import is replace-everything with per-domain partial failure: apply every valid domain, report each failure and every rejected line; never abort midway.
- A bundle whose `app.platform` is not `"dash"` is refused before any state is touched.
- **This workspace has no cargo/rustc/flutter/dart toolchain.** No task may claim a build or test result unless the command was actually run. Daemon tests are written for developers who can run them; CI runs only `cargo fmt --all -- --check`, `cargo clippy -p dash-essential-daemon --all-targets -- -D warnings` (`rust-check.yml`), `flutter test --no-pub --coverage` and `flutter analyze --no-fatal-infos` (`flutter-tests.yml`).

## Review Focus

1. **Bundle from another platform** (`app.platform != "dash"`, or `schema != 1`) — refused with an error before any hardware, daemon file, or UI state changes. Pinned by Task 9's early-reject test and Task 5's refusal test.
2. **A corrupt `state` line** (`gpu_max_freq_mhz=abc`, or an unknown key) — the current value is kept, and the exact line is named in the import result. Pinned by Tasks 1 and 5.
3. **An oversized or out-of-order chunk** — the pending upload is reset and the import fails loudly; the Dart sender treats a connection reset as "command too large" rather than a daemon crash. Pinned by Task 3.
4. **`customPath` naming a file that is not on this device** — pruned to the empty string so the background falls back to the default style, and the prune is reported in the result sheet. Pinned by Task 9.
5. **A bundle missing `ui` or `hardware`** — nothing is applied on either side; the user sees a clear single error instead of a half-replaced phone. Pinned by Tasks 5 and 9.

## Bundle format (single source of truth for all tasks)

Exported file, assembled on the Dart side:

```json
{
  "schema": 1,
  "app": { "version": "1.23.12", "platform": "dash" },
  "ui": { "appearance": { "…": "DashAppearanceConfig.toJson() verbatim" },
          "interaction": { "motionModelVersion": 3, "motionSpeed": 1.0,
                           "hapticsEnabled": true } },
  "hardware": { "state": ["key=value", "…"], "profiles": { "…": "…" } ,
                "conditionals": { "…": "…" }, "dexopt": { "…": "…" } }
}
```

`GET save.bundle` returns the `hardware` object only. `ui` is read and written
by Dart alone, so the daemon never owns UI state. Each hardware domain is
`null` when its durable file does not exist.

Import reply: `OK ` followed by

```json
{ "state": { "ok": true, "rejected": ["bad_line"] },
  "profiles": { "ok": false, "error": "…" },
  "conditionals": { "ok": true, "rejected": ["name"] },
  "dexopt": { "ok": true } }
```

---

### Task 1: Rejected-line reporting for the persisted-state parser

**Files:**
- Modify: `runtime/daemon-rust/src/lib.rs` (`parse_persisted_state`, ~4197-4230; `#[cfg(test)]` module ~9340)
- Test: same file, `#[cfg(test)]`

**Interfaces:**
- Produces: `fn parse_persisted_state_reported(raw: &str) -> (PersistedState, Vec<String>)` in `lib.rs`, where the `Vec` lists every non-comment, non-empty `key=value` line the field table could not consume, verbatim. `parse_persisted_state(raw)` becomes `parse_persisted_state_reported(raw).0`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn rejected_state_lines_are_reported_and_isolated() {
    let raw = "gpu_max_freq_mhz=abc\nunknown_key=1\ncharging=5\ntouch=2\n";
    let (state, rejected) = super::parse_persisted_state_reported(raw);
    assert_eq!(rejected, vec!["gpu_max_freq_mhz=abc", "unknown_key=1"]);
    assert_eq!(state.charging, 5);
    assert_eq!(state.touch, 2);
}

#[test]
fn comments_and_blank_lines_are_not_rejections() {
    let (_state, rejected) = super::parse_persisted_state_reported("# comment\n\n");
    assert!(rejected.is_empty());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dash-essential-daemon parse_persisted_state_reported`
Expected: FAIL, `cannot find function`.

- [ ] **Step 3: Implement `parse_persisted_state_reported`**

Same loop as today; when no `PERSISTED_FIELDS` row accepts a line (unknown key
or `field.parse` returning `None`), push the trimmed raw line into the
rejection vector. Keep the current "a bad line never replaces a good default"
behaviour exactly. Refactor `parse_persisted_state` into a wrapper.

- [ ] **Step 4: Run the daemon tests and lints**

Run: `cargo test -p dash-essential-daemon` and `cargo clippy -p dash-essential-daemon --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 5: Commit**

```bash
git add runtime/daemon-rust/src/lib.rs
git commit -m "feat(daemon): report rejected persisted-state lines during parse"
```

---

### Task 2: `GET save.bundle` — daemon-side export

**Files:**
- Modify: `runtime/daemon-rust/src/lib.rs` (new `fn save_bundle()`, dispatch in `handle_enabled_command` ~8787)
- Modify: `runtime/host-rust/src/backend_bridge.rs` (exchange whitelist ~505-541)
- Test: `runtime/daemon-rust/src/lib.rs`, `#[cfg(test)]`

**Interfaces:**
- Consumes: `persisted_state()`, `persisted_state_text`, `state_dir()`, `app_controls::Config::load`, `conditional`/`dexopt` file layout (`conditionals.json`, `dexopt.json`).
- Produces: `fn save_bundle() -> Result<serde_json::Value, String>` returning the `hardware` object: `state` as the array of lines `persisted_state_text` emits; `profiles` as `Config::load(&state_dir().join("app-profiles.json")).map(|c| c.json())`, `null` when the file is missing; `conditionals`/`dexopt` as the parsed `serde_json::Value` of their files, `null` when missing, `Err` when a present file is unreadable. Wire command `GET save.bundle` answering `OK {json}`.

- [ ] **Step 1: Write the failing test**

`state_dir()` resolves through a `OnceLock` (`lib.rs:4174-4183`) shared by
every test in the binary, so a test that calls `std::env::set_var` in-process
races every other test and may still see the default
`/data/adb/dash-essential`. Daemon tests that need a state directory must
therefore follow the existing child-process pattern the crash tests use
(`run_crash_test`, `lib.rs:10045`): spawn `std::env::current_exe()` with
`--exact <test name>` and `.env(STATE_DIR_ENV, &directory)`, and have the
child perform the assertions and exit non-zero on failure. Write the test in
that shape, rooted under a unique `std::env::temp_dir()` directory per child.

Child assertions: with `state.conf` = `charging=4\ntouch=2\n` and
`app-profiles.json` = `{"schema":1,"enabled":false,"notify":false,"profiles":[]}`,
`save_bundle()` answers a `Value` whose `state` array is
`["charging=4","touch=2"]`, whose `profiles` is the canonical parsed object,
whose `conditionals` and `dexopt` are `Value::Null`, and whose `state` lines
round-trip through `parse_persisted_state_reported` back to the same
`PersistedState` fields.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p dash-essential-daemon save_bundle`
Expected: FAIL, `cannot find function`.

- [ ] **Step 3: Implement `save_bundle` and dispatch**

Assemble the object; then, before returning, serialize it and answer
`Err("export bundle too large".into())` when the serialized text exceeds
700,000 bytes — set below the host bridge's 786,432-byte inbound read so the
export fails with a real error instead of a truncated reply the host cannot
parse. Dispatch as a `cmd == "GET save.bundle"` arm in
`handle_enabled_command`, next to `GET snapshot`: it is a read, so the
`GATE.read` the caller already holds is the right lock. The enabled-switch
check in `handle_command_inner` only guards `SET`/`ACTION`, so like every
other `GET` this answers whether or not the service is enabled — a read of
durable state, exactly like `GET snapshot` today.

- [ ] **Step 4: Whitelist the command in the host bridge**

Add `|| command == "GET save.bundle"` to the prefix list in
`dash_backend_app_controls_exchange` (`backend_bridge.rs`), which is the only
framed path Dart can call. The ordinary `request()` path is not extended.

- [ ] **Step 5: Run the tests and lints**

Run: `cargo test -p dash-essential-daemon`, `cargo clippy -p dash-essential-daemon --all-targets -- -D warnings`, `cargo fmt --all -- --check`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add runtime/daemon-rust/src/lib.rs runtime/host-rust/src/backend_bridge.rs
git commit -m "feat(daemon): return durable state as a bundle for export"
```

---

### Task 3: Chunked import upload — `SET import.chunk`

**Files:**
- Modify: `runtime/daemon-rust/src/lib.rs` (new accumulator + dispatch arm)
- Modify: `runtime/host-rust/src/backend_bridge.rs` (whitelist)
- Test: same file, `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `struct ImportUpload { next_index: usize, bytes: Vec<u8> }`, `static IMPORT_UPLOAD: std::sync::Mutex<Option<ImportUpload>>` in `lib.rs`, capped at 1 MiB.
  - `fn import_chunk(raw: &str) -> Result<(), String>`: `raw` is `<index> <part>`. Index 0 starts an upload and discards any abandoned partial buffer; every later index must equal `next_index`, else `Err("import chunk out of order")`. Appending past 1 MiB answers `Err("import chunk too large")` and drops the buffer, so a poisoned upload can never block a fresh one.
  - Wire command `SET import.chunk <index> <part>`, dispatched as a top-level branch in `handle_command_inner` (see Step 3 for why it must not ride the `SET` gate).

- [ ] **Step 1: Write the failing tests**

Chunk arithmetic is pure process state, so these run in-process; each resets
the accumulator on completion so parallel tests never share a buffer:

```rust
#[test]
fn chunks_accumulate_in_order() {
    super::import_chunk("0 AAAA").unwrap();
    super::import_chunk("1 BBBB").unwrap();
    let guard = super::IMPORT_UPLOAD.lock().unwrap();
    assert_eq!(guard.as_ref().unwrap().bytes, b"AAAABBBB");
    assert_eq!(guard.as_ref().unwrap().next_index, 2);
}

#[test]
fn a_new_upload_drops_an_abandoned_one() {
    super::import_chunk("0 AAAA").unwrap();
    super::import_chunk("2 BBBB").unwrap_err();
    super::import_chunk("0 CCC").unwrap();
    let guard = super::IMPORT_UPLOAD.lock().unwrap();
    assert_eq!(guard.as_ref().unwrap().bytes, b"CCC");
}

#[test]
fn an_oversized_upload_is_refused_and_reset() {
    super::import_chunk("0 AAAA").unwrap();
    let big = "B".repeat(600_000);
    super::import_chunk(&format!("1 {big}")).unwrap_err();
    assert!(super::IMPORT_UPLOAD.lock().unwrap().is_none());
}
```

Each of these runs in-process and must reset the accumulator on completion so
parallel tests never share a buffer.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dash-essential-daemon import_chunk`
Expected: FAIL, `cannot find`.

- [ ] **Step 3: Implement the accumulator and the dispatch arm**

Chunk payloads are UTF-8 JSON fragments; the whole command stays under 4096
bytes because Dart splits at 3000 bytes. `take()` on the buffer (used by
`import.apply`) resets it.

Dispatch `SET import.chunk` as a **top-level** branch in
`handle_command_inner`, placed with the `GET logs` block and *before* the
`SET`/`ACTION` gate: that gate runs `check_global_command`, the enabled check,
and `service_control::set_intent(true, true)` for unrecognized `SET` commands
(`lib.rs:8771-8782`) — none of which belongs around writing a buffer. The arm
holds no gate at all; `import.chunk` touches no hardware and no durable file,
so neither `GATE.read` nor `GATE.write` is needed.

- [ ] **Step 4: Whitelist the command**

Add `|| command.starts_with("SET import.chunk ")` to the exchange prefix list.

- [ ] **Step 5: Run the tests and lints**

Run: `cargo test -p dash-essential-daemon`, `cargo clippy -p dash-essential-daemon --all-targets -- -D warnings`, `cargo fmt --all -- --check`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add runtime/daemon-rust/src/lib.rs runtime/host-rust/src/backend_bridge.rs
git commit -m "feat(daemon): accept a chunked import upload"
```

---

### Task 4: app_runtime replace helpers (profiles and conditionals)

**Files:**
- Modify: `runtime/daemon-rust/src/app_runtime.rs` (new `apply_config`, `replace_conditionals`; `#[cfg(test)]` module if absent — check the file's existing tests first and follow their style)
- Consumes: `conditional::Conditional::parse`, `conditional::Stack`, `Capabilities::validate`, `reconcile`, `wake`, `release`, `state_dir()`.

**Interfaces:**
- Produces for Task 5:
  - `pub(super) fn apply_config(config: Config) -> Result<(), String>` — saves `config` to `app-profiles.json`, sets `r.config`/`r.config_error`, runs `reconcile(&mut r, Wake::Changed)` and `wake(Wake::Changed)` exactly the way the tail of `command()` does after a `SET app.profile`. This is the piece `reset_all()` (1562-1570) is missing: it only ever sets the default and nothing reloads the file afterwards.
  - `pub(super) fn replace_conditionals(items: &serde_json::Value) -> Result<serde_json::Value, String>` — builds a fresh `Stack` from the `conditionals` array: each entry goes through `Conditional::parse` and `Capabilities::validate` the way `Stack::upsert` does (`conditional.rs:190-205`); entries that fail are skipped and named in the reply, which is the only way a replace can keep the design's revalidation promise, because `Stack::load` drops bad entries silently. Returns `{"ok":true,"rejected":[names]}`. On success saves and assigns `r.stack`, reconciles and wakes. A parse failure of the container answers `Err`, which the caller reports as a failed domain.

- [ ] **Step 1: Write the failing tests**

These touch real files under the state directory, so they must run as
child-process tests with their own `DASH_STATE_DIR` exactly as Task 2
describes — `state_dir()` is a process-wide `OnceLock` and an in-process
`set_var` races every other test.
then assert the file parses back through `Config::load` and the runtime
`r.config` reflects it; `replace_conditionals` on
`[{"name":"n","profile":{…a profile that needs a control the default
Capabilities lack…}}]` returns the name in `rejected`; on a fully valid array,
`r.stack.list()` contains it and `conditionals.json` exists.
- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dash-essential-daemon apply_config` / `replace_conditionals`
Expected: FAIL, `cannot find`.

- [ ] **Step 3: Implement both helpers**

Mirror the existing `SET conditional.save` arm (`app_runtime.rs:1455-1470`)
for the stack path, minus the `upsert` merge semantics — an import **replaces**
the stack, so no name-based replacement, no `MAX_CONDITIONALS` growth check
beyond `take(MAX_CONDITIONALS)` on the imported array.

- [ ] **Step 4: Run the tests and lints**

Run: `cargo test -p dash-essential-daemon`, `cargo clippy -p dash-essential-daemon --all-targets -- -D warnings`, `cargo fmt --all -- --check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add runtime/daemon-rust/src/app_runtime.rs
git commit -m "feat(daemon): replace per-app profiles and the conditional stack"
```

---

### Task 5: `ACTION import.apply` — the replace-everything handler

**Files:**
- Modify: `runtime/daemon-rust/src/lib.rs` (new `fn import_apply() -> Result<Value, String>`; new top-level branch in `handle_command_inner` ~8724)
- Modify: `runtime/daemon-rust/src/diagnostics.rs` (`AREA_BACKUP`)
- Modify: `runtime/host-rust/src/backend_bridge.rs` (whitelist)
- Test: same files.

**Interfaces:**
- Consumes: Task 1 `parse_persisted_state_reported`; Task 3 `IMPORT_UPLOAD.take()`; Task 4 `apply_config`/`replace_conditionals`; `app_controls::Config::parse`; `dexopt::set_schedule`; `save_persisted_state`, `mutate_persisted_state`, `restore_persisted_state`, `reassert_runtime_state`, `reassert_persisted_governors`, `apply_charging_profile`.
- Produces: wire command `ACTION import.apply` answering `OK <per-domain json>` or `ERR <reason>`.

- [ ] **Step 1: Write the failing tests**

`import_apply` writes the state directory, so each of these runs as a
child-process test with its own `DASH_STATE_DIR` (the `OnceLock` rule from
Task 2). Each child runs exactly one test, so that child stages its chunks
and calls `import_apply` in-process; the `assert!` on `state.conf` reads the
child's own directory.

```rust
#[test]
fn import_refuses_a_foreign_platform() {
    let bundle = serde_json::json!({"schema":1,"app":{"platform":"rodin"},
        "hardware":{"state":[],"profiles":null,"conditionals":null,"dexopt":null}});
    // stage the upload, then apply
    super::import_chunk(&format!("0 {}", bundle.to_string())).unwrap();
    let error = super::import_apply().unwrap_err();
    assert_eq!(error, "bundle is not from this platform");
    // nothing was applied: the pending buffer is gone and state.conf is absent
    assert!(super::IMPORT_UPLOAD.lock().unwrap().is_none());
    assert!(!directory.join("state.conf").exists());
}

#[test]
fn import_applies_state_and_names_rejected_lines() {
    // state ["charging=4","touch=2","gpu_max_freq_mhz=abc"], profiles null
    // -> state.conf contains charging=4/touch=2 and no gpu_max_freq_mhz line,
    //    the reply names gpu_max_freq_mhz=abc, and the live persisted state
    //    (persisted_state().lock()) carries charging=4.
}

#[test]
fn import_reports_a_failed_domain_and_still_applies_the_rest() {
    // profiles intentionally invalid (wrong schema), state valid
    // -> profiles ok:false with an error, state ok:true
}
```

Use a per-test temp `DASH_STATE_DIR` as in Task 2. `import_apply` must answer
`Err("no import upload pending")` when the buffer is empty.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p dash-essential-daemon import_`
Expected: FAIL, `cannot find`.

- [ ] **Step 3: Add `AREA_BACKUP`**

In `diagnostics.rs`, alongside the eight existing areas (29-36):

```rust
pub const AREA_BACKUP: &str = "backup";
```

- [ ] **Step 4: Implement `import_apply` and the top-level branch**

Order matters and each domain is independent: a domain failure is recorded in
the reply and the next domain still runs.

1. `IMPORT_UPLOAD.take()` (empty → `Err("no import upload pending")`), parse the JSON, require `schema == 1` and `app.platform == "dash"`, else `Err("bundle is not from this platform")` *before any state is touched*, and drop the buffer.
2. Refuse when `!service_control::enabled()` with the existing string `Dash Essential is disabled; enable it in Settings`.
3. **profiles**: `Config::parse` on the serialized `profiles` object (or `Config::default()` when it is `null`) → `app_runtime::reset_all()` to release any borrowed control and clear the profiles → `app_runtime::apply_config(config)`. `reset_all` must run before `apply_config` so a held control is released before new profiles apply.
4. **conditionals**: `app_runtime::replace_conditionals(&hardware["conditionals"])`, where `null` means an empty stack.
5. **dexopt**: `dexopt::set_schedule(&value.to_string())` when non-`null`; `set_schedule` already validates cadence, day, and the opt-out list and answers `Err`. `null` leaves the schedule alone (an absent file on the exporter was never a setting).
6. **state**: join `state` lines with `\n`, `parse_persisted_state_reported`, then `mutate_persisted_state(|s| *s = parsed)` — this is the exact path a `SET` takes, so the maintenance-guard `transient_write` rule and the atomic write are reused rather than reinvented. Then `restore_persisted_state()`, `reassert_runtime_state(true)`, `reassert_persisted_governors()`, and the charging profile applied live:

```rust
if state.bypass_charging != 1 && valid_charging_profile(state.charging) {
    let _ = apply_charging_profile(state.charging);
}
```

without the `CHARGING_BOOT_RESTORE_DONE` one-shot, which is boot-only
(`lib.rs:5141-5148`) and would otherwise leave the imported profile dormant
until the next reboot.

7. `diagnostics::record(diagnostics::AREA_BACKUP, "info", "import", &summary)` on success, mirroring the area pattern every other control uses.
8. Answer `Ok(json!({ "state": {…}, "profiles": {…}, "conditionals": {…}, "dexopt": {…} }))`.

Dispatch — new top-level branch in `handle_command_inner` placed with the
other write-gated branches, mirroring `subsystem_clocks::command`:

```rust
if cmd == "ACTION import.apply" {
    let _gate = match service_control::GATE.write() {
        Ok(gate) => gate,
        Err(_) => return "ERR service transition lock poisoned".into(),
    };
    return match import_apply() {
        Ok(value) => format!("OK {value}"),
        Err(error) => format!("ERR {error}"),
    };
}
```

Never an arm inside `handle_enabled_command`: that function runs under a held
`GATE.read` and taking the write lock on the same thread deadlocks.

- [ ] **Step 5: Whitelist the command**

Add `|| command == "ACTION import.apply"` to the exchange prefix list.

- [ ] **Step 6: Run the tests and lints**

Run: `cargo test -p dash-essential-daemon`, `cargo clippy -p dash-essential-daemon --all-targets -- -D warnings`, `cargo fmt --all -- --check`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add runtime/daemon-rust/src/lib.rs runtime/daemon-rust/src/diagnostics.rs runtime/host-rust/src/backend_bridge.rs
git commit -m "feat(daemon): apply an imported settings bundle"
```

---

### Task 6: `DashActivity` backup methods (Java)

**Files:**
- Modify: `android/platform/io/github/neeschal/dashessential/DashActivity.java`
- No test file: no Java test target exists; verification is `tools/check-platform-dex.sh` in CI plus code review. Keep the added surface small — the 40,960-byte DEX cap is a hard budget.

**Interfaces:**
- Consumes: the existing `writeMedia` helper (~134) and `main`/`current` fields.
- Produces (public methods the host calls by name through JNI — signatures are the contract with Task 7):
  - `public void saveBackup(byte[] data)` — writes `dash-essential-backup-<millis>.json` into `MediaStore.Downloads` under `RedFox/Backup/` with mime `application/json`, reusing `writeMedia`; on success `nativeBackupResult(1, displayName.getBytes(StandardCharsets.UTF_8))` where `displayName` is the URI `writeMedia` returned (the share action and the completion message both need it), on failure `nativeBackupResult(2, null)`; a Toast like `saveFpsExport` shows.
  - `public void openBackup()` — `startActivityForResult(new Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("application/json"), BACKUP_REQUEST)`.
  - `@Override protected void onActivityResult(int request, int result, Intent data)` — for `BACKUP_REQUEST`: reads the returned content URI fully through `ContentResolver.openInputStream`, then `nativeBackupResult(3, bytes)`; result not OK or a null URI → `nativeBackupResult(5, null)` (cancelled); a read failure → `nativeBackupResult(4, null)`.
  - `public void shareBackup(String uri)` — `Intent.ACTION_SEND` with `setType("application/json")`, `putExtra(Intent.EXTRA_STREAM, Uri.parse(uri))` and `Intent.createChooser`, started outside the main thread restriction via `startActivity` from the activity (the caller is the host on a Flutter thread, so post to `main` like `saveFpsExport`'s toast does).
  - `public boolean backupPathExists(String path)` — `content://` → `getContentResolver().getType(Uri.parse(path)) != null` (a stream open is also acceptable); otherwise `new java.io.File(path).exists()`.
  - `private static native void nativeBackupResult(int code, byte[] data);` next to the existing `nativeBack` declaration.

- [ ] **Step 1: Add the methods**

One private `static final int BACKUP_REQUEST = 0x100;` distinct from any other
request code in the class. Read the stream with a bounded `ByteArrayOutputStream`
loop; a malformed bundle may be large, so cap the read at 4 MiB and answer
`nativeBackupResult(4, null)` past it.

- [ ] **Step 2: Verify the DEX constraints hold**

Run (on a machine with an Android SDK and the built APK):
`tools/check-platform-dex.sh <apk>`
Expected: `PLATFORM_BRIDGE_DEX=PASS (framework adapters only)`.
On this workspace the build cannot run; re-read the class and confirm no new
class or inner class was introduced and nothing outside `DashActivity` changed.

- [ ] **Step 3: Commit**

```bash
git add android/platform/io/github/neeschal/dashessential/DashActivity.java
git commit -m "feat(platform): backup export, import picker and share in DashActivity"
```

---

### Task 7: Host backup bridge (FFI slots and JNI)

**Files:**
- Modify: `runtime/host-rust/src/platform_bridge.rs` (new FFI next to `dash_host_export_save`, ~336; the JNI callback next to the `DashStatusNotificationService` native methods)
- Test: `runtime/host-rust/src/platform_bridge.rs`, `#[cfg(test)]` — slot state machine only; JNI cannot run off-device.

**Interfaces:**
- Consumes: `ACTIVITY`/`with_activity` (platform_bridge.rs:60-110), the same `attach_current_thread` + `with_local_frame` pattern `dash_host_export_save` uses.
- Produces (all `#[unsafe(no_mangle)] extern "C"`, mirrored in Task 8):
  - `dash_host_backup_export(ptr: *const u8, len: i32) -> i32` — 1 when the activity accepted the bytes (length sanity-checked exactly the way `dash_host_export_save` caps its inputs, with `MAX_BACKUP_LEN = 16 MiB`), 0 otherwise. Calls `saveBackup([B)`.
  - `dash_host_backup_import() -> i32` — calls `openBackup()`, 1/0.
  - `dash_host_backup_share(ptr, len) -> i32` — calls `shareBackup(String)`.
  - `dash_host_backup_path_exists(ptr, len) -> i32` — 1/0 via `backupPathExists`; an unreadable value answers 0 (prune), never blocks an import.
  - `dash_host_backup_state() -> i32` — reads an `AtomicI32`: `0` idle, `1` export ok, `2` export failed, `3` import ready, `4` import failed, `5` import cancelled. This is the polling slot the photo-permission flow (`dash_host_photo_permission_state`) established. The payload carries the export's display URI as UTF-8 in state `1`, and the bundle bytes in state `3`; Dart decodes accordingly.
  - `dash_host_backup_payload(ptr: *mut u8, capacity: i32) -> i32` — copies the imported bytes out, returning the length or -1, the same output-buffer contract `dash_backend_app_controls_exchange` uses.
  - `dash_host_backup_reset() -> i32` — clears the slot and the payload, called by Dart before and after each operation.
  - `Java_io_github_neeschal_dashessential_DashActivity_nativeBackupResult(_: JNIEnv, _: JClass, code: jint, data: JByteArray)` — static native matching Task 6's declaration; stores the code into the state slot and the bytes into a `Mutex<Vec<u8>>`. Import bytes are the authoritative copy; convert with `env.convert_byte_array`.

- [ ] **Step 1: Write the failing test for the slot semantics**

```rust
#[test]
fn backup_state_and_payload_round_trip() {
    unsafe { super::dash_host_backup_reset() };
    assert_eq!(super::dash_host_backup_state(), 0);
    super::store_backup_result(3, b"payload");   // hook the JNI callback uses
    assert_eq!(super::dash_host_backup_state(), 3);
    let mut out = vec![0u8; 64];
    let len = super::dash_host_backup_payload(out.as_mut_ptr(), out.len() as i32);
    assert_eq!(len, 7);
    assert_eq!(&out[..7], b"payload");
    unsafe { super::dash_host_backup_reset() };
    assert_eq!(super::dash_host_backup_state(), 0);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p dash_essential_backend backup_state` (crate name from `host-rust/Cargo.toml`)
Expected: FAIL, `cannot find`.

- [ ] **Step 3: Implement the slots and the FFI**

Put the state, payload, and `store_backup_result` behind one `Mutex`-guarded
struct so a reset cannot race a completing import. The JNI callback delegates
to it. Path/length checks mirror `dash_host_export_save` exactly.

- [ ] **Step 4: Run the tests and lints**

Run: `cargo test -p dash_essential_backend`, `cargo clippy -p dash_essential_backend --all-targets -- -D warnings`, `cargo fmt --all -- --check`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add runtime/host-rust/src/platform_bridge.rs
git commit -m "feat(host): backup status, payload and export/import FFI"
```

---

### Task 8: Dart backend bindings

**Files:**
- Modify: `ui/flutter/lib/backend/dash_backend.dart` (FFI lookups ~200-230, methods ~911-925, guarded `lib.providesSymbol` pattern for newer symbols)
- Modify: `ui/flutter/lib/backend/per_app_backend.dart` (no change needed if `command()` covers the new commands — verify the exchange symbol and 4096-byte input check, ~98-104)
- Test: `ui/flutter/test/backup_bundle_test.dart` (new, following `per_app_backend_test.dart`'s style)

**Interfaces:**
- Consumes: `PerAppBackend.instance.command(cmd)` (returns the decoded `data`, throws `StateError` with the daemon error), `DynamicLibrary.open('libdash_essential_host.so')`.
- Produces for Task 9, on `DashBackend`:
  - `int backupState()` → `dash_host_backup_state`
  - `bool backupExport(Uint8List bytes)` → `dash_host_backup_export`
  - `bool backupImport()` → `dash_host_backup_import`
  - `bool backupShare(String uri)` → `dash_host_backup_share`
  - `int backupPathExists` — `bool backupPathExists(String path)` → `dash_host_backup_path_exists`
  - `Uint8List? backupPayload()` → `dash_host_backup_payload` (allocates with the same `malloc`/`free` pair `per_app_backend.dart` uses, 1 MiB capacity)
  - `void backupReset()` → `dash_host_backup_reset`
  - `Future<Map<String, dynamic>> saveBundle()` → `PerAppBackend.instance.command('GET save.bundle')` cast to `Map<String, dynamic>`
  - `Future<Map<String, dynamic>> importApply(String hardwareJson)` → splits `hardwareJson` into ≤3000-byte chunks, `PerAppBackend.instance.command('SET import.chunk $i $part')` for each, then `command('ACTION import.apply')`; the whole command stays under 4096.

- [ ] **Step 1: Write the failing test**

A Dart unit test that builds a `hardware` JSON string, calls the chunk
splitting, and asserts no command exceeds 4096 bytes and the concatenation of
the parts reproduces the input. The chunk splitter must therefore be a small
pure function (`List<String> splitImportChunks(String json)`) the test can
reach, and `importApply` uses it.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd ui/flutter && flutter test test/backup_bundle_test.dart`
Expected: FAIL, `method not found`.

- [ ] **Step 3: Implement the lookups and the methods**

Follow the file's established pattern: `late` fields, `lookupFunction` guarded
by `lib.providesSymbol` for anything newer than the base table. The host side
of this surface is Task 7.

- [ ] **Step 4: Run the Dart tests and analyzer**

Run: `cd ui/flutter && flutter test --no-pub && flutter analyze --no-fatal-infos`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/flutter/lib/backend/dash_backend.dart ui/flutter/lib/backend/per_app_backend.dart ui/flutter/test/backup_bundle_test.dart
git commit -m "feat(ui): backup bindings and chunked import upload"
```

---

### Task 9: Settings UI, import/export flow, and appearance normalization

**Files:**
- Modify: `ui/flutter/lib/main.dart` (`_SettingsScreenState` ~2055; `_DashAppState` `_persistAppearance` ~506 and `_loadAppearance` ~462; `DashInteractionSettings` ~16600)
- Test: `ui/flutter/test/backup_settings_test.dart` (new)

**Interfaces:**
- Consumes: Task 8's `DashBackend` methods, `DashAppearanceConfig.fromJson/toJson`, `dashVersionName` from `ui/flutter/lib/release_identity.dart`.
- Produces:
  - A public `_DashAppState.applyAppearance(DashAppearanceConfig config)` — sets `_appearance.value`, calls the existing `_persistAppearance()`, and re-runs the side-effect tail of `_loadAppearance` (`DashThemeController.setPreference`, `_pushFpsOverlayStyle`, `_restoreFpsOverlay`, `_pushFpsRecording`, `_pushFpsAuto`, status notification). The existing savers are private and `load()` is once-gated by `_loaded`, so a replace needs this entry point.
  - A `DashInteractionSettings.replace({double motionSpeed, bool haptics})` static method — sets the fields, `_loaded = true`, bumps `revision`, and `_persist()`.
  - `String? _normalizeCustomPath(String raw, DashBackend backend)`: empty stays empty; `content://` answers `backupPathExists`; anything else `File(raw).existsSync()`. Missing → `''`, and the caller records the prune.
  - A Settings section: **Backup & transfer** with Export and Import entries, status polling (`backupState()` every 200 ms, the photo flow's cadence), and the result sheet.

- [ ] **Step 1: Write the failing widget test**

Pump `SettingsScreen` with a stubbed `DashBackend` whose `saveBundle` returns a
minimal `hardware` map and whose import FFI slots answer `backupState() == 3`
with a two-domain payload. Assert: tapping Import drives the chunk upload,
`applyAppearance` is reached with `customPath` pruned when `backupPathExists`
answers 0, and the result sheet lists the domains with a Share action on
export completion. Follow the existing tests'umping style
(`home_dashboard_test.dart`, `service_confirmation_test.dart`).

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd ui/flutter && flutter test test/backup_settings_test.dart`
Expected: FAIL, key methods missing.

- [ ] **Step 3: Implement the flow**

Export: `saveBundle()` → merge `{"schema":1,"app":{"version":dashVersionName,"platform":"dash"},"ui":{"appearance":…,"interaction":…},"hardware":…}` → `backupReset()` then `backupExport(utf8.encode(json))` → poll `backupState()` → on `1` decode `backupPayload()` as UTF-8 to get the file's URI and show the completion UI with a **Share** button that calls `backupShare(uri)`; on `2` an error. The share action (`ACTION_SEND` + `Intent.createChooser`) lives in Java (Task 6) because there is no share plugin and none may be added.

Import: `backupReset()` then `backupImport()` → poll `backupState()` → on `3`, `backupPayload()` → `utf8.decode` and `jsonDecode`:
1. **Local gate first, before any daemon traffic:** `schema == 1` and `app.platform == "dash"`, else abort with a clear message. `hardware` and `ui` both present and objects, else abort.
2. Normalize `ui.appearance.customPath` (above).
3. `importApply(jsonEncode(hardware))`; treat a `StateError` message containing a connection/reset indication as a transport failure, never as a silent success.
4. On the reply: write the `ui` block through `applyAppearance`/`DashInteractionSettings.replace`, then show the per-domain result sheet including `state.rejected` lines and the customPath prune.

- [ ] **Step 4: Run the Dart tests and analyzer**

Run: `cd ui/flutter && flutter test --no-pub && flutter analyze --no-fatal-infos`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/flutter/lib/main.dart ui/flutter/test/backup_settings_test.dart
git commit -m "feat(ui): settings backup export/import and appearance normalization"
```

---

### Task 10: End-to-end review and documentation

**Files:**
- Verify: every file touched; `docs/ARCHITECTURE.md` only if it enumerates protocol commands (check whether it lists commands; update if so).

- [ ] **Step 1: Re-read the whole diff against the spec**

Checklist: MediaStore export with no picker and no `ACTION_CREATE_DOCUMENT`;
share action on the completion UI; `ACTION_OPEN_DOCUMENT` import; customPath
existence check with prune and report; replace-everything; per-domain partial
failure; platform refusal; no new dependencies; no new DEX class.

- [ ] **Step 2: Run every check a developer would run**

```bash
cargo fmt --all -- --check
cargo clippy -p dash-essential-daemon --all-targets -- -D warnings
cargo clippy -p dash_essential_backend --all-targets -- -D warnings
cargo test -p dash-essential-daemon
cargo test -p dash_essential_backend
cd ui/flutter && flutter test --no-pub && flutter analyze --no-fatal-infos
```

None of these can run on this workspace; record the result of each one that was
actually executed and mark the rest "not run".

- [ ] **Step 3: Commit any doc touch-up**

```bash
git add .
git commit -m "docs: settings backup and restore"
```
