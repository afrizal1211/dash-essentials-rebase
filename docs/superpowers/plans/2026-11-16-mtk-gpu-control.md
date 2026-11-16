# MediaTek GPU and DDR Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a MediaTek Control screen to Dash Essential that pins the GPU clock (optionally with the table's own voltage), disables `thermal/gpt`, toggles FPSGO / FBT ceiling / Perfmgr / GED KPI / DCS, and writes a fixed DDR OPP — all through the daemon with journaling and readback.

**Architecture:** A new daemon module `mtk_control` shaped like `subsystem_clocks`: string commands, live kernel-node reads, `service_control::write` for every readable node, plain `fs::write` for the two write-only dvfsrc force nodes. A new Flutter `part` screen reads `GET mtk.control` and sends `ACTION mtk.*`.

**Tech Stack:** Rust (daemon, serde_json), Flutter (Dart, no new packages).

**Spec:** `docs/superpowers/specs/2026-11-16-mtk-gpu-control-design.md`

## Global Constraints

- No new Flutter dependencies, no new DEX classes, no new Android permissions.
- The daemon owns state; the kernel nodes are the truth and are read live.
- Use `service_control::write` for every node whose original can be read. Use plain `fs::write` only for `dvfsrc_force_vcore_dvfs_opp` and `dvfsrc_force_ddr_opp`, which are write-only.
- Never derive an OPP index from a formula or from the other OPP table.
- Never write a voltage the OPP table does not list for the chosen row.
- Never report success without a readback.
- Never leave a risky write applied after a failed verification.
- Never render a control whose node is absent.
- Driver values are written verbatim: frequency in kilohertz, voltage in the driver's own raw unit.
- Voltage and fixed DDR OPP can hard-lock the GPU/SoC; recovery is a reboot. The UI must say so.
- All UI text is English.
- `cargo clippy -p dash-essential-daemon --all-targets -- -D warnings` must stay clean; so must `cargo fmt --check`.
- Clocks: `MTK_GPUFREQ_DIR = "/proc/gpufreqv2"`, status poll budget 300 ms, poll interval 30 ms.

## Review Focus

- Same frequency in two OPP rows (338 MHz appears three times): the pin must use the lowest index, not the last.
- GPU idling below the table when the pin lands (`Freq: 26000` while index 51 is 338 MHz): verification compares the index, never the frequency alone.
- Service disable or daemon restart while a pin is active: `-1` and `0 0` must be written even though the journal may be empty.
- Write-only force node present on a kernel where it cannot be read: reset and release must still write `0`, and the UI must not claim a current value.
- Kernel with `/proc/gpufreqv2` but no `stack_signed_opp_table`, no `thermal/gpt`, or no dvfsrc helper: those controls disappear, nothing is written, nothing crashes.

---

### Task 1: Read the MediaTek driver and expose `GET mtk.control`

**Files:**
- Create: `runtime/daemon-rust/src/mtk_control.rs`
- Modify: `runtime/daemon-rust/src/lib.rs:28` (add `mod mtk_control;`)
- Modify: `runtime/daemon-rust/src/lib.rs:8966` (mount the command branch beside `subsystem_clocks`)
- Modify: `runtime/host-rust/src/backend_bridge.rs:499` (allowlist `GET mtk.control`)

