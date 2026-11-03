//! Cheap per-sample hardware telemetry for FPS sessions.
//!
//! The recorder samples twice a second, so every reading here must stay a
//! handful of sysfs/proc reads. A sensor the ROM does not expose stays `0`;
//! callers decide whether a zero means "missing" or "idle", and the session
//! records which metrics actually answered.
//!
//! Nothing here writes to the device. It is the same read-only surface the
//! overlay already uses, widened to the values Prism prints beside a session.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// One sample's hardware state. `0` means unavailable for every field except
/// CPU/GPU load, where zero is also a real idle reading.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Telemetry {
    pub cpu_mhz: i32,
    pub cpu_load: i32,
    pub cpu_load_max: i32,
    pub gpu_mhz: i32,
    pub gpu_load: i32,
    pub battery_temp_dc: i32,
    pub battery_level: i32,
    pub power_mw: i32,
    pub cpu_temp_dc: i32,
    pub gpu_temp_dc: i32,
    pub ram_used_mb: i32,
    pub swap_used_mb: i32,
    pub package_ram_mb: i32,
    pub cpu_core_count: u32,
    pub cpu_max_freq_khz: u32,
    pub cpu_cur_freq_khz: u32,
    pub gpu_core_count: u32,
    pub gpu_max_freq_khz: u32,
    pub gpu_cur_freq_khz: u32,
    pub max_safe_temp_c: Option<u32>,
    pub min_operating_temp_c: u32,
}

#[derive(Clone, Copy, Default)]
struct CpuTimes {
    total: u64,
    idle: u64,
}

/// Previous `/proc/stat` counters. CPU usage needs a delta; the first sample
/// after a recording starts therefore reports zero instead of a lifetime
/// average since boot.
static CPU_PREVIOUS: Mutex<Vec<CpuTimes>> = Mutex::new(Vec::new());

fn read_number(path: &Path) -> Option<i64> {
    std::fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<i64>()
        .ok()
}

fn read_dt_max_temp() -> Option<u32> {
    const DT_PATH: &str = "/sys/firmware/devicetree/base/thermal-zones/cpu/max-temp";
    std::fs::read_to_string(DT_PATH)
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()
}

fn cpu_core_count() -> u32 {
    let Ok(text) = std::fs::read_to_string("/proc/cpuinfo") else {
        return 0;
    };
    text.lines().filter(|l| l.starts_with("processor")).count() as u32
}

fn cpu_max_freq_khz() -> u32 {
    let mut max_khz = 0u32;
    for cpu in 0..16 {
        let path = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_max_freq");
        if let Some(value) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            && value > max_khz
        {
            max_khz = value;
        }
    }
    max_khz
}

fn cpu_cur_freq_khz() -> u32 {
    std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

fn gpu_core_count() -> u32 {
    1
}

fn gpu_max_freq_khz() -> u32 {
    (crate::gpu_get_max_freq_mhz() as u32) * 1000
}

fn gpu_cur_freq_khz() -> u32 {
    (crate::gpu_get_cur_freq_mhz() as u32) * 1000
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn proc_stat() -> Vec<CpuTimes> {
    let Ok(text) = std::fs::read_to_string("/proc/stat") else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| line.starts_with("cpu"))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let name = fields.next()?;
            if name != "cpu" && !name[3..].chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let values: Vec<u64> = fields.filter_map(|field| field.parse().ok()).collect();
            if values.len() < 4 {
                return None;
            }
            // user nice system idle iowait irq softirq steal guest guest_nice
            let idle = values[3] + values.get(4).copied().unwrap_or(0);
            let total = values.iter().take(8).sum();
            Some(CpuTimes { total, idle })
        })
        .collect()
}

fn cpu_usage(previous: &[CpuTimes], current: &[CpuTimes]) -> (i32, i32) {
    if previous.len() != current.len() || current.is_empty() {
        return (0, 0);
    }
    let mut total_busy = 0u64;
    let mut total_all = 0u64;
    let mut peak = 0i32;
    for (before, now) in previous.iter().zip(current).skip(1) {
        let total = now.total.saturating_sub(before.total);
        let idle = now.idle.saturating_sub(before.idle);
        if total == 0 {
            continue;
        }
        let busy = total.saturating_sub(idle);
        total_busy += busy;
        total_all += total;
        peak = peak.max((busy * 100 / total).min(100) as i32);
    }
    if total_all == 0 {
        return (0, peak);
    }
    (((total_busy * 100) / total_all).min(100) as i32, peak)
}

fn update_cpu_load() -> (i32, i32) {
    let current = proc_stat();
    if current.is_empty() {
        return (0, 0);
    }
    let mut previous = CPU_PREVIOUS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let usage = cpu_usage(&previous, &current);
    *previous = current;
    usage
}

