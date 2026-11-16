# MediaTek GPU and DDR Control — Design

- Date: 2026-11-16
- Status: draft, awaiting approval
- Source of behaviour: Scene `com.omarea.vtools` N1 2026.11 PreRelease 1,
  `assets/kr-script/mtk/{mtk_v2.sh,mtk_utils_v2.sh,mtk_v2_env.sh}` and gate
  `assets/kr-script/common/mtk_v2.sh`
- Approved scope: option 3 (everything) — hard pin, voltage, `thermal/gpt`,
  FPSGO / FBT ceiling / Perfmgr / GED KPI / DCS switches, DDR minimum and
  fixed DDR OPP.

## Intent

Give Dash the same MediaTek control Scene exposes, in English, with Dash's
own safety rules: the daemon owns state, every write is journal-and-restore,
and a request the kernel does not actually accept is reported as a failure
instead of being shown as applied.

Scene's mechanism is plain root shell over `/proc` and `/sys` text nodes.
There is no MediaTek HAL, no binder call and no vendor library involved. That
is why the behaviour ports cleanly.

## What Scene actually does

Scene renders its page with its own kr-script XML DSL and runs one shell
fragment per control.

**Platform gate** (`common/mtk_v2.sh`): show the v2 page when
`ro.board.platform` contains `mt`, `/proc/ppm` is absent, and
`/proc/gpufreqv2` is a directory. The GPU group additionally requires
`/proc/gpufreqv2/stack_signed_opp_table`.

**Fixed frequency** (`mtk_utils_v2.sh` `gpu_freq()`):

```sh
if state == -1:                       # "do not fix"
  echo -1  > fix_target_opp_index
  echo 0 0 > fix_custom_freq_volt
elif voltage is empty or -1:          # pin, driver picks the voltage
  echo 0 0 > fix_custom_freq_volt
  echo <opp> > fix_target_opp_index   # mt6989 writes "<opp> <opp>"
  echo disable > /sys/kernel/thermal/gpt   # when the node exists
else:                                 # pin, explicit voltage
  echo -1 > fix_target_opp_index
  echo "<freq> <volt>" > fix_custom_freq_volt
```

`<opp>` is the index printed by `stack_signed_opp_table`, found by matching
the chosen frequency against that table. The option list is the frequency
column of the same table, labelled `<n>KHz`.

**Maximum frequency** (`gpu_freq_max_freq()`): write the ladder index to
`/sys/kernel/ged/hal/custom_upbound_gpu_freq`, then call `gpu_freq()` with
`state=-1`. That second step clears the fixed-frequency pin.

**Why the note says they cannot coexist**: setting a maximum clears the pin.
Setting a pin does not clear the GED ceiling, but `fix_target_opp_index`
outranks the DVFS decision, so the pin wins. Scene states both facts in
`mtk_v2.sh:54`.

The stack table's own line format, verbatim from the device:

```
[00*] freq: 1612000, volt:  79500, vsram:  79500
```

Both numbers are raw driver units and are written back verbatim. `freq` is
kilohertz (`338000` is 338 MHz). `volt` is the driver's own unit
(`79500`); Dash must not convert, scale or format it.

**Other controls on the same page**: `fpsgo/common/fpsgo_enable`,
`fpsgo/fbt/enable_ceiling`, `mtk_fpsgo/parameters/perfmgr_enable`
(or `perfmgr_mtk/...`), `sspm_v3/holders/ged/parameters/is_GED_KPI_enabled`,
`ged/hal/dcs_mode` (0 disables, a minimum core count enables), DDR minimum at
the dvfsrc devfreq `min_freq`, and fixed DDR at
`<dvfsrc>/<name>:dvfsrc-helper/dvfsrc_force_vcore_dvfs_opp`.

**Locking**: Scene wraps writes as `chmod 644` → write → `chmod 444`. Dash
replaces that with `service_control::write`, which journals the original and
restores it on release. Dash's approach is strictly safer and needs no
permission flips.

## Device facts (probed on the user's phone, `mt6991`, model `2602BRT18C`)

