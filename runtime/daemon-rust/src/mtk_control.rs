//! MediaTek gpufreqv2 control.
//!
//! Two operating-point tables are published and they do not agree:
//! `gpu_working_opp_table` and `stack_signed_opp_table` both label 52 rows
//! `[00]`..`[51]`, but the same index names a different frequency in each.
//! `fix_target_opp_index` is written with the index printed by the *stack*
//! table, and every write is read back from `gpufreq_status` before it is
//! reported as applied.

use super::read_node_text;
use super::service_control;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MTK_STACK_OPPS: &str = "/proc/gpufreqv2/stack_signed_opp_table";
const MTK_STATUS: &str = "/proc/gpufreqv2/gpufreq_status";
const MTK_FIX_INDEX: &str = "/proc/gpufreqv2/fix_target_opp_index";
const MTK_FIX_FREQ_VOLT: &str = "/proc/gpufreqv2/fix_custom_freq_volt";
const MTK_THERMAL_GPT: &str = "/sys/kernel/thermal/gpt";
const MTK_SOC: &str = "/sys/devices/soc0/machine";

const SWITCH_FPSGO: &str = "/sys/kernel/fpsgo/common/fpsgo_enable";
const SWITCH_FBT_CEILING: &str = "/sys/kernel/fpsgo/fbt/enable_ceiling";
const SWITCH_PERFMGR: [&str; 2] = [
    "/sys/module/mtk_fpsgo/parameters/perfmgr_enable",
    "/sys/module/perfmgr_mtk/parameters/perfmgr_enable",
];
const SWITCH_GED_KPI: &str = "/sys/module/sspm_v3/holders/ged/parameters/is_GED_KPI_enabled";
const DCS_MODE: &str = "/sys/kernel/ged/hal/dcs_mode";

const DVFSRC_ROOTS: [&str; 4] = [
    "/sys/devices/platform/soc/1c00f000.dvfsrc",
    "/sys/devices/platform/1c00f000.dvfsrc",
    "/sys/devices/platform/1c013000.dvfsrc",
    "/sys/devices/platform/soc/1c100000.dvfsrc",
];
const DVFSRC_HELPER_SUFFIX: &str = ":dvfsrc-helper";
/// The combined (vcore, DDR) operating-point table. `dvfsrc_force_vcore_dvfs_opp`
/// is indexed against this table, never the DDR-only `dvfsrc_ddr_opp_table`.
const DVFSRC_OPP_TABLE: &str = "dvfsrc_opp_table";
/// Verified on device: `FORCE_OPP_IDX` reads `255, 255` when nothing is forced
/// and writing 255 to a force node clears the force. `0` is a real OPP index
/// (the lowest pair) and would pin it instead of releasing.
const DVFSRC_RELEASE: &str = "255";
const DVFSRC_FORCE_VCORE: &str = "dvfsrc_force_vcore_dvfs_opp";
const DVFSRC_FORCE_DDR: &str = "dvfsrc_force_ddr_opp";

/// One row of the driver's own operating-point table.
///
/// `index` is the label the driver prints, not a position we compute. `khz`
/// and `volt` are raw driver units and are written back verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct OppRow {
    pub index: u32,
    pub khz: u32,
    pub volt: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct DcsMenu {
    pub current: Option<u32>,
    pub entries: Vec<(u32, String)>,
}

pub(super) struct GpuInput {
    pub opps: Vec<OppRow>,
    pub pin: Option<(u32, u32)>,
    pub thermal_gpt: Option<bool>,
}

pub(super) struct ControlInput {
    pub gpu: Option<GpuInput>,
    pub switches: [Option<bool>; 4],
    pub dcs: Option<DcsMenu>,
    pub ddr: Option<Vec<DvfsrcOpp>>,
}

fn field_u32(line: &str, key: &str) -> Option<u32> {
    let tail = line.split_once(key)?.1;
    tail.trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .ok()
}

/// Parse the stack operating-point table.
///
/// Each line names its own index, for example
/// `[00*] freq: 1612000, volt:  79500, vsram:  79500`.
pub(super) fn parse_stack_opps(raw: &str) -> Vec<OppRow> {
    let mut rows = Vec::new();
    for line in raw.lines() {
        let (Some(start), Some(end)) = (line.find('['), line.find(']')) else {
            continue;
        };
        let Ok(index) = line[start + 1..end]
            .trim()
            .trim_end_matches('*')
            .trim()
            .parse::<u32>()
        else {
            continue;
        };
        let (Some(khz), Some(volt)) = (field_u32(line, "freq:"), field_u32(line, "volt:")) else {
            continue;
        };
        rows.push(OppRow { index, khz, volt });
    }
    rows
}