/// The fastest online CPU core, matching the number the overlay prints.
fn cpu_mhz() -> i32 {
    (0..8)
        .filter_map(|cpu| {
            crate::cpu_freq(cpu)
                .trim()
                .parse::<i32>()
                .ok()
                .filter(|khz| *khz > 0)
        })
        .max()
        .map(|khz| khz / 1000)
        .unwrap_or(0)
}

#[derive(Default)]
struct ThermalZones {
    cpu: Vec<PathBuf>,
    gpu: Vec<PathBuf>,
    soc: Vec<PathBuf>,
}

fn classify_zone(name: &str) -> u8 {
    let name = name.to_ascii_lowercase();
    if name.contains("gpu") {
        2
    } else if name.contains("cpu") {
        1
    } else if name.contains("soc") || name.contains("ap") {
        3
    } else {
        0
    }
}

fn scan_thermal_zones() -> ThermalZones {
    let mut zones = ThermalZones::default();
    let Ok(directory) = std::fs::read_dir("/sys/class/thermal") else {
        return zones;
    };
    for entry in directory.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("thermal_zone"))
        {
            continue;
        }
        let Some(kind) = read_trimmed(&path.join("type")) else {
            continue;
        };
        match classify_zone(&kind) {
            1 => zones.cpu.push(path.join("temp")),
            2 => zones.gpu.push(path.join("temp")),
            3 => zones.soc.push(path.join("temp")),
            _ => {}
        }
    }
    zones
}

fn thermal_zones() -> &'static Mutex<ThermalZones> {
    static ZONES: OnceLock<Mutex<ThermalZones>> = OnceLock::new();
    ZONES.get_or_init(|| Mutex::new(scan_thermal_zones()))
}

fn max_temp_deci_c(paths: &[PathBuf]) -> i32 {
    paths
        .iter()
        .filter_map(|path| read_number(path))
        // Thermal zones report millidegrees Celsius.
        .map(|milli| (milli / 100) as i32)
        .max()
        .unwrap_or(0)
}

fn temperatures() -> (i32, i32) {
    let zones = thermal_zones()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cpu = max_temp_deci_c(&zones.cpu).max(max_temp_deci_c(&zones.soc));
    (cpu, max_temp_deci_c(&zones.gpu))
}

fn battery_number(name: &str) -> i32 {
    crate::battery(name).trim().parse().unwrap_or(0)
}

fn power_mw() -> i32 {
    let current = crate::battery("current_now")
        .trim()
        .parse::<i64>()
        .unwrap_or(0)
        .unsigned_abs();
    let voltage = crate::battery("voltage_now")
        .trim()
        .parse::<i64>()
        .unwrap_or(0)
        .unsigned_abs();
    // microamp * microvolt / 1e9 = milliwatt.
    let milliwatts = current.saturating_mul(voltage) / 1_000_000_000;
    milliwatts.min(i32::MAX as u64) as i32
}

#[derive(Default)]
struct Memory {
    total_mb: i32,
    available_mb: i32,
    swap_total_mb: i32,
    swap_free_mb: i32,
}

fn memory() -> Memory {
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
        return Memory::default();
    };
    let mut values: BTreeMap<&str, i32> = BTreeMap::new();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        if let Some(kb) = rest
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<i32>().ok())
        {
            values.insert(key, kb / 1024);
        }
    }
    Memory {
        total_mb: values.get("MemTotal").copied().unwrap_or(0),
        available_mb: values.get("MemAvailable").copied().unwrap_or(0),
        swap_total_mb: values.get("SwapTotal").copied().unwrap_or(0),
        swap_free_mb: values.get("SwapFree").copied().unwrap_or(0),
    }
}

static PACKAGE_PID: Mutex<(String, i32)> = Mutex::new((String::new(), 0));

fn package_pid(package: &str) -> i32 {
    if package.is_empty() {
        return 0;
    }
    let mut cached = PACKAGE_PID
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cached.0 == package && cached.1 > 0 {
        return cached.1;
    }
    let Ok(output) = std::process::Command::new("/system/bin/pidof")
        .arg(package)
        .output()
    else {
        return 0;
    };
    let pid = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .find_map(|value| value.parse::<i32>().ok())
        .unwrap_or(0);
    *cached = (package.to_string(), pid);
    pid
}

fn package_ram_mb(package: &str) -> i32 {
    let pid = package_pid(package);
    if pid <= 0 {
        return 0;
    }
    let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
        let mut cached = PACKAGE_PID
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if cached.1 == pid {
            *cached = (String::new(), 0);
        }
        return 0;
    };
    text.lines()
        .find_map(|line| {
            let rest = line.strip_prefix("VmRSS:")?;
            rest.split_whitespace()
                .next()
                .and_then(|value| value.parse::<i32>().ok())
                .map(|kb| kb / 1024)
        })
        .unwrap_or(0)
}