Present: `/proc/gpufreqv2/{fix_target_opp_index,fix_custom_freq_volt,gpu_working_opp_table,gpu_signed_opp_table,stack_signed_opp_table,gpufreq_status,limit_table}`,
`/sys/kernel/ged/hal/{custom_upbound_gpu_freq,custom_boost_gpu_freq,dcs_mode}`,
`/sys/kernel/thermal/gpt`,
`/sys/kernel/fpsgo/{common/fpsgo_enable,fbt/enable_ceiling}`,
`/sys/module/mtk_fpsgo/parameters/perfmgr_enable`,
`/sys/devices/platform/1c013000.dvfsrc/1c013000.dvfsrc:dvfsrc-helper/{dvfsrc_force_vcore_dvfs_opp,dvfsrc_force_ddr_opp,dvfsrc_ddr_opp_table}`.

Absent: `/proc/ppm`, `/sys/module/perfmgr_mtk`, `/sys/module/ged/parameters/gpu_cust_upper_freq`.

Live values: `custom_upbound_gpu_freq=49`, `gpu_cust_boost_freq=338000`,
`fpsgo_enable=1`, `enable_ceiling=0`, `perfmgr_enable=0`,
`thermal/gpt=40000,7,75000,666666666,666666666`.

### The two OPP tables do not agree

`gpu_working_opp_table` and `stack_signed_opp_table` both publish 52 rows with
the same `[00]`…`[51]` index labels, and their frequencies differ:

| index | working | stack |
|---|---|---|
| 00 | 1612000 | 1612000 |
| 01 | 1612000 | 1586000 |
| 04 | 1508000 | 1508000 |
| 31 | 1378000 | 1352000 |
| 49 | 572000 | 572000 |
| 51 | 338000 | 338000 |

Dash currently resolves GED indices (`custom_boost_gpu_freq`,
`custom_upbound_gpu_freq`) against the **working** table. Scene resolves
`fix_target_opp_index` against the **stack** table. Copying one index into the
other index space silently pins a different frequency. This is the single
biggest correctness trap in the port and the reason for mandatory verification
below.

`gpufreq_status` reports the live result:

```
[GPU   OPP]      Index: 51, Freq:   26000, Volt:  49375, Vsram:  75000
[STACK OPP]      Index: 51, Freq:   26000, Volt:  49000, Vsram:  75000
```

`dvfsrc_force_vcore_dvfs_opp` and `dvfsrc_force_ddr_opp` are write-only
(`--w-------`, unreadable even as root). `service_control::write` cannot
journal them, so their release path must write the explicit "off" value.

The force node takes an index into the **combined** `dvfsrc_opp_table`
(mt6991: 87 rows, `[44 ]:760000, 8528  Mbps [4]`), never the DDR-only
`dvfsrc_ddr_opp_table` (10 raw values). Verified on device (mt6991):
`FORCE_OPP_IDX` reads `255, 255` when nothing is forced; writing `N` to
`dvfsrc_force_vcore_dvfs_opp` pins combined OPP `N`; writing `255` clears it.
`0` is a real OPP index and would pin the lowest pair. Release sentinel is
`255` (Scene uses `-1`/`999`; both are wrong for this driver). Writing to
`dvfsrc_force_ddr_opp` has no observable effect on mt6991 and is not written.

## Already covered by Dash — do not rebuild

- **DDR minimum frequency**: `subsystem_clocks` exposes the dvfsrc devfreq
  `min_freq` window with a journal, a 30 s trial and a restore path
  (`runtime/daemon-rust/src/subsystem_clocks.rs`).
- **GPU floor and ceiling**: `custom_boost_gpu_freq` / `custom_upbound_gpu_freq`
  plus the `gpu_cust_*` module parameters (`set_gpu_min_freq`,
  `set_gpu_max_freq`, `write_gpu_bounds`).
- **Thermal cooling cap and EB DVFS policy disarm**: `clear_gpu_cooling_cap`,
  `disable_eb_dvfs_policy`, `ensure_gpu_thermal_mode`.

The one deliberate contract this design bends is the header of
`subsystem_clocks.rs`: "never voltage/force-OPP nodes". Fixed DDR OPP is a
force-OPP node, and voltage is a voltage write. They enter only because the
user chose them explicitly, and the UI must say so.