/// The lowest index that publishes a frequency.
///
/// The table lists a few frequencies in more than one row, so a request is
/// resolved the way the vendor tooling resolves it: first row wins.
pub(super) fn pin_index_for_khz(opps: &[OppRow], khz: u32) -> Option<u32> {
    opps.iter().find(|row| row.khz == khz).map(|row| row.index)
}

/// The GPU's live index and frequency, in kilohertz.
///
/// The GPU idles below the table, so this is only used to confirm an index
/// that was just written, never to learn the table.
pub(super) fn parse_gpufreq_status(raw: &str) -> Option<(u32, u32)> {
    let line = raw
        .lines()
        .find(|line| line.trim_start().starts_with("[GPU   OPP]"))?;
    Some((field_u32(line, "Index:")?, field_u32(line, "Freq:")?))
}

/// The DCS menu, as the kernel prints it.
///
/// The kernel reports whether DCS is disabled (`T enable: 0`) but never the
/// active code, so an enabled state has no code to show.
pub(super) fn parse_dcs_menu(raw: &str) -> DcsMenu {
    let mut menu = DcsMenu::default();
    if raw
        .lines()
        .any(|line| line.trim_start().starts_with("T enable: 0"))
    {
        menu.current = Some(0);
    }
    for line in raw.lines() {
        let line = line.trim();
        let Some(end) = line.find(']') else { continue };
        if !line.starts_with('[') {
            continue;
        }
        let Ok(code) = line[1..end].trim().parse::<u32>() else {
            continue;
        };
        let label = line[end + 1..].trim();
        if label.is_empty() {
            continue;
        }
        menu.entries.push((code, label.to_string()));
    }
    menu
}

/// One row of the combined DVFSRC operating-point table.
///
/// The kernel prints one row per (vcore, DDR) pair, for example
/// `[44 ]:760000, 8528  Mbps [4]`. `index` is the printed label, `vcore` is
/// microvolts and `mbps` is the DDR rate of that row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct DvfsrcOpp {
    pub index: u32,
    pub vcore: u32,
    pub mbps: u32,
}

/// Parse the combined DVFSRC operating-point table.
///
/// Header lines such as `NUM_DVFSRC_OPP : 87` carry no `[NN]` label and are
/// skipped.
pub(super) fn parse_dvfsrc_opps(raw: &str) -> Vec<DvfsrcOpp> {
    let mut rows = Vec::new();
    for line in raw.lines() {
        let (Some(start), Some(end)) = (line.find('['), line.find(']')) else {
            continue;
        };
        let Ok(index) = line[start + 1..end].trim().parse::<u32>() else {
            continue;
        };
        let body = &line[end + 1..];
        let mut parts = body.split(',');
        let vcore = parts.next().and_then(|part| {
            part.trim()
                .trim_start_matches(':')
                .trim()
                .parse::<u32>()
                .ok()
        });
        let mbps = parts.next().and_then(|part| {
            part.split_whitespace()
                .next()
                .and_then(|value| value.parse::<u32>().ok())
        });
        let (Some(vcore), Some(mbps)) = (vcore, mbps) else {
            continue;
        };
        rows.push(DvfsrcOpp { index, vcore, mbps });
    }
    rows
}

/// The dvfsrc helper device directory, whose nodes take the force write.
pub(super) fn dvfsrc_helper_dir() -> Option<PathBuf> {
    let base = DVFSRC_ROOTS
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_dir())
        .or_else(|| {
            std::fs::read_dir("/sys/devices/platform")
                .ok()?
                .flatten()
                .map(|entry| entry.path())
                .find(|path| path.is_dir() && path.extension().is_some_and(|ext| ext == "dvfsrc"))
        })?;
    let name = base.file_name()?.to_str()?;
    Some(base.join(format!("{name}{DVFSRC_HELPER_SUFFIX}")))
}

fn switch_candidates(name: &str) -> &'static [&'static str] {
    match name {
        "fpsgo" => &[SWITCH_FPSGO],
        "fbt_ceiling" => &[SWITCH_FBT_CEILING],
        "perfmgr" => &SWITCH_PERFMGR,
        "ged_kpi" => &[SWITCH_GED_KPI],
        _ => &[],
    }
}