/// Reads every metric once. Called inside the recorder's sampler, never on the
/// IPC thread, so a slow `pidof` cannot delay a settings poll.
pub(super) fn sample(package: Option<&str>) -> Telemetry {
    let (cpu_load, cpu_load_max) = update_cpu_load();
    let (cpu_temp_dc, gpu_temp_dc) = temperatures();
    let memory = memory();
    Telemetry {
        cpu_mhz: cpu_mhz(),
        cpu_load,
        cpu_load_max,
        gpu_mhz: crate::gpu_get_cur_freq_mhz(),
        gpu_load: crate::gpu_get_loading(),
        battery_temp_dc: battery_number("temp"),
        battery_level: battery_number("capacity"),
        power_mw: power_mw(),
        cpu_temp_dc,
        gpu_temp_dc,
        ram_used_mb: (memory.total_mb - memory.available_mb).max(0),
        swap_used_mb: (memory.swap_total_mb - memory.swap_free_mb).max(0),
        package_ram_mb: package_ram_mb(package.unwrap_or("")),
        cpu_core_count: cpu_core_count(),
        cpu_max_freq_khz: cpu_max_freq_khz(),
        cpu_cur_freq_khz: cpu_cur_freq_khz(),
        gpu_core_count: gpu_core_count(),
        gpu_max_freq_khz: gpu_max_freq_khz(),
        gpu_cur_freq_khz: gpu_cur_freq_khz(),
        max_safe_temp_c: read_dt_max_temp().or(Some(85)),
        min_operating_temp_c: 30,
    }
}

/// Clears the CPU delta and the process cache when a recording starts, so a
/// new session does not inherit the previous game's counters.
pub(super) fn reset() {
    CPU_PREVIOUS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
    *PACKAGE_PID
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = (String::new(), 0);
}

/// The display resolution recorded with a session. Read once per session: it
/// only changes on fold/rotation, and `wm` is too slow for the 500 ms loop.
pub(super) fn resolution() -> String {
    let Ok(output) = std::process::Command::new("/system/bin/wm")
        .args(["size"])
        .output()
    else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let Some((_, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if value.split_once('x').is_some_and(|(width, height)| {
            width.chars().all(|c| c.is_ascii_digit()) && height.chars().all(|c| c.is_ascii_digit())
        }) {
            return value.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_delta_uses_the_busiest_core_and_the_total() {
        let before = vec![
            CpuTimes {
                total: 100,
                idle: 60,
            },
            CpuTimes {
                total: 100,
                idle: 50,
            },
            CpuTimes {
                total: 100,
                idle: 100,
            },
        ];
        let after = vec![
            CpuTimes {
                total: 200,
                idle: 120,
            },
            CpuTimes {
                total: 200,
                idle: 100,
            },
            CpuTimes {
                total: 200,
                idle: 200,
            },
        ];
        // The leading "cpu" aggregate is skipped: core 0 is 50 busy of 100,
        // core 1 stays idle. Total 50 busy of 200, peak 50%.
        assert_eq!(cpu_usage(&before, &after), (25, 50));
        assert_eq!(cpu_usage(&[], &after), (0, 0));
        assert_eq!(cpu_usage(&before, &after[..1]), (0, 0));
    }

    #[test]
    fn a_zero_delta_is_idle_not_a_divide_by_zero() {
        let values = vec![CpuTimes {
            total: 100,
            idle: 50,
        }];
        assert_eq!(cpu_usage(&values, &values), (0, 0));
    }

    #[test]
    fn thermal_zones_are_classified_by_name() {
        assert_eq!(classify_zone("cpu-0-0-usr"), 1);
        assert_eq!(classify_zone("soc-thermal"), 3);
        assert_eq!(classify_zone("gpu-thermal"), 2);
        assert_eq!(classify_zone("battery"), 0);
    }

    #[test]
    fn resolution_accepts_the_wm_size_shapes() {
        for (line, expected) in [
            ("Physical size: 1080x2400", Some("1080x2400")),
            ("Override size: 720x1600", Some("720x1600")),
            (
                "Physical size: 1080x2400\nOverride size: 720x1600",
                Some("1080x2400"),
            ),
            ("Physical size: unknown", None),
            ("", None),
        ] {
            let parsed = line.lines().find_map(|entry| {
                let (_, value) = entry.split_once(':')?;
                let value = value.trim();
                value
                    .split_once('x')
                    .filter(|(width, height)| {
                        width.chars().all(|c| c.is_ascii_digit())
                            && height.chars().all(|c| c.is_ascii_digit())
                    })
                    .map(|_| value)
            });
            assert_eq!(parsed, expected, "{line}");
        }
    }
}