## Goals

1. A "MediaTek Control" screen listing only the controls this kernel exposes.
2. GPU hard pin with optional explicit voltage, verified against
   `gpufreq_status`.
3. `thermal/gpt` disable with journaled restore.
4. FPSGO, FBT ceiling, Perfmgr, GED KPI and DCS switches.
5. Fixed DDR OPP, gated on the force node existing.
6. Everything restored on release/reboot, or clearly documented as not
   restorable when the node is write-only.

## Non-goals

- No MTK HAL, binder, JNI or vendor `.so`.
- No new DEX classes, no new Flutter dependencies, no new Android permissions.
- No CPU control. Scene's CPU page stays out of scope.
- No per-app GPU pinning. Global control only.
- No `chmod 444` locking. Dash's writer and restore path replace it.
- No automatic enabling. Every risky control starts in the current kernel
  state and only changes when the user acts.

## Anti-goals

- Never derive an OPP index from a formula or from the other table.
- Never write a voltage the driver's own table does not list for that
  frequency.
- Never report success without reading the result back.
- Never leave a risky write applied after a failed verification.
- Never show a control for a node that is absent.

## Constraints and risks

- **Voltage and fixed DDR OPP can hang or hard-lock the GPU/SoC.** Recovery is
  a reboot. The confirmation dialog must state this in plain language.
- `fix_target_opp_index` semantics on `mt6989` differ (index written twice).
  On `mt6991` write once, then verify.
- `thermal/gpt` accepts `disable`; the enabled form is the original scalar
  string, which `service_control::write` journals. Do not invent an `enable`
  token.
- `dcs_mode` is not boolean. `0` disables; a minimum core-count code enables.
  The UI must present the kernel's own menu, not a switch.
- Write-only force nodes cannot be journaled. Release writes `0` and says so.
- The daemon already owns `custom_upbound_gpu_freq` through GPU profiles.
  A hard pin must not fight a profile: before pinning, call the existing
  `disable_eb_dvfs_policy()`, `clear_gpu_cooling_cap()` and
  `ensure_gpu_thermal_mode()`. Pinning must **not** write
  `custom_upbound_gpu_freq` — that is the write that clears the pin.

## Approaches

**A. Daemon-native module (recommended).** New
`runtime/daemon-rust/src/mtk_control.rs` shaped like `subsystem_clocks.rs`:
`is_command`, `command`, `restore_all`, live-node reads, no persisted fields.
Flutter talks to it with `PerAppBackend.instance.command('GET mtk.control')`
and `ACTION mtk.*`. Writes go through `service_control::write` after
`reversible_path` is extended to accept `/proc/gpufreqv2/`.

**B. Shell-script port.** Ship Scene's scripts and `exec` them from the
daemon. Rejected: no journaling, no verification, no state ownership, and it
puts root shell logic outside the daemon's audit surface.

**C. Hard pin only.** Smallest, but the user already rejected the smaller
scope.

Approach A is chosen.

## Design

### Daemon module

`mtk_control.rs`, mounted next to `subsystem_clocks` in the command dispatch.
No persisted state fields: the kernel nodes are the truth, so `GET` reads them
live. This is not the same as `subsystem_clocks`, which keeps its own
`subsystem-clock-originals.json`, reloads it in `start()` and runs a trial
deadline thread. The mtk module needs none of that because `service_control`
already persists the journal, but it does need an explicit release hook (see
"Restore wiring").

```
GET  mtk.control
ACTION mtk.gpu.pin <khz>            # khz=0 or "off" releases
ACTION mtk.gpu.pin_volt <khz> <volt>  # explicit voltage, confirmed in UI
ACTION mtk.thermal_gpt <disable|restore>
ACTION mtk.switch <fpsgo|fbt_ceiling|perfmgr|ged_kpi> <0|1>
ACTION mtk.dcs <0|menu-code>
ACTION mtk.ddr.force_opp <index|off>
ACTION mtk.reset <control>          # one control back to its original
```