fn read_switch(name: &str) -> Option<bool> {
    switch_candidates(name)
        .iter()
        .find_map(|path| read_node_text(path).ok().map(|raw| raw.trim() == "1"))
}

/// The node this kernel exposes for a switch, in preference order.
pub(super) fn switch_path_in(name: &str, exists: impl Fn(&Path) -> bool) -> Option<&'static str> {
    switch_candidates(name)
        .iter()
        .copied()
        .find(|path| exists(Path::new(path)))
}

pub(super) fn switch_path(name: &str) -> Option<&'static str> {
    switch_path_in(name, |path| path.is_file())
}

pub(super) fn dcs_write_value(code: u32) -> String {
    code.to_string()
}

fn collect_input() -> Option<ControlInput> {
    let stack = read_node_text(MTK_STACK_OPPS).ok()?;
    let opps = parse_stack_opps(&stack);
    if opps.is_empty() {
        return None;
    }
    let pin = read_node_text(MTK_FIX_INDEX)
        .ok()
        .and_then(|raw| parse_fix_index(&raw))
        .and_then(|index| {
            opps.iter()
                .find(|row| row.index == index)
                .map(|row| (row.index, row.khz))
        });
    let thermal_gpt = read_node_text(MTK_THERMAL_GPT)
        .ok()
        .map(|raw| raw.trim() == "disable");
    let dcs = read_node_text(DCS_MODE)
        .ok()
        .map(|raw| parse_dcs_menu(&raw));
    let ddr = dvfsrc_helper_dir()
        .and_then(|dir| read_node_text(dir.join(DVFSRC_OPP_TABLE)).ok())
        .map(|raw| parse_dvfsrc_opps(&raw))
        .filter(|table| !table.is_empty());
    Some(ControlInput {
        gpu: Some(GpuInput {
            opps,
            pin,
            thermal_gpt,
        }),
        switches: [
            read_switch("fpsgo"),
            read_switch("fbt_ceiling"),
            read_switch("perfmgr"),
            read_switch("ged_kpi"),
        ],
        dcs,
        ddr,
    })
}

fn unsupported() -> Value {
    json!({
        "supported": false,
        "reason": "MediaTek gpufreqv2 nodes are not available",
    })
}

pub(super) fn render_control(input: Option<ControlInput>) -> Value {
    let Some(input) = input else {
        return unsupported();
    };
    let Some(gpu) = input.gpu else {
        return unsupported();
    };
    let opps = gpu
        .opps
        .iter()
        .map(|row| json!({"index": row.index, "khz": row.khz, "volt": row.volt}))
        .collect::<Vec<_>>();
    let pin = match gpu.pin {
        Some((index, khz)) => json!({"index": index, "khz": khz}),
        None => json!({"index": null, "khz": null}),
    };
    let thermal_gpt = match gpu.thermal_gpt {
        Some(disabled) => json!({"present": true, "disabled": disabled}),
        None => json!({"present": false}),
    };
    let switch = |value: Option<bool>| match value {
        Some(on) => json!({"present": true, "on": on}),
        None => json!({"present": false, "on": null}),
    };
    let dcs = match input.dcs {
        Some(menu) => json!({
            "present": true,
            "code": menu.current,
            "menu": menu.entries
                .iter()
                .map(|(code, label)| json!([code, label]))
                .collect::<Vec<_>>(),
        }),
        None => json!({"present": false}),
    };
    let ddr = match input.ddr {
        Some(table) => json!({
            "present": true,
            "readable": false,
            "opps": table
                .iter()
                .map(|row| json!({"index": row.index, "vcore": row.vcore, "mbps": row.mbps}))
                .collect::<Vec<_>>(),
        }),
        None => json!({"present": false}),
    };
    json!({
        "supported": true,
        "reason": null,
        "soc": read_node_text(MTK_SOC)
            .map(|raw| raw.trim().to_string())
            .unwrap_or_default(),
        "gpu": {
            "opps": opps,
            "pin": pin,
            "thermal_gpt": thermal_gpt,
        },
        "switches": {
            "fpsgo": switch(input.switches[0]),
            "fbt_ceiling": switch(input.switches[1]),
            "perfmgr": switch(input.switches[2]),
            "ged_kpi": switch(input.switches[3]),
        },
        "dcs": dcs,
        "ddr": ddr,
    })
}

