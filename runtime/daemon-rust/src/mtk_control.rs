//! MediaTek gpufreqv2 control.
//!
//! Two operating-point tables are published and they do not agree:
//! `gpu_working_opp_table` and `stack_signed_opp_table` both label 52 rows
//! `[00]`..`[51]`, but the same index names a different frequency in each.
//! `fix_target_opp_index` is written with the index printed by the *stack*
//! table, and every write is read back from `gpufreq_status` before it is
//! reported as applied.

use super::read_node_text;
use serde_json::{Value, json};
use std::path::PathBuf;

const MTK_STACK_OPPS: &str = "/proc/gpufreqv2/stack_signed_opp_table";
#[allow(dead_code)] // used by the pin and force flows added in later tasks
const MTK_STATUS: &str = "/proc/gpufreqv2/gpufreq_status";
const MTK_FIX_INDEX: &str = "/proc/gpufreqv2/fix_target_opp_index";
#[allow(dead_code)] // used by the pin and force flows added in later tasks
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
const DVFSRC_DDR_TABLE: &str = "dvfsrc_ddr_opp_table";
#[allow(dead_code)] // used by the pin and force flows added in later tasks
const DVFSRC_FORCE_VCORE: &str = "dvfsrc_force_vcore_dvfs_opp";

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
    pub ddr: Option<Vec<u64>>,
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
#[allow(dead_code)] // used by the pin and force flows added in later tasks
pub(super) fn pin_index_for_khz(opps: &[OppRow], khz: u32) -> Option<u32> {
    opps.iter().find(|row| row.khz == khz).map(|row| row.index)
}

/// The GPU's live index and frequency, in kilohertz.
///
/// The GPU idles below the table, so this is only used to confirm an index
/// that was just written, never to learn the table.
#[allow(dead_code)] // used by the pin verification added in a later task
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

pub(super) fn parse_dvfsrc_opp_table(raw: &str) -> Vec<u64> {
    raw.split_whitespace()
        .filter_map(|value| value.parse().ok())
        .collect()
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

fn collect_input() -> Option<ControlInput> {
    let stack = read_node_text(MTK_STACK_OPPS).ok()?;
    let opps = parse_stack_opps(&stack);
    if opps.is_empty() {
        return None;
    }
    let pin = read_node_text(MTK_FIX_INDEX)
        .ok()
        .and_then(|raw| raw.trim().parse::<i64>().ok())
        .filter(|index| *index >= 0)
        .and_then(|index| {
            opps.iter()
                .find(|row| row.index == index as u32)
                .map(|row| (row.index, row.khz))
        });
    let thermal_gpt = read_node_text(MTK_THERMAL_GPT)
        .ok()
        .map(|raw| raw.trim() == "disable");
    let dcs = read_node_text(DCS_MODE)
        .ok()
        .map(|raw| parse_dcs_menu(&raw));
    let ddr = dvfsrc_helper_dir()
        .and_then(|dir| read_node_text(dir.join(DVFSRC_DDR_TABLE)).ok())
        .map(|raw| parse_dvfsrc_opp_table(&raw))
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
        Some(table) => json!({"present": true, "readable": false, "opps": table}),
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
}

pub(super) fn command(cmd: &str) -> Result<Value, String> {
    if cmd == "GET mtk.control" {
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