`GET mtk.control` returns the same shape `subsystem_clocks` uses:
`supported`, `reason`, plus one object per control with `present`, live value
and the values the kernel offers. Absent nodes come back `present: false` and
the UI hides them. The two force nodes have no readable value, so their object
carries `present: true, readable: false` and no stored index; the UI shows the
control as a write-only picker with no "current" marker.

Three mounting rules:

- The `subsystem_clocks::is_command` branch at `lib.rs:8966` runs before the
  global `service_control::enabled()` gate at `lib.rs:9019`. The mtk branch
  must therefore check `service_control::enabled()` itself for every `ACTION`
  and return the same "disabled" error. `GET` is served while disabled, as
  `subsystem_clocks` does (`subsystem_clocks.rs:341,391`).
- Every command string must be added to the allowlist in
  `dash_backend_app_controls_exchange` (`runtime/host-rust/src/backend_bridge.rs:499`).
  Anything not listed returns `-1` before it reaches the daemon.
- `ACTION mtk.reset <control>` is the reset the UI calls: restore the
  journaled original when the node is readable, otherwise write the documented
  off value. It is separate from the release path.

### Paths

| Control | Node | Write |
|---|---|---|
| hard pin | `/proc/gpufreqv2/fix_target_opp_index` | stack-table index; `-1` off |
| pin voltage | `/proc/gpufreqv2/fix_custom_freq_volt` | `<freq> <volt>`; `0 0` off |
| OPP source | `/proc/gpufreqv2/stack_signed_opp_table` | read only |
| verification | `/proc/gpufreqv2/gpufreq_status` | read only |
| thermal | `/sys/kernel/thermal/gpt` | `disable` |
| FPSGO | `/sys/kernel/fpsgo/common/fpsgo_enable` | `0` / `1` |
| FBT ceiling | `/sys/kernel/fpsgo/fbt/enable_ceiling` | `0` / `1` |
| Perfmgr | `/sys/module/mtk_fpsgo/parameters/perfmgr_enable`, else `perfmgr_mtk` | `0` / `1` |
| GED KPI | `/sys/module/sspm_v3/holders/ged/parameters/is_GED_KPI_enabled` | `0` / `1` |
| DCS | `/sys/kernel/ged/hal/dcs_mode` | `0` or menu code |
| DDR fixed | `<dvfsrc>/<name>:dvfsrc-helper/dvfsrc_force_vcore_dvfs_opp` | combined DVFSRC OPP index |
| DDR table | same dir `dvfsrc_opp_table` (combined vcore+DDR rows) | read only |

`<dvfsrc>` is probed in Scene's order, extended with the path this device
actually uses:
`/sys/devices/platform/soc/1c00f000.dvfsrc`, `/sys/devices/platform/1c00f000.dvfsrc`,
`/sys/devices/platform/1c013000.dvfsrc`, `/sys/devices/platform/soc/1c100000.dvfsrc`,
then a scan of `/sys/devices/platform/*.dvfsrc`.

### Hard pin flow

1. Parse `stack_signed_opp_table` into `(index, freq_khz, volt_raw)`, keeping
   the row's printed index. All three values are raw; no unit conversion.
2. Reject a frequency the table does not list. No nearest-match guessing.
   When a frequency appears in more than one row (338 MHz does), take the
   first row in table order, which is the lowest index. Scene's
   `grep "$state,"` behaves the same way.
3. Write `0 0` to `fix_custom_freq_volt` for a plain pin.
4. Write the table's own index to `fix_target_opp_index`.
5. Poll `gpufreq_status` for up to 300 ms and require
   `[GPU OPP] Index:` to equal the written index. Fall back to comparing
   `Freq:` (kilohertz) to the requested kHz only when the index cannot be
   parsed. Do not compare frequencies alone: an idle GPU reports values below
   the table (the probe showed `Freq: 26000` at index 51), so a frequency-only
   check fails during the settle window.
6. On mismatch, restore the journaled original for `fix_target_opp_index` and
   `fix_custom_freq_volt` when present, otherwise write the off pair
   (`-1` and `0 0`), call `service_control::forget` for both keys, and return
   the observed index and frequency in the error.

Voltage variant: write `-1` to `fix_target_opp_index` first, then
`<freq> <volt>` to `fix_custom_freq_volt`, then verify the same way. `<volt>`
is the table's own `volt` field for that row, written verbatim; it is not a
user-typed number and not converted to microvolts.