pub(super) fn is_command(cmd: &str) -> bool {
    cmd == "GET mtk.control"
        || cmd.starts_with("ACTION mtk.gpu.pin ")
        || cmd.starts_with("ACTION mtk.gpu.pin_volt ")
        || cmd.starts_with("ACTION mtk.switch ")
        || cmd.starts_with("ACTION mtk.thermal_gpt ")
        || cmd.starts_with("ACTION mtk.dcs ")
        || cmd.starts_with("ACTION mtk.ddr.force_opp ")
        || cmd.starts_with("ACTION mtk.reset ")
}

fn ensure_enabled() -> Result<(), String> {
    if service_control::enabled() {
        Ok(())
    } else {
        Err("Dash Essential is disabled".into())
    }
}

fn write_node(path: &str, value: &str) -> Result<(), String> {
    service_control::write(path, value).map_err(|error| format!("{path}: {error}"))
}

/// The pinned index is live.
///
/// The comparison is on the index alone. The GPU idles below the table, so a
/// frequency check would fail while a correct pin settles.
/// The index the driver registered on the fix node, if any.
///
/// The node answers with a debug dump: `fix GPU/STACK OPP index: 39/39`
/// while a pin is registered, `... is disabled` otherwise. It never prints a
/// bare integer, so `trim().parse()` alone never sees the pin.
pub(super) fn parse_fix_index(raw: &str) -> Option<u32> {
    let line = raw.lines().find(|line| line.contains("OPP index"))?;
    let rest = line[line.find("OPP index")? + "OPP index".len()..].trim();
    let tail = rest.strip_prefix(':')?;
    let slash = tail.find('/')?;
    let gpu = tail[..slash].trim().parse::<u32>().ok()?;
    let stack = tail[slash + 1..].trim().parse::<u32>().ok()?;
    (gpu == stack).then_some(gpu)
}

pub(super) fn pin_verified(expected_index: u32, raw: &str) -> bool {
    parse_gpufreq_status(raw).is_some_and(|(index, _)| index == expected_index)
        || parse_fix_index(raw).is_some_and(|index| index == expected_index)
}

fn verify_pin(index: u32) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_millis(300);
    loop {
        // gpufreq_status confirms the live index only once the GPU actually
        // transitions; the fix node's readback confirms registration at
        // write time. Accept either.
        let status = read_node_text(MTK_STATUS).ok();
        let fix = read_node_text(MTK_FIX_INDEX).ok();
        if status.is_some_and(|raw| pin_verified(index, &raw))
            || fix.is_some_and(|raw| pin_verified(index, &raw))
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    rollback_pin();
    Err(format!("GPU pin verify failed: index {index} not live"))
}

/// Put back what the pin replaced.
///
/// The journal owns the pre-pin value when the node was readable; otherwise
/// the driver's own off value is the only safe state.
fn rollback_pin() {
    for (path, off) in [(MTK_FIX_INDEX, "-1"), (MTK_FIX_FREQ_VOLT, "0 0")] {
        let restored = match service_control::original(path) {
            Some(Value::String(value)) => service_control::write(path, &value),
            _ => service_control::write(path, off),
        };
        if let Err(error) = restored {
            crate::diagnostics::record(
                crate::diagnostics::AREA_GPU,
                "error",
                "mtk.pin.rollback",
                &format!("{path}: {error}"),
            );
        }
    }
    let _ = service_control::forget(&[MTK_FIX_INDEX.to_string(), MTK_FIX_FREQ_VOLT.to_string()]);
}

fn release_pin() -> Result<(), String> {
    write_node(MTK_FIX_INDEX, "-1")?;
    write_node(MTK_FIX_FREQ_VOLT, "0 0")?;
    let _ = service_control::forget(&[MTK_FIX_INDEX.to_string(), MTK_FIX_FREQ_VOLT.to_string()]);
    Ok(())
}