**Interfaces:**
- Consumes: `super::read_node_text`, `service_control::enabled()`
- Produces:
  - `pub(super) fn is_command(cmd: &str) -> bool`
  - `pub(super) fn command(cmd: &str) -> Result<serde_json::Value, String>`
  - `struct OppRow { index: u32, khz: u32, volt: u32 }`
  - `fn parse_stack_opps(raw: &str) -> Vec<OppRow>`
  - `fn pin_index_for_khz(opps: &[OppRow], khz: u32) -> Option<u32>`
  - `fn parse_gpufreq_status(raw: &str) -> Option<(u32, u32)>` returning `(index, khz)`
  - `fn parse_dcs_menu(raw: &str) -> DcsMenu` with `struct DcsMenu { current: Option<u32>, entries: Vec<(u32, String)> }`; `current` is `Some(0)` only when the dump reports `T enable: 0`, otherwise `None`, because the kernel does not report the active code
  - `fn parse_dvfsrc_opp_table(raw: &str) -> Vec<u64>`
  - `fn dvfsrc_helper_dir() -> Option<PathBuf>`
  - `struct ControlInput { gpu: Option<GpuInput>, switches: [Option<bool>; 4], dcs: Option<DcsMenu>, ddr: Option<Vec<u64>> }`
    with `struct GpuInput { opps: Vec<OppRow>, pin: Option<(u32, u32)>, thermal_gpt: Option<bool> }`;
    `switches` is ordered `fpsgo, fbt_ceiling, perfmgr, ged_kpi`, and `thermal_gpt` is `Some(true)` when `thermal/gpt` reads `disable`
  - `fn render_control(input: Option<ControlInput>) -> Value`
  - `GET mtk.control` payload, exactly these keys:
    ```json
    {
      "supported": true,
      "reason": null,
      "soc": "mt6991",
      "gpu": {
        "opps": [{"index": 0, "khz": 1612000, "volt": 79500}],
        "pin": {"index": null, "khz": null},
        "thermal_gpt": {"present": true, "disabled": true}
      },
      "switches": {
        "fpsgo": {"present": true, "on": true},
        "fbt_ceiling": {"present": true, "on": false},
        "perfmgr": {"present": true, "on": false},
        "ged_kpi": {"present": false, "on": null}
      },
      "dcs": {"present": true, "code": 0, "menu": [[0, "disable"]]},
      "ddr": {"present": true, "readable": false, "opps": [1532000000]}
    }
    ```

- [ ] **Step 1: Write the failing tests**

In `mtk_control.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const STACK: &str = "[00*] freq: 1612000, volt:  79500, vsram:  79500\n\
                         [01*] freq: 1586000, volt:  79000, vsram:  79000\n\
                         [49*] freq:  442000, volt:  52500, vsram:  75000\n\
                         [50*] freq:  338000, volt:  49375, vsram:  75000\n\
                         [51*] freq:  338000, volt:  49375, vsram:  75000";

    #[test]
    fn parses_printed_index_frequency_and_raw_volt() {
        let rows = parse_stack_opps(STACK);
        assert_eq!(rows.len(), 5);
        assert_eq!((rows[0].index, rows[0].khz, rows[0].volt), (0, 1612000, 79500));
        assert_eq!((rows[4].index, rows[4].khz, rows[4].volt), (51, 338000, 49375));
    }

    #[test]
    fn duplicate_frequency_resolves_to_the_lowest_index() {
        let rows = parse_stack_opps(STACK);
        assert_eq!(pin_index_for_khz(&rows, 338000), Some(50));
        assert_eq!(pin_index_for_khz(&rows, 442000), Some(49));
        assert_eq!(pin_index_for_khz(&rows, 999000), None);
    }

    #[test]
    fn reads_the_gpu_opp_index_and_frequency_from_status() {
        let status = "[GPUFREQ-DEBUG] Current Status of GPUFREQ\n\
                      [GPU   OPP]      Index: 51, Freq:   26000, Volt:  49375, Vsram:  75000\n\
                      [STACK OPP]      Index: 51, Freq:   26000, Volt:  49000, Vsram:  75000";
        assert_eq!(parse_gpufreq_status(status), Some((51, 26000)));
    }

    #[test]
    fn parses_the_dcs_menu_and_current_code() {
        let menu = "DCS Policy is enable\n\
                    Current in use core num: 11\n\
                    T enable: 0 (0:disable 1:enable 2:enable with log)\n\
                    [1] Achieve 6/11 Freq --> 64\n\
                    Please echo [the min Code you want] for enable";
        assert_eq!(menu.current, Some(0));
        assert_eq!(menu.entries, vec![(1u32, "Achieve 6/11 Freq --> 64".to_string())]);
    }

    #[test]
    fn parses_the_dvfsrc_opp_table() {
        assert_eq!(
            parse_dvfsrc_opp_table("1532000000 2067000000 9594000000 9594000000 "),
            vec![1532000000, 2067000000, 9594000000, 9594000000]
        );
    }

    #[test]
    fn absent_kernel_reports_unsupported_with_a_reason() {
        let payload = render_control(None);
        assert_eq!(payload["supported"], false);
        assert!(payload["reason"].as_str().is_some_and(|r| !r.is_empty()));
    }

    #[test]
    fn a_kernel_without_a_ddr_force_node_hides_only_that_control() {
        let input = ControlInput {
            gpu: Some(GpuInput {
                opps: parse_stack_opps(STACK),
                pin: None,
                thermal_gpt: None,
            }),
            switches: [None; 4],
            dcs: None,
            ddr: None,
        };
        let payload = render_control(Some(input));
        assert_eq!(payload["supported"], true);
        assert_eq!(payload["gpu"]["thermal_gpt"]["present"], false);
        assert_eq!(payload["ddr"]["present"], false);
        assert_eq!(payload["gpu"]["opps"].as_array().unwrap().len(), 5);
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p dash-essential-daemon mtk_control`
Expected: FAIL — module and functions do not exist.