Release: `-1` to `fix_target_opp_index`, `0 0` to `fix_custom_freq_volt`.

### Write path and restore wiring

`service_control::reversible_path` accepts `/sys/`, `/proc/sys/` and
`/proc/touch_boost/`. Extend it to accept `/proc/gpufreqv2/` so the pin and
voltage writes are journaled, with its own test.

The two dvfsrc force nodes are write-only (`--w-------`, unreadable even as
root). `service_control::write` reads a node before journaling it
(`service_control.rs:347-366`), so it must not be used for them: the read
fails and the write aborts. Those two writes use plain `fs::write`, and
`reversible_path` must explicitly exclude both filenames.

Release wiring: `transition_service` only calls
`subsystem_clocks::restore_all()` (`lib.rs:8654`). Add
`mtk_control::restore_all()` beside it. That function writes the documented
off value to the write-only force nodes (`0` to each), because no original
exists to replay. Journaled nodes are already restored by the generic journal
replay; the hook exists only for the nodes that cannot be journaled.

### Switches and DCS

`fpsgo`, `fbt_ceiling`, `perfmgr` and `ged_kpi` are plain `0`/`1` writes
through `service_control::write`, so reset and release both come from the
journal.

`dcs_mode` is a menu, not a value: `0` disables it, and any other code sets a
minimum available core count. `GET` parses the kernel's own menu into
`(code, label)` pairs plus whether it is disabled (`T enable: 0`). The active
code is not reported, so an enabled state has no code to show.
`ACTION mtk.dcs 0` disables, `ACTION mtk.dcs <code>` enables, and reset
replays the journalled original because the node is readable.

### Flutter

New `ui/flutter/lib/mtk_control.dart`, declared as a `part of 'main.dart'`
next to `subsystem_clocks.dart`. It mirrors `SubsystemClocksScreen`: one card
per control, live value, Apply, OEM Reset, and a note when a node is absent.
Timeouts, dirty state and haptics copy the existing screen.

Plumbing, all of which the screen needs before it can open:

- add `mtkControl` to `enum DashScreen` (`main.dart:66`) and to
  `DashScreenName.title` (`main.dart:88`);
- add the `_detailBody` case beside `DashScreen.subsystemClocks`
  (`main.dart:1089`);
- add the settings row beside the `subsystemClocks` row (`main.dart:5672`);
- add `part 'mtk_control.dart';` to the part list (`main.dart:34-42`).

Section order: "MediaTek Control" goes after `// 8. BACKUP & TRANSFER` in the
settings page.

The confirmation copy for the voltage and fixed-DDR controls must say, in
plain words, that a wrong value can freeze the GPU and that only a reboot
recovers it.

### Test plan

- Rust unit tests for `stack_signed_opp_table` parsing (printed index,
  kilohertz, raw volt, duplicate rows), dvfsrc path probing, `dcs_mode` menu
  parsing, and the `reversible_path` predicate for
  `/proc/gpufreqv2/fix_target_opp_index` (accept) and
  `/sys/devices/platform/1c013000.dvfsrc/.../dvfsrc_force_vcore_dvfs_opp`
  (reject).
- A verification test that a mismatched `gpufreq_status` produces an error and
  a rollback to the journaled original, or to `-1` when no original exists.
- A test that `ACTION mtk.*` returns the disabled error when the service is
  disabled.
- Flutter widget test: an absent node renders no card.
- On-device check after implementation: pin 598 MHz, read
  `gpufreq_status` index, release, confirm the pre-pin value returns.

## What is left to the builder

- Whether `fix_target_opp_index` indexes the stack table or the working table
  on this kernel. The design assumes the stack table (Scene's behaviour) and
  makes verification mandatory, so the builder may correct the table choice
  from observed `gpufreq_status` output.
- Whether `mt6991` needs the `mt6989` double-index write. Verify, do not
  assume.
- The exact `dcs_mode` menu format. The design fixes the behaviour (parse to
  code and label, `0` disables); the builder samples the real printed menu.
- The wording of the risk notice and the settings row label.