/// Pin one operating point, optionally at the table's own voltage.
///
/// A frequency the table does not list is an error, never a nearest match:
/// a wrong row pins an unrelated clock.
fn gpu_pin_with(khz: u32, volt: Option<u32>, rows: &[OppRow]) -> Result<(), String> {
    let index = pin_index_for_khz(rows, khz)
        .ok_or_else(|| format!("GPU OPP table has no {khz} kHz row"))?;
    let row = rows
        .iter()
        .find(|row| row.index == index)
        .ok_or_else(|| format!("GPU OPP table has no index {index}"))?;
    // Reject a bad voltage before disarming anything, so a user typo never
    // leaves the EB DVFS policy and cooling cap off with no pin in place.
    if let Some(volt) = volt
        && volt != row.volt
    {
        return Err(format!(
            "GPU OPP {khz} kHz lists voltage {}, not {volt}",
            row.volt
        ));
    }
    // The daemon owns the GPU through GPU profiles and Battery Saver while
    // the service is enabled; a hard pin must not fight them. Never write
    // custom_upbound_gpu_freq here — that write clears the pin.
    crate::disable_eb_dvfs_policy()?;
    crate::clear_gpu_cooling_cap();
    crate::ensure_gpu_thermal_mode()?;
    match volt {
        Some(volt) => {
            write_node(MTK_FIX_INDEX, "-1")?;
            write_node(MTK_FIX_FREQ_VOLT, &format!("{khz} {volt}"))?;
        }
        None => {
            write_node(MTK_FIX_FREQ_VOLT, "0 0")?;
            write_node(MTK_FIX_INDEX, &index.to_string())?;
        }
    }
    verify_pin(index)
}

fn gpu_pin(khz: u32, volt: Option<u32>) -> Result<(), String> {
    let stack =
        read_node_text(MTK_STACK_OPPS).map_err(|error| format!("{MTK_STACK_OPPS}: {error}"))?;
    let rows = parse_stack_opps(&stack);
    if rows.is_empty() {
        return Err("MediaTek OPP table is empty".into());
    }
    gpu_pin_with(khz, volt, &rows)
}

fn set_switch(name: &str, on: bool) -> Result<(), String> {
    let path = switch_path(name).ok_or_else(|| format!("{name} is not available"))?;
    write_node(path, if on { "1" } else { "0" })?;
    let live = read_node_text(path).map_err(|error| format!("{path}: {error}"))?;
    if (live.trim() == "1") != on {
        return Err(format!("{name} did not retain {}", if on { 1 } else { 0 }));
    }
    Ok(())
}

/// Disable `thermal/gpt`, or put back the configuration it replaced.
///
/// The enabled form is the driver's own scalar string, which the journal
/// captured before the disable; there is no `enable` token to invent.
fn set_thermal_gpt(disable: bool) -> Result<(), String> {
    if disable {
        return write_node(MTK_THERMAL_GPT, "disable");
    }
    let Some(Value::String(original)) = service_control::original(MTK_THERMAL_GPT) else {
        return Err("no journaled thermal configuration to restore".into());
    };
    write_node(MTK_THERMAL_GPT, &original)?;
    let _ = service_control::forget(&[MTK_THERMAL_GPT.to_string()]);
    Ok(())
}

fn set_dcs(code: u32) -> Result<(), String> {
    if code != 0 {
        let raw = read_node_text(DCS_MODE).map_err(|error| format!("{DCS_MODE}: {error}"))?;
        if !parse_dcs_menu(&raw)
            .entries
            .iter()
            .any(|(entry, _)| *entry == code)
        {
            return Err(format!("DCS menu does not offer code {code}"));
        }
    }
    write_node(DCS_MODE, &dcs_write_value(code))
}

/// How one control is released.
pub(super) struct ResetTarget {
    /// The value that turns the control off when nothing was journaled. An
    /// empty value means the control can only be released from its journal,
    /// because the kernel offers no off token to write.
    pub off: &'static str,
}

pub(super) fn reset_target(name: &str) -> Option<ResetTarget> {
    let off = match name {
        "gpu" => "-1",
        "thermal_gpt" => "",
        "fpsgo" | "fbt_ceiling" | "perfmgr" | "ged_kpi" | "dcs" => "0",
        "ddr" => DVFSRC_RELEASE,
        _ => return None,
    };
    Some(ResetTarget { off })
}

/// The DDR force node can be indexed only against the table the driver
/// publishes; the devfreq frequency list is a different index space.
pub(super) fn ddr_index_valid(index: u32, table: &[DvfsrcOpp]) -> bool {
    (index as usize) < table.len()
}

fn force_node_path(node: &str) -> Result<PathBuf, String> {
    let dir =
        dvfsrc_helper_dir().ok_or_else(|| "MediaTek dvfsrc helper is not available".to_string())?;
    Ok(dir.join(node))
}