- [ ] **Step 3: Implement the parsers**

`mtk_control.rs` header comment states the two-table hazard and that
`fix_target_opp_index` indexes `stack_signed_opp_table`.

Implement:
- `parse_stack_opps`: per line, take the digits between `[` and `]` as
  `index`, the number after `freq:` as `khz`, the number after `volt:` as
  `volt`. Skip malformed lines.
- `pin_index_for_khz`: first row with a matching `khz`, in table order.
- `parse_gpufreq_status`: the line starting with `[GPU   OPP]`; take the
  digits after `Index:` and after `Freq:`; return `None` if either is missing.
- `parse_dcs_menu`: `current` is `Some(0)` when the dump contains
  `T enable: 0`, otherwise `None`; entries are `[<code>] <label>` lines.
  Return `DcsMenu { current: None, entries: vec![] }` when the menu is not
  recognised.
- `parse_dvfsrc_opp_table`: whitespace-separated integers, order preserved.
- `dvfsrc_helper_dir`: try, in order,
  `/sys/devices/platform/soc/1c00f000.dvfsrc`,
  `/sys/devices/platform/1c00f000.dvfsrc`,
  `/sys/devices/platform/1c013000.dvfsrc`,
  `/sys/devices/platform/soc/1c100000.dvfsrc`; return the first existing
  path (its basename is needed by the caller); else scan
  `/sys/devices/platform` for a `*.dvfsrc` entry.

Run: `cargo test -p dash-essential-daemon parse`
Expected: PASS for the parser tests. The two `render_control` tests still
fail to compile at this point, so run only the named tests.

- [ ] **Step 4: Implement `render_control` and the `GET` command**