/// Write a dvfsrc force node.
///
/// These nodes are write-only (`--w-------`), so the journal cannot read an
/// original and cannot replay it; release writes the off value explicitly.
fn write_force_node(node: &str, value: &str) -> Result<(), String> {
    let path = force_node_path(node)?;
    std::fs::write(&path, value).map_err(|error| format!("{}: {error}", path.display()))
}

fn ddr_force(index: Option<u32>) -> Result<(), String> {
    let dir =
        dvfsrc_helper_dir().ok_or_else(|| "MediaTek dvfsrc helper is not available".to_string())?;
    let value = match index {
        Some(index) => {
            // The force node is indexed against the combined (vcore, DDR)
            // table, so the selection is validated there, never against the
            // DDR-only table.
            let table_path = dir.join(DVFSRC_OPP_TABLE);
            let raw = read_node_text(&table_path)
                .map_err(|error| format!("{}: {error}", table_path.display()))?;
            if !ddr_index_valid(index, &parse_dvfsrc_opps(&raw)) {
                return Err(format!("DDR OPP table has no index {index}"));
            }
            index.to_string()
        }
        None => DVFSRC_RELEASE.to_string(),
    };
    write_force_node(DVFSRC_FORCE_VCORE, &value)
}

fn restore_node(path: &str, off: &str) -> Result<(), String> {
    match service_control::original(path) {
        Some(Value::String(value)) => {
            service_control::write(path, &value).map_err(|error| format!("{path}: {error}"))?
        }
        _ if off.is_empty() => {
            return Err(format!("no journaled value to restore for {path}"));
        }
        _ => {
            service_control::write(path, off).map_err(|error| format!("{path}: {error}"))?;
        }
    }
    let _ = service_control::forget(&[path.to_string()]);
    Ok(())
}

fn reset_control(name: &str) -> Result<(), String> {
    let target = reset_target(name).ok_or_else(|| format!("unknown mtk control {name}"))?;
    if name == "gpu" {
        restore_node(MTK_FIX_INDEX, "-1")?;
        restore_node(MTK_FIX_FREQ_VOLT, "0 0")?;
        return Ok(());
    }
    if name == "ddr" {
        for node in [DVFSRC_FORCE_VCORE, DVFSRC_FORCE_DDR] {
            write_force_node(node, target.off)?;
        }
        return Ok(());
    }
    let path = match name {
        "thermal_gpt" => MTK_THERMAL_GPT,
        "dcs" => DCS_MODE,
        switch => switch_path(switch).ok_or_else(|| format!("{switch} is not available"))?,
    };
    restore_node(path, target.off)
}

/// Release every write the daemon made to the dvfsrc force nodes.
///
/// The journaled nodes need no work here; the generic replay owns them. The
/// force nodes are write-only, so release writes the driver's `255` sentinel.
/// Every node is attempted even when one fails, so one stuck node cannot
/// leave the other forced.
pub(super) fn restore_all() -> Result<(), String> {
    let Some(dir) = dvfsrc_helper_dir() else {
        return Ok(());
    };
    let mut errors = Vec::new();
    for node in [DVFSRC_FORCE_VCORE, DVFSRC_FORCE_DDR] {
        let path = dir.join(node);
        if !path.exists() {
            continue;
        }
        if let Err(error) = std::fs::write(&path, DVFSRC_RELEASE) {
            errors.push(format!("{}: {error}", path.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub(super) fn command(cmd: &str) -> Result<Value, String> {
    if cmd == "GET mtk.control" {
        return Ok(render_control(collect_input()));
    }
    if let Some(arg) = cmd.strip_prefix("ACTION mtk.gpu.pin ") {
        ensure_enabled()?;
        let khz = arg
            .trim()
            .parse::<u32>()
            .map_err(|_| "bad GPU pin frequency".to_string())?;
        if khz == 0 {
            release_pin()?;
        } else {
            gpu_pin(khz, None)?;
        }
        return Ok(render_control(collect_input()));
    }
    if let Some(arg) = cmd.strip_prefix("ACTION mtk.gpu.pin_volt ") {
        ensure_enabled()?;
        let parts = arg.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err("expected frequency and voltage".into());
        }
        let khz = parts[0]
            .parse::<u32>()
            .map_err(|_| "bad GPU pin frequency".to_string())?;
        let volt = parts[1]
            .parse::<u32>()
            .map_err(|_| "bad GPU pin voltage".to_string())?;
        gpu_pin(khz, Some(volt))?;
        return Ok(render_control(collect_input()));
    }
    if let Some(arg) = cmd.strip_prefix("ACTION mtk.switch ") {
        ensure_enabled()?;
        let parts = arg.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 2 {
            return Err("expected switch name and 0 or 1".into());
        }
        let on = match parts[1] {
            "0" => false,
            "1" => true,
            _ => return Err("switch value must be 0 or 1".into()),
        };
        set_switch(parts[0], on)?;
        return Ok(render_control(collect_input()));
    }
    if let Some(arg) = cmd.strip_prefix("ACTION mtk.thermal_gpt ") {
        ensure_enabled()?;
        let disable = match arg.trim() {
            "disable" => true,
            "restore" => false,
            _ => return Err("expected disable or restore".into()),
        };
        set_thermal_gpt(disable)?;
        return Ok(render_control(collect_input()));
    }
    if let Some(arg) = cmd.strip_prefix("ACTION mtk.dcs ") {
        ensure_enabled()?;
        let code = arg
            .trim()
            .parse::<u32>()
            .map_err(|_| "bad DCS code".to_string())?;
        set_dcs(code)?;
        return Ok(render_control(collect_input()));
    }
    if let Some(arg) = cmd.strip_prefix("ACTION mtk.ddr.force_opp ") {
        ensure_enabled()?;
        let index = match arg.trim() {
            "off" => None,
            raw => Some(
                raw.parse::<u32>()
                    .map_err(|_| "bad DDR OPP index".to_string())?,
            ),
        };
        ddr_force(index)?;
        return Ok(render_control(collect_input()));
    }
    if let Some(arg) = cmd.strip_prefix("ACTION mtk.reset ") {
        ensure_enabled()?;
        reset_control(arg.trim())?;
        return Ok(render_control(collect_input()));
    }
    Err("unsupported mtk command".into())
}

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
        assert_eq!(
            (rows[0].index, rows[0].khz, rows[0].volt),
            (0, 1612000, 79500)
        );
        assert_eq!(
            (rows[4].index, rows[4].khz, rows[4].volt),
            (51, 338000, 49375)
        );
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
        let menu = parse_dcs_menu(menu);
        assert_eq!(menu.current, Some(0));
        assert_eq!(
            menu.entries,
            vec![(1u32, "Achieve 6/11 Freq --> 64".to_string())]
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

    #[test]
    fn pin_verification_accepts_the_written_index_while_the_gpu_idles() {
        let status = "[GPU   OPP]      Index: 51, Freq:   26000, Volt:  49375, Vsram:  75000";
        assert!(pin_verified(51, status));
        assert!(!pin_verified(50, status));
    }

    #[test]
    fn pin_verification_accepts_the_driver_fix_readback_when_the_gpu_idles() {
        // Verified on device (mt6991): after `echo 39 > fix_target_opp_index`
        // the node reads back `fix GPU/STACK OPP index: 39/39` while
        // `gpufreq_status` keeps `Index: 51` because the idle GPU never
        // transitions. The readback is the driver's own confirmation.
        let readback = "[GPUFREQ-DEBUG] fix GPU/STACK OPP index: 39/39\n\
                        [GPUFREQ-DEBUG] fix GPU F/V, STACK F/V is disabled";
        assert!(pin_verified(39, readback));
        assert!(!pin_verified(40, readback));
        assert!(!pin_verified(
            39,
            "[GPUFREQ-DEBUG] fix GPU/STACK OPP index is disabled"
        ));
    }

    #[test]
    fn the_fix_node_readback_reports_the_current_pin() {
        // GET mtk.control must show the active pin; the node prints a dump,
        // never a bare integer (verified on device).
        let readback = "[GPUFREQ-DEBUG] fix GPU/STACK OPP index: 39/39";
        assert_eq!(parse_fix_index(readback), Some(39));
        assert_eq!(
            parse_fix_index("[GPUFREQ-DEBUG] fix GPU/STACK OPP index is disabled"),
            None
        );
        assert_eq!(parse_fix_index("fix GPU/STACK OPP index: 39/40"), None);
        assert_eq!(parse_fix_index(""), None);
    }

    #[test]
    fn pin_rejects_an_unknown_frequency_without_writing() {
        let rows = parse_stack_opps(STACK);
        assert_eq!(pin_index_for_khz(&rows, 999000), None);
        assert!(gpu_pin_with(999000, None, &rows).is_err());
        assert!(gpu_pin_with(1612000, Some(1), &rows).is_err());
    }

    #[test]
    fn perfmgr_prefers_the_node_this_kernel_exposes() {
        let mtk = Path::new(SWITCH_PERFMGR[0]);
        assert_eq!(
            switch_path_in("perfmgr", |path| path == mtk),
            Some(SWITCH_PERFMGR[0])
        );
        let pandora = Path::new(SWITCH_PERFMGR[1]);
        assert_eq!(
            switch_path_in("perfmgr", |path| path == pandora),
            Some(SWITCH_PERFMGR[1])
        );
        assert_eq!(switch_path_in("perfmgr", |_| false), None);
        assert_eq!(switch_path_in("nonsense", |_| true), None);
    }

    #[test]
    fn dcs_zero_disables_and_any_code_enables() {
        assert_eq!(dcs_write_value(0), "0");
        assert_eq!(dcs_write_value(64), "64");
    }

    #[test]
    fn parses_the_dvfsrc_combined_opp_table() {
        let rows = parse_dvfsrc_opps(
            "NUM_VCORE_OPP : 6\nNUM_DDR_OPP : 11\nNUM_DVFSRC_OPP : 87\n\n\
             [0  ]:875000, 9594  Mbps [5]\n\
             [44 ]:760000, 8528  Mbps [4]\n\
             [86 ]:685000, 3094  Mbps [1]",
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(
            (rows[0].index, rows[0].vcore, rows[0].mbps),
            (0, 875000, 9594)
        );
        assert_eq!(
            (rows[1].index, rows[1].vcore, rows[1].mbps),
            (44, 760000, 8528)
        );
        assert_eq!(
            (rows[2].index, rows[2].vcore, rows[2].mbps),
            (86, 685000, 3094)
        );
    }

    #[test]
    fn ddr_force_rejects_an_index_past_the_table() {
        let table = parse_dvfsrc_opps("[0  ]:875000, 9594  Mbps [5]\n[86 ]:685000, 3094  Mbps [1]");
        assert_eq!(table.len(), 2);
        assert!(ddr_index_valid(1, &table));
        assert!(!ddr_index_valid(2, &table));
        // 255 is the driver's release sentinel, never an OPP index.
        assert!(!ddr_index_valid(255, &table));
    }

    #[test]
    fn actions_require_the_service_to_be_enabled() {
        // `state_dir()` resolves once per process, so the gate is exercised
        // in a child that sees only its own state directory holding an
        // explicit `{"enabled": false}` intent. A fresh directory alone
        // would report enabled (a missing intent file is not a lockout).
        let directory = std::env::temp_dir().join(format!("dash-mtk-gate-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("service.json"), "{\"enabled\": false}").unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "mtk_control::tests::actions_require_the_service_to_be_enabled_child",
            ])
            .env("DASH_STATE_DIR", &directory)
            .env("DASH_MTK_GATE_CHILD", "1")
            .output()
            .expect("spawn the gate child");
        std::fs::remove_dir_all(&directory).ok();
        assert!(
            output.status.success(),
            "gate child failed: {} | stdout: {}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[test]
    fn actions_require_the_service_to_be_enabled_child() {
        if std::env::var_os("DASH_MTK_GATE_CHILD").is_none() {
            // Parent run: the real assertions live in the child process.
            return;
        }
        // The gate must fire before any node write is attempted.
        assert_eq!(
            command("ACTION mtk.switch fpsgo 1").unwrap_err(),
            "Dash Essential is disabled"
        );
        assert_eq!(
            command("ACTION mtk.gpu.pin 598000").unwrap_err(),
            "Dash Essential is disabled"
        );
        assert_eq!(
            command("ACTION mtk.reset ddr").unwrap_err(),
            "Dash Essential is disabled"
        );
    }

    #[test]
    fn reset_names_are_the_ones_the_ui_sends() {
        for name in [
            "gpu",
            "thermal_gpt",
            "fpsgo",
            "fbt_ceiling",
            "perfmgr",
            "ged_kpi",
            "dcs",
            "ddr",
        ] {
            assert!(reset_target(name).is_some(), "{name}");
        }
        assert!(reset_target("nonsense").is_none());
        // Verified on device: FORCE_OPP_IDX reads `255, 255` when unset, and
        // writing 255 to the force node clears a force. `0` would pin the
        // lowest OPP instead of releasing it.
        assert_eq!(reset_target("ddr").unwrap().off, "255");
        assert_eq!(reset_target("gpu").unwrap().off, "-1");
    }
}