`render_control` builds the payload documented in Interfaces. A control whose
input is `None` renders `{"present": false}` and nothing else; `gpu` is
`None` only when `/proc/gpufreqv2/stack_signed_opp_table` is unreadable, and
that makes the whole payload `supported: false` with the reason
`"MediaTek gpufreqv2 nodes are not available"`.
`command("GET mtk.control")` gathers `ControlInput` from the live nodes and
calls it. `is_command` accepts only `cmd == "GET mtk.control"`. Every other
form returns `Err("unsupported mtk command")` until later tasks add it.

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test -p dash-essential-daemon mtk_control`
Expected: PASS, 7 tests.

- [ ] **Step 6: Mount the module and allowlist the command**

In `lib.rs`, add `mod mtk_control;` after `mod service_control;`, and add a
branch directly after the `subsystem_clocks::is_command` block:

```rust
if mtk_control::is_command(cmd) {
    let _gate = match service_control::GATE.write() {
        Ok(gate) => gate,
        Err(_) => return "ERR service transition lock poisoned".into(),
    };
    return match mtk_control::command(cmd) {
        Ok(value) => format!("OK {value}"),
        Err(error) => format!("ERR {error}"),
    };
}
```

In `backend_bridge.rs`, add `|| command == "GET mtk.control"` to the
allowlist.

- [ ] **Step 7: Verify the whole crate still lints and builds**

Run: `cargo clippy -p dash-essential-daemon --all-targets -- -D warnings && cargo fmt --check`
Expected: no warnings, no diff.

- [ ] **Step 8: Commit**

```bash
git add runtime/daemon-rust/src/mtk_control.rs runtime/daemon-rust/src/lib.rs runtime/host-rust/src/backend_bridge.rs
git commit -m "feat(mtk): read MediaTek GPU, switch and DDR nodes"
```

---

### Task 2: Journal `/proc/gpufreqv2` writes

**Files:**
- Modify: `runtime/daemon-rust/src/service_control.rs:283-306`
- Test: `runtime/daemon-rust/src/service_control.rs` (`mod tests`)

**Interfaces:**
- Produces: `reversible_path` returns `true` for `/proc/gpufreqv2/*`, and
  `false` for any path whose file name is `dvfsrc_force_vcore_dvfs_opp` or
  `dvfsrc_force_ddr_opp`.

- [ ] **Step 1: Write the failing test**

Add to the existing `mod tests` in `service_control.rs`:

```rust
#[test]
fn gpufreqv2_nodes_are_reversible_but_dvfsrc_force_nodes_are_not() {
    assert!(reversible_path(Path::new(
        "/proc/gpufreqv2/fix_target_opp_index"
    )));
    assert!(reversible_path(Path::new("/proc/gpufreqv2/fix_custom_freq_volt")));
    assert!(!reversible_path(Path::new(
        "/sys/devices/platform/1c013000.dvfsrc/1c013000.dvfsrc:dvfsrc-helper/dvfsrc_force_vcore_dvfs_opp"
    )));
    assert!(!reversible_path(Path::new(
        "/sys/devices/platform/1c013000.dvfsrc/1c013000.dvfsrc:dvfsrc-helper/dvfsrc_force_ddr_opp"
    )));
}
```

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cargo test -p dash-essential-daemon gpufreqv2_nodes_are_reversible`
Expected: FAIL on the first assertion.

- [ ] **Step 3: Extend the predicate**

Add `|| raw.starts_with("/proc/gpufreqv2/")` to the accepted prefixes, and add
`"dvfsrc_force_vcore_dvfs_opp" | "dvfsrc_force_ddr_opp"` to the `matches!`
exclusion list of file names.

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cargo test -p dash-essential-daemon gpufreqv2_nodes_are_reversible`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add runtime/daemon-rust/src/service_control.rs
git commit -m "feat(mtk): journal gpufreqv2 writes and skip write-only force nodes"
```

---

### Task 3: GPU hard pin with voltage and readback

**Files:**
- Modify: `runtime/daemon-rust/src/mtk_control.rs`
- Modify: `runtime/daemon-rust/src/lib.rs:8966` block is unchanged; `is_command`
  and `command` grow in `mtk_control.rs`

**Interfaces:**
- Consumes: `parse_stack_opps`, `pin_index_for_khz`, `parse_gpufreq_status`,
  `reversible_path` (Task 2)
- Produces:
  - `ACTION mtk.gpu.pin <khz>` (`0` releases)
  - `ACTION mtk.gpu.pin_volt <khz> <volt>`
  - `fn gpu_pin_with(khz: u32, volt: Option<u32>, rows: &[OppRow]) -> Result<(), String>`
  - `fn pin_verified(expected_index: u32, status: &str) -> bool`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn pin_verification_accepts_the_written_index_while_the_gpu_idles() {
    let status = "[GPU   OPP]      Index: 51, Freq:   26000, Volt:  49375, Vsram:  75000";
    assert!(pin_verified(51, status));
    assert!(!pin_verified(50, status));
}

#[test]
fn pin_rejects_an_unknown_frequency_without_writing() {
    let rows = parse_stack_opps(STACK);
    assert_eq!(pin_index_for_khz(&rows, 999000), None);
    assert!(gpu_pin_with(999000, None, &rows).is_err());
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p dash-essential-daemon pin_`
Expected: FAIL — `pin_verified` and `gpu_pin_with` do not exist.

- [ ] **Step 3: Implement the pin**

- `fn gpu_pin_with(khz: u32, volt: Option<u32>, rows: &[OppRow]) -> Result<(), String>`
  is the testable core; `fn gpu_pin(khz, volt)` reads the live table and calls
  it.
- `khz == 0` releases: `service_control::write("/proc/gpufreqv2/fix_target_opp_index", "-1")`
  then `service_control::write("/proc/gpufreqv2/fix_custom_freq_volt", "0 0")`.
- Plain pin: resolve the index, reject when absent, write `0 0` to
  `fix_custom_freq_volt`, write the index to `fix_target_opp_index`.
- Voltage pin: reject a `volt` that is not the chosen row's own `volt`; write
  `-1` to `fix_target_opp_index`, then `"<khz> <volt>"` to
  `fix_custom_freq_volt`.
- After any pin, poll `pin_verified(index, status)` every 30 ms for at most
  300 ms. On timeout, restore the journaled originals for both keys when
  `service_control::original` returns one, otherwise write `-1` and `0 0`;
  call `service_control::forget` for both keys; return
  `Err(format!("GPU pin verify failed: index {index} not live"))`.
- `pin_verified`: `parse_gpufreq_status(status).map(|(index, _)| index) == Some(expected_index)`.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p dash-essential-daemon pin_`
Expected: PASS.

- [ ] **Step 5: Wire the commands**

In `is_command`, accept `cmd.starts_with("ACTION mtk.gpu.pin ")` and
`cmd.starts_with("ACTION mtk.gpu.pin_volt ")`. In `command`, parse the
arguments and call `gpu_pin`. Every `ACTION` returns
`Err("Dash Essential is disabled")` when `!service_control::enabled()`, in
the same words `subsystem_clocks` uses.

- [ ] **Step 6: Run the crate tests and lints**

Run: `cargo test -p dash-essential-daemon && cargo clippy -p dash-essential-daemon --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git add runtime/daemon-rust/src/mtk_control.rs
git commit -m "feat(mtk): pin the GPU clock with verified readback"
```

---

### Task 4: Feature switches and DCS

**Files:**
- Modify: `runtime/daemon-rust/src/mtk_control.rs`

**Interfaces:**
- Consumes: `parse_dcs_menu`
- Produces:
  - `ACTION mtk.switch <fpsgo|fbt_ceiling|perfmgr|ged_kpi> <0|1>`
  - `ACTION mtk.thermal_gpt <disable|restore>`
  - `ACTION mtk.dcs <0|code>`
  - `fn switch_path(name: &str) -> Option<&'static str>`
  - `fn dcs_write_value(code: u32) -> String`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn perfmgr_uses_the_node_this_kernel_exposes() {
    // The first existing node wins; mtk_fpsgo is present on mt6991.
    assert_eq!(
        switch_path("perfmgr"),
        Some("/sys/module/mtk_fpsgo/parameters/perfmgr_enable")
    );
    assert_eq!(switch_path("nonsense"), None);
}

#[test]
fn dcs_zero_disables_and_any_code_enables() {
    assert_eq!(dcs_write_value(0), "0");
    assert_eq!(dcs_write_value(64), "64");
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p dash-essential-daemon switch_`
Expected: FAIL — `switch_path` and `dcs_write_value` do not exist.

- [ ] **Step 3: Implement the switches**

- `switch_path`: match on the name to a fixed list; `perfmgr` tries
  `/sys/module/mtk_fpsgo/parameters/perfmgr_enable` then
  `/sys/module/perfmgr_mtk/parameters/perfmgr_enable` and returns the first
  that exists. Unknown names return `None`.
- `ACTION mtk.switch`: reject an unknown name or a value other than `0`/`1`;
  write through `service_control::write`; read back and require the same
  digit; return the new state.
- `ACTION mtk.dcs`: `parse::<u32>()`, reject a code not present in the parsed
  menu unless it is `0`, write `dcs_write_value(code)` through
  `service_control::write`.
- `ACTION mtk.thermal_gpt disable`: write `disable` to
  `/sys/kernel/thermal/gpt` through `service_control::write` (which journals
  the original scalar string). `ACTION mtk.thermal_gpt restore` restores that
  journaled original with `service_control::write`, and returns an error when
  no original was journaled.
- Add all three forms to `is_command` and to `command`'s dispatch, with the
  same `service_control::enabled()` gate as Task 3.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p dash-essential-daemon switch_ && cargo test -p dash-essential-daemon dcs_`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add runtime/daemon-rust/src/mtk_control.rs
git commit -m "feat(mtk): add FPSGO, FBT, Perfmgr, GED KPI and DCS controls"
```

---

### Task 5: Fixed DDR OPP, per-control reset, and release

**Files:**
- Modify: `runtime/daemon-rust/src/mtk_control.rs`
- Modify: `runtime/daemon-rust/src/lib.rs:8654` (call `mtk_control::restore_all()`)

**Interfaces:**
- Consumes: `dvfsrc_helper_dir`, `parse_dvfsrc_opp_table`, `service_control::{write, original, forget}`
- Produces:
  - `ACTION mtk.ddr.force_opp <index|off>`
  - `ACTION mtk.reset <gpu|thermal_gpt|fpsgo|fbt_ceiling|perfmgr|ged_kpi|dcs|ddr>`
  - `struct ResetTarget { off: &'static str }`
  - `fn reset_target(name: &str) -> Option<ResetTarget>`
  - `fn ddr_index_valid(index: u32, table: &[u64]) -> bool`
  - `pub(super) fn restore_all() -> Result<(), String>`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn ddr_force_rejects_an_index_past_the_table() {
    let table = parse_dvfsrc_opp_table("1532000000 2067000000 9594000000");
    assert_eq!(table.len(), 3);
    assert!(ddr_index_valid(2, &table));
    assert!(!ddr_index_valid(3, &table));
}

#[test]
fn reset_names_are_the_ones_the_ui_sends() {
    for name in [
        "gpu", "thermal_gpt", "fpsgo", "fbt_ceiling", "perfmgr", "ged_kpi", "dcs", "ddr",
    ] {
        assert!(reset_target(name).is_some(), "{name}");
    }
    assert!(reset_target("nonsense").is_none());
    assert_eq!(reset_target("ddr").unwrap().off, "0");
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test -p dash-essential-daemon ddr_force_rejects`
Expected: FAIL — functions do not exist.

- [ ] **Step 3: Implement DDR, reset and release**

- `ddr_index_valid(index: u32, table: &[u64]) -> bool` is `(index as usize) < table.len()`.
- `ACTION mtk.ddr.force_opp <index|off>`: resolve `dvfsrc_helper_dir()`, read
  `dvfsrc_ddr_opp_table`, reject an out-of-range index, then plain
  `fs::write` to `<dir>/<basename>:dvfsrc-helper/dvfsrc_force_vcore_dvfs_opp`
  with the index, or `0` for `off`. No readback is possible; return the index
  that was written.
- `reset_target(name) -> Option<ResetTarget>` maps each accepted name to nodes
  and their off values: `off` is `"0"` for both dvfsrc force nodes and
  `"-1"` for the GPU pin. Unknown names return `None`.
- `restore_all`: if `dvfsrc_helper_dir()` resolves, write `0` to both force
  nodes (ignore a missing node). Then return `Ok(())`. Journaled nodes need no
  work here; the generic journal replay owns them.
- `ACTION mtk.reset <name>`: restore the journaled original for each node of
  that control when `service_control::original` returns one, call
  `service_control::forget`, otherwise write the documented off value. For
  `gpu` that off value is the `-1` / `0 0` pair.

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test -p dash-essential-daemon ddr_`
Expected: PASS.

- [ ] **Step 5: Wire release**

In `transition_service`, add `mtk_control::restore_all()` directly after
`subsystem_clocks::restore_all()?;`.

- [ ] **Step 6: Verify the crate**

Run: `cargo test -p dash-essential-daemon && cargo clippy -p dash-essential-daemon --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git add runtime/daemon-rust/src/mtk_control.rs runtime/daemon-rust/src/lib.rs
git commit -m "feat(mtk): fixed DDR OPP, per-control reset and release"
```

---

### Task 6: MediaTek Control screen

**Files:**
- Create: `ui/flutter/lib/mtk_control.dart`
- Modify: `ui/flutter/lib/main.dart:34-42` (part list), `:66` (enum),
  `:88` (title), `:1089` (detail body), `:5669` (hub row)
- Test: `ui/flutter/test/mtk_control_test.dart`

**Interfaces:**
- Consumes: `PerAppBackend.instance.command`, the `GET mtk.control` payload
  keys from Task 1, `ACTION mtk.*` from Tasks 3-5
- Produces: `class MtkControlScreen extends StatefulWidget` with
  `const MtkControlScreen({required this.onBack, this.exchange, super.key})`,
  where `exchange` is `Future<dynamic> Function(String)?`

- [ ] **Step 1: Write the failing widget test**

```dart
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:dash_essential_ui/main.dart';

Map<String, dynamic> status({bool gedKpi = false}) => <String, dynamic>{
  'supported': true,
  'reason': null,
  'soc': 'mt6991',
  'gpu': <String, dynamic>{
    'opps': <Map<String, dynamic>>[
      <String, dynamic>{'index': 0, 'khz': 1612000, 'volt': 79500},
      <String, dynamic>{'index': 51, 'khz': 338000, 'volt': 49375},
    ],
    'pin': <String, dynamic>{'index': null, 'khz': null},
    'thermal_gpt': <String, dynamic>{'present': true, 'disabled': false},
  },
  'switches': <String, dynamic>{
    'fpsgo': <String, dynamic>{'present': true, 'on': true},
    'fbt_ceiling': <String, dynamic>{'present': true, 'on': false},
    'perfmgr': <String, dynamic>{'present': true, 'on': false},
    'ged_kpi': <String, dynamic>{'present': gedKpi, 'on': null},
  },
  'dcs': <String, dynamic>{'present': true, 'code': 0, 'menu': <List<dynamic>>[]},
  'ddr': <String, dynamic>{'present': false, 'readable': false, 'opps': <int>[]},
};

void main() {
  testWidgets('absent controls render no card', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MtkControlScreen(
            onBack: () {},
            exchange: (command) async => status(),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('GED KPI'), findsNothing);
    expect(find.text('Fixed DDR OPP'), findsNothing);
    expect(find.text('Fixed frequency'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('a switch sends its action command', (tester) async {
    final commands = <String>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: MtkControlScreen(
            onBack: () {},
            exchange: (command) async {
              commands.add(command);
              return status();
            },
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final finder = find.byKey(const ValueKey('mtk-switch-fpsgo'));
    await Scrollable.ensureVisible(tester.element(finder), alignment: 0.5);
    await tester.pumpAndSettle();
    await tester.tap(finder);
    await tester.pumpAndSettle();
    expect(commands, contains('ACTION mtk.switch fpsgo 0'));
    await tester.pumpWidget(const SizedBox());
  });
}
```

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cd ui/flutter && flutter test test/mtk_control_test.dart`
Expected: FAIL — `MtkControlScreen` is not defined.

- [ ] **Step 3: Implement the screen**

`mtk_control.dart` starts with `part of 'main.dart';` and copies the
`SubsystemClocksScreen` skeleton: `_exchange`, `_read`, `_action`, 2 s refresh
timer, lifecycle pause, `_error` banner, `DetailHeader(title: 'MediaTek
Control')`.

Cards, each rendered only when its `present` is `true`:

- **Fixed frequency** — a dropdown of `gpu.opps` showing `"<khz> kHz"`, an
  optional voltage dropdown that offers only `"<volt>"` for the chosen row,
  Apply sends `ACTION mtk.gpu.pin <khz>` or
  `ACTION mtk.gpu.pin_volt <khz> <volt>`, and a release button sends
  `ACTION mtk.gpu.pin 0`. The note reads: "Fixed frequency outranks the
  maximum frequency and cannot be combined with it."
- **GPU voltage warning** — plain text inside the card: "A wrong voltage can
  freeze the GPU. Only a reboot recovers it."
- **Thermal governor** — `SwitchListTile` keyed `mtk-thermal-gpt`, sends
  `ACTION mtk.thermal_gpt disable` / `ACTION mtk.reset thermal_gpt`.
- **Feature switches** — one `SwitchListTile` per present entry of `switches`,
  keys `mtk-switch-fpsgo`, `mtk-switch-fbt_ceiling`, `mtk-switch-perfmgr`,
  `mtk-switch-ged_kpi`, sending `ACTION mtk.switch <name> <0|1>`.
- **DCS** — dropdown of `dcs.menu` plus a Disable entry; when `dcs.code` is
  `null` show the label `Enabled (code not reported)`. Sends
  `ACTION mtk.dcs <code>`.
- **Fixed DDR OPP** — a dropdown of `ddr.opps`, a Release button
  (`ACTION mtk.ddr.force_opp off`), and the note: "Some kernels expose this
  node write-only, so the current value cannot be shown. A wrong value can
  freeze the device; only a reboot recovers it."
- Each card has an OEM Reset button sending `ACTION mtk.reset <name>`.

- [ ] **Step 4: Wire the plumbing**

Add `part 'mtk_control.dart';`, `mtkControl` to `enum DashScreen`, a title
case `'MediaTek Control'`, a `_detailBody` case returning
`MtkControlScreen(onBack: _back)`, and a `_HubControlRow` after the
`subsystemClocks` row:

```dart
_HubControlRow(
  title: 'MediaTek Control',
  subtitle: 'GPU pin, voltage, thermal and DDR',
  icon: Icons.developer_board_rounded,
  accent: const Color(0xFFFFB84D),
  onTap: () => onOpen(DashScreen.mtkControl),
),
```

- [ ] **Step 5: Run the widget test to confirm it passes**

Run: `cd ui/flutter && flutter test test/mtk_control_test.dart`
Expected: PASS, 2 tests.

- [ ] **Step 6: Run analysis and the full Flutter suite**

Run: `cd ui/flutter && flutter analyze && flutter test`
Expected: no issues, all tests pass.

- [ ] **Step 7: Commit**

```bash
git add ui/flutter/lib/mtk_control.dart ui/flutter/lib/main.dart ui/flutter/test/mtk_control_test.dart
git commit -m "feat(mtk): add the MediaTek Control screen"
```

---

### Task 7: Changelog and on-device verification

**Files:**
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: everything above
- Produces: release notes for the feature and a recorded on-device result

- [ ] **Step 1: Write the changelog entry**

Add above `## 1.23.14 — Vega`, using the next version heading chosen at
release time (the release flow owns version bumps):

```markdown
## Unreleased

### Added

- MediaTek Control: fixed GPU frequency with the OPP table's own voltage,
  `thermal/gpt` disable, FPSGO / FBT ceiling / Perfmgr / GED KPI / DCS
  switches, and a fixed DDR OPP. Every readable write is journaled and
  restored; the two dvfsrc force nodes are write-only and are reset to `0` on
  release.

### Note

- A wrong GPU voltage or DDR OPP can freeze the device. Recovery is a reboot.
```

- [ ] **Step 2: Run the full local verification**

Run:
```bash
cargo fmt --check
cargo clippy -p dash-essential-daemon --all-targets -- -D warnings
cargo test -p dash-essential-daemon
cd ui/flutter && flutter analyze && flutter test
```
Expected: all clean.

- [ ] **Step 3: Verify on the device**

On the `mt6991` phone with Dash installed and the service enabled:

1. Open MediaTek Control. Confirm the GPU card lists the stack table's
   frequencies and the live pin reads "not fixed".
2. Pin 598 MHz. Reopen the screen and confirm the pin shows index 33
   (or whichever index `stack_signed_opp_table` prints for 598 MHz).
3. In a root shell, `cat /proc/gpufreqv2/gpufreq_status` and confirm
   `[GPU OPP] Index:` matches the pinned index.
4. Release, then confirm `cat /proc/gpufreqv2/fix_target_opp_index` shows
   `-1` and `gpufreq_status` leaves the pin.
5. Toggle FPSGO off and on, and confirm `/sys/kernel/fpsgo/common/fpsgo_enable`
   matches each time.
6. Disable the service from Settings and confirm
   `/proc/gpufreqv2/fix_target_opp_index` is `-1` and
   `/sys/kernel/fpsgo/common/fpsgo_enable` is back to its original value.

Record the observed index and any kernel that behaves differently in the
commit message.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs(mtk): document MediaTek GPU and DDR control"
```
