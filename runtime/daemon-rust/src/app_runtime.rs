//! Android adapters for Per-App Controls. No UI process or root prompt owns
//! profile lifetimes; the authenticated daemon does.
use super::app_controls::{
    App, Capabilities, Config, Control, Hardware, Ownership, Profile, Status, Target,
};
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc,
};

thread_local! { static TRANSIENT: Cell<bool> = const { Cell::new(false) }; }
pub(super) fn transient_write() -> bool {
    TRANSIENT.with(Cell::get)
}
struct Temporary(bool);
impl Temporary {
    fn begin() -> Self {
        Self(TRANSIENT.with(|v| v.replace(true)))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        TRANSIENT.with(|v| v.set(self.0));
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    state: PersistedState,
    #[serde(default)]
    live: Value,
}
static ORIGINALS: OnceLock<Mutex<BTreeMap<String, Saved>>> = OnceLock::new();
static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();
/// What the reader learned about a foreground change.
#[derive(Clone, Debug)]
pub(super) enum Wake {
    /// A resume event named the app that took the foreground. The event
    /// carries the user and the component, so the foreground is known without
    /// asking the framework for it.
    Resumed(App),
    /// The foreground may have changed and the authoritative source has to be
    /// read.
    Changed,
}

static WAKE: OnceLock<mpsc::SyncSender<Wake>> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);
static RECOVERY_READY: AtomicBool = AtomicBool::new(false);

struct Runtime {
    config: Config,
    config_error: Option<String>,
    owner: Ownership<Saved>,
    foreground: Option<App>,
    error: Option<String>,
    watching: bool,
    /// Where the last foreground answer came from. A resume event names the
    /// app outright; anything else had to be read from the framework.
    source: &'static str,
}
fn runtime() -> &'static Mutex<Runtime> {
    RUNTIME.get_or_init(|| {
        let loaded = Config::load(&state_dir().join("app-profiles.json"));
        let config_error = loaded.as_ref().err().cloned();
        Mutex::new(Runtime {
            config: loaded.unwrap_or_default(),
            config_error,
            owner: Ownership::default(),
            foreground: None,
            error: None,
            watching: false,
            source: "none",
        })
    })
}
fn originals() -> &'static Mutex<BTreeMap<String, Saved>> {
    ORIGINALS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn key(control: &Control) -> String {
    match control {
        Control::Touch => "touch".into(),
        Control::Gpu => "gpu".into(),
        Control::Refresh => "refresh".into(),
        Control::CpuCores => "cores".into(),
        Control::CpuRange(p) => format!("range{p}"),
        Control::CpuGovernor(p) => format!("governor{p}"),
    }
}
fn control(key: &str) -> Option<Control> {
    match key {
        "touch" => Some(Control::Touch),
        "gpu" => Some(Control::Gpu),
        "refresh" => Some(Control::Refresh),
        "cores" => Some(Control::CpuCores),
        _ => [0, 4, 7].into_iter().find_map(|p| {
            if key == format!("range{p}") {
                Some(Control::CpuRange(p))
            } else if key == format!("governor{p}") {
                Some(Control::CpuGovernor(p))
            } else {
                None
            }
        }),
    }
}
fn copy_preference(c: &Control, from: &PersistedState, to: &mut PersistedState) {
    match c {
        Control::Touch => to.touch = from.touch,
        Control::CpuCores => {
            to.cpu_manual = from.cpu_manual;
            to.cpu_online_mask = from.cpu_online_mask;
        }
        Control::CpuRange(0) => {
            to.cpu_min_freq0 = from.cpu_min_freq0;
            to.cpu_max_freq0 = from.cpu_max_freq0;
        }
        Control::CpuRange(4) => {
            to.cpu_min_freq4 = from.cpu_min_freq4;
            to.cpu_max_freq4 = from.cpu_max_freq4;
        }
        Control::CpuRange(7) => {
            to.cpu_min_freq7 = from.cpu_min_freq7;
            to.cpu_max_freq7 = from.cpu_max_freq7;
        }
        Control::CpuGovernor(0) => to.cpu0 = from.cpu0.clone(),
        Control::CpuGovernor(4) => to.cpu4 = from.cpu4.clone(),
        Control::CpuGovernor(7) => to.cpu7 = from.cpu7.clone(),
        Control::Gpu => {
            to.perf = from.perf;
            to.gpu = from.gpu.clone();
            to.gpu_profile_cpu_isolated = from.gpu_profile_cpu_isolated;
            to.gpu_min_freq_mhz = from.gpu_min_freq_mhz;
            to.gpu_max_freq_mhz = from.gpu_max_freq_mhz;
            to.gpu_governor = from.gpu_governor.clone();
            to.gpu_ged_boost = from.gpu_ged_boost;
            to.gpu_uncap = from.gpu_uncap;
            to.gpu_power_policy = from.gpu_power_policy.clone();
        }
        _ => {}
    }
}
pub(super) fn global_preferences(effective: &PersistedState) -> Result<PersistedState, String> {
    let entries = originals()
        .lock()
        .map_err(|_| "per-app originals lock poisoned")?;
    let mut global = effective.clone();
    for (name, saved) in entries.iter() {
        if let Some(c) = control(name) {
            copy_preference(&c, &saved.state, &mut global);
        }
        if name.starts_with("range") {
            global.cpu_thermal_mode_prev = saved.state.cpu_thermal_mode_prev;
        }
    }
    Ok(global)
}
fn atomic_json(path: &Path, value: &Value) -> Result<(), String> {
    let dir = path.parent().ok_or("missing state directory")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .custom_flags(0x20000)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|e| e.to_string())?;
    file.write_all(value.to_string().as_bytes())
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())?;
    fs::File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
fn journal(entries: &BTreeMap<String, Saved>) -> Result<(), String> {
    atomic_json(
        &state_dir().join("app-recovery.json"),
        &serde_json::to_value(entries).map_err(|e| e.to_string())?,
    )
}
fn cli(program: &str, args: &[&str]) -> Result<String, String> {
    let result = run_process_with_timeout(program, args, Duration::from_secs(3))?;
    let stderr = String::from_utf8_lossy(&result.stderr);
    if !result.status.success()
        || stderr.contains("Exception")
        || stderr.contains("Permission Denial")
    {
        return Err(format!(
            "Android command rejected: {} {}",
            result.status,
            stderr.trim()
        ));
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
}
fn current_user() -> Result<u32, String> {
    cli("/system/bin/cmd", &["activity", "get-current-user"])?
        .parse()
        .map_err(|_| "current Android user unavailable".into())
}
fn settings(user: u32, op: &str, key: &str, value: Option<&str>) -> Result<String, String> {
    let user = user.to_string();
    let mut args = vec!["settings", "--user", &user, op, "system", key];
    if let Some(value) = value {
        args.push(value);
    }
    cli("/system/bin/cmd", &args)
}

struct Adapter {
    captured: BTreeMap<String, Saved>,
}
impl Adapter {
    fn new() -> Self {
        Self {
            captured: BTreeMap::new(),
        }
    }
}
impl Hardware for Adapter {
    type Saved = Saved;
    fn capture(&mut self, c: &Control) -> Result<Saved, String> {
        let effective = persisted_state()
            .lock()
            .map_err(|_| "state lock poisoned")?
            .clone();
        let state = global_preferences(&effective)?;
        let live = match c {
            Control::CpuRange(p) => json!([
                read_cpu_cluster_limit(*p as i32, "scaling_min_freq")?,
                read_cpu_cluster_limit(*p as i32, "scaling_max_freq")?,
                read_mi_thermal_config_mode().unwrap_or(-1)
            ]),
            Control::CpuGovernor(p) => json!(read_trimmed(format!(
                "/sys/devices/system/cpu/cpufreq/policy{p}/scaling_governor"
            ))?),
            Control::Refresh => {
                let user = current_user()?;
                json!({"user":user,"min":settings(user,"get","min_refresh_rate",None)?,"max":settings(user,"get","peak_refresh_rate",None)?,
                    "preferred":cli("/system/bin/cmd",&["display","get-user-preferred-display-mode","0"])?,
                    "vendor":capture_vendor_refresh(user)?})
            }
            _ => Value::Null,
        };
        let saved = Saved { state, live };
        self.captured.insert(key(c), saved.clone());
        Ok(saved)
    }
    fn apply(&mut self, c: &Control, target: &Target) -> Result<(), String> {
        let name = key(c);
        {
            let mut entries = originals().lock().map_err(|_| "originals lock poisoned")?;
            if !entries.contains_key(&name) {
                let saved = self
                    .captured
                    .get(&name)
                    .ok_or("missing per-app recovery original")?;
                let mut candidate = entries.clone();
                candidate.insert(name.clone(), saved.clone());
                journal(&candidate)?;
                *entries = candidate;
            }
        }
        let _temporary = Temporary::begin();
        match (c, target) {
            (Control::Touch, Target::Touch(v)) => set_touch_profile(*v as i32),
            (Control::Gpu, Target::Gpu(v)) => apply_performance_profile_borrowed(*v as i32),
            (Control::CpuRange(p), Target::CpuRange { min, max }) => {
                set_cpu_cluster_freq_range(*p as i32, *min as i32, *max as i32)
            }
            (Control::CpuGovernor(p), Target::CpuGovernor(v)) => {
                set_cpu_governor(*p as i32, v)?;
                mutate_persisted_state(|s| match p {
                    0 => s.cpu0 = v.clone(),
                    4 => s.cpu4 = v.clone(),
                    7 => s.cpu7 = v.clone(),
                    _ => {}
                })
            }
            (Control::CpuCores, Target::CpuCores(mask)) => {
                set_cpu_manual(true)?;
                apply_saved_cpu_mask(*mask as i32)?;
                mutate_persisted_state(|s| s.cpu_online_mask = *mask as i32)
            }
            (Control::Refresh, Target::Refresh(hz)) => apply_refresh(*hz),
            _ => Err("invalid per-app target".into()),
        }
    }
    /// Whether the hardware still holds the target.
    ///
    /// The platform's own services take these nodes back: the power HAL
    /// publishes its own GPU state, and the stock ROM re-asserts the touch
    /// rate on every screen on. A profile that was applied once and then
    /// taken back is not a profile that is in effect, so the control is
    /// written again when it has drifted.
    fn is_live(&mut self, c: &Control, target: &Target) -> Option<bool> {
        match (c, target) {
            (Control::Gpu, Target::Gpu(v)) => Some(gpu_profile_configured(*v as i32)),
            (Control::Touch, Target::Touch(v)) => Some(touch_profile_is_live(*v as i32)),
            _ => None,
        }
    }
    fn restore(&mut self, c: &Control, saved: &Saved) -> Result<(), String> {
        {
            let _temporary = Temporary::begin();
            match c {
                Control::Touch => set_touch_profile(saved.state.touch.max(0))?,
                Control::Gpu => {
                    apply_performance_profile(saved.state.perf)?;
                    let min = saved.state.gpu_min_freq_mhz;
                    let max = saved.state.gpu_max_freq_mhz;
                    if min > 0 && max >= min {
                        set_gpu_min_freq(338)?;
                        set_gpu_max_freq(max)?;
                        set_gpu_min_freq(min)?;
                    }
                    let governor = if !saved.state.gpu_governor.is_empty() {
                        &saved.state.gpu_governor
                    } else {
                        &saved.state.gpu
                    };
                    if !governor.is_empty() {
                        set_gpu_governor(governor)?;
                    }
                    if !saved.state.gpu_power_policy.is_empty() {
                        set_gpu_power_policy(&saved.state.gpu_power_policy)?;
                    }
                    if saved.state.gpu_uncap == 1 {
                        set_gpu_uncap(true)?;
                    }
                }
                Control::CpuRange(p) => {
                    let desired = persisted_cpu_range(&saved.state, *p as i32).unwrap_or((
                        saved.live[0]
                            .as_i64()
                            .ok_or("missing original CPU minimum")? as i32,
                        saved.live[1]
                            .as_i64()
                            .ok_or("missing original CPU maximum")? as i32,
                    ));
                    apply_cluster_freq_controls(*p as i32, desired.0, desired.1)?;
                }
                Control::CpuGovernor(p) => set_cpu_governor(
                    *p as i32,
                    saved.live.as_str().ok_or("missing original governor")?,
                )?,
                Control::CpuCores => {
                    if saved.state.cpu_manual == 1 {
                        set_cpu_manual(true)?;
                        apply_saved_cpu_mask(saved.state.cpu_online_mask)?;
                    } else {
                        set_cpu_manual(false)?;
                    }
                }
                Control::Refresh => restore_refresh(&saved.live)?,
            }
        }
        // The preferences of the device are restored and saved here, outside
        // the borrowed scope: a profile that only borrows a control must not
        // leave the preferences of the device changed.
        mutate_persisted_state(|state| copy_preference(c, &saved.state, state))?;
        if matches!(c, Control::CpuRange(_)) {
            let _lock = cpu_freq_apply_lock()
                .lock()
                .map_err(|_| "CPU apply lock poisoned")?;
            let ranges_active = persisted_cpu_ranges_active(
                &persisted_state()
                    .lock()
                    .map_err(|_| "state lock poisoned")?
                    .clone(),
            );
            if !ranges_active {
                // A restarted daemon may have captured the temporary thermal
                // mode as its startup baseline. The journal owns the original.
                let original = saved
                    .live
                    .get(2)
                    .and_then(Value::as_i64)
                    .map(|v| v as i32)
                    .unwrap_or(saved.state.cpu_thermal_mode_prev);
                if original >= 0 {
                    mutate_persisted_state(|state| state.cpu_thermal_mode_prev = original)?;
                }
                restore_cpu_thermal_mode_unlocked()?;
            }
        }
        let mut entries = originals().lock().map_err(|_| "originals lock poisoned")?;
        let mut candidate = entries.clone();
        candidate.remove(&key(c));
        journal(&candidate)?;
        *entries = candidate;
        Ok(())
    }
}

fn refresh_modes(raw: &str) -> BTreeSet<u16> {
    raw.split("fps=")
        .skip(1)
        .filter_map(|part| {
            part.split(|c: char| !c.is_ascii_digit() && c != '.')
                .next()?
                .parse::<f64>()
                .ok()
        })
        .filter_map(|hz| {
            [60, 90, 120]
                .into_iter()
                .find(|v| (hz - *v as f64).abs() < 0.1)
        })
        .collect()
}
fn preferred_mode(raw: &str) -> Result<(i32, i32, f64), String> {
    if raw.trim() == "User preferred display mode: null" {
        return Ok((-1, -1, 0.0));
    }
    let values: Vec<_> = raw
        .strip_prefix("User preferred display mode:")
        .ok_or("display preference API unsupported")?
        .split_whitespace()
        .collect();
    if values.len() != 3 {
        return Err("invalid display preference reply".into());
    }
    let width = values[0].parse().map_err(|_| "invalid preferred width")?;
    let height = values[1].parse().map_err(|_| "invalid preferred height")?;
    let hz = values[2]
        .parse::<f64>()
        .map_err(|_| "invalid preferred refresh")?;
    if !hz.is_finite() {
        return Err("invalid preferred refresh".into());
    }
    Ok((width, height, hz))
}
fn physical_mode(raw: &str, hz: u16) -> Result<(i32, i32), String> {
    for block in raw.split('{') {
        let number = |key: &str| -> Option<f64> {
            block
                .split_once(key)?
                .1
                .split(|c: char| !c.is_ascii_digit() && c != '.')
                .next()?
                .parse()
                .ok()
        };
        if number("fps=").is_some_and(|rate| (rate - hz as f64).abs() < 0.1) {
            let w = number("width=").ok_or("display width missing")? as i32;
            let h = number("height=").ok_or("display height missing")? as i32;
            if w > 0 && h > 0 {
                return Ok((w, h));
            }
        }
    }
    Err("supported physical display mode unavailable".into())
}
fn apply_refresh(hz: u16) -> Result<(), String> {
    let user = current_user()?;
    let saved = originals()
        .lock()
        .map_err(|_| "originals lock poisoned")?
        .get("refresh")
        .cloned()
        .ok_or("refresh original missing")?;
    if hz == 0 {
        return restore_refresh(&saved.live);
    }
    let display = cli(
        "/system/bin/cmd",
        &["display", "get-displays", "-t", "internal"],
    )?;
    if !refresh_modes(&display).contains(&hz) {
        return Err(format!("{hz} Hz is not a supported internal display mode"));
    }
    let rate = hz.to_string();
    let (width, height) = physical_mode(&display, hz)?;
    cli(
        "/system/bin/cmd",
        &[
            "display",
            "set-user-preferred-display-mode",
            &width.to_string(),
            &height.to_string(),
            &rate,
            "0",
        ],
    )?;
    let preferred = preferred_mode(&cli(
        "/system/bin/cmd",
        &["display", "get-user-preferred-display-mode", "0"],
    )?)?;
    if preferred.0 != width || preferred.1 != height || (preferred.2 - hz as f64).abs() > 0.1 {
        return Err("preferred display mode readback rejected".into());
    }
    // Bounds are set once per actual profile transition, not by a refresh-rate
    // tug-of-war loop. The ROM still owns thermal/display safety decisions.
    settings(user, "put", "peak_refresh_rate", Some(&rate))?;
    settings(user, "put", "min_refresh_rate", Some(&rate))?;
    // Some ROMs vote from their own refresh settings instead of AOSP bounds.
    // Only touch existing, numeric refresh controls captured in the journal;
    // never manufacture vendor settings on an unrelated ROM.
    if let Some(entries) = saved.live["vendor"].as_array() {
        for entry in entries {
            let table = entry["table"].as_str().ok_or("missing refresh table")?;
            let key = entry["key"].as_str().ok_or("missing refresh key")?;
            let target = if key == "is_smart_fps" { "0" } else { &rate };
            refresh_setting(user, table, "put", key, Some(target))?;
            if refresh_setting(user, table, "get", key, None)? != target {
                return Err(format!("ROM refresh setting rejected: {key}"));
            }
        }
    }
    for key in ["min_refresh_rate", "peak_refresh_rate"] {
        let readback = settings(user, "get", key, None)?
            .parse::<f64>()
            .map_err(|_| "refresh readback unavailable")?;
        if (readback - hz as f64).abs() > 0.1 {
            return Err("refresh preference readback rejected".into());
        }
    }
    Ok(())
}
fn restore_refresh(saved: &Value) -> Result<(), String> {
    let user = saved["user"].as_u64().ok_or("missing refresh user")? as u32;
    if let Some(entries) = saved["vendor"].as_array() {
        for entry in entries {
            let table = entry["table"].as_str().ok_or("missing refresh table")?;
            let key = entry["key"].as_str().ok_or("missing refresh key")?;
            let value = entry["value"]
                .as_str()
                .ok_or("missing original refresh value")?;
            refresh_setting(user, table, "put", key, Some(value))?;
            if refresh_setting(user, table, "get", key, None)? != value {
                return Err(format!("ROM refresh restore rejected: {key}"));
            }
        }
    }
    let preferred = preferred_mode(
        saved["preferred"]
            .as_str()
            .ok_or("missing original preferred mode")?,
    )?;
    if preferred.0 <= 0 || preferred.1 <= 0 || preferred.2 <= 0.0 {
        cli(
            "/system/bin/cmd",
            &["display", "clear-user-preferred-display-mode", "0"],
        )?;
    } else {
        cli(
            "/system/bin/cmd",
            &[
                "display",
                "set-user-preferred-display-mode",
                &preferred.0.to_string(),
                &preferred.1.to_string(),
                &preferred.2.to_string(),
                "0",
            ],
        )?;
    }
    let actual = preferred_mode(&cli(
        "/system/bin/cmd",
        &["display", "get-user-preferred-display-mode", "0"],
    )?)?;
    if preferred.0 != actual.0 || preferred.1 != actual.1 || (preferred.2 - actual.2).abs() > 0.1 {
        return Err("preferred mode restore readback rejected".into());
    }
    for (key, field) in [("min_refresh_rate", "min"), ("peak_refresh_rate", "max")] {
        let original = saved[field]
            .as_str()
            .ok_or("missing original refresh setting")?;
        if original == "null" {
            settings(user, "delete", key, None)?;
        } else {
            settings(user, "put", key, Some(original))?;
        }
        if settings(user, "get", key, None)? != original {
            return Err("refresh restore readback rejected".into());
        }
    }
    Ok(())
}
fn refresh_setting(
    user: u32,
    table: &str,
    action: &str,
    key: &str,
    value: Option<&str>,
) -> Result<String, String> {
    if !matches!(table, "system" | "secure")
        || !matches!(key, "user_refresh_rate" | "miui_refresh_rate" | "is_smart_fps")
    {
        return Err("invalid ROM refresh setting".into());
    }
    let user = user.to_string();
    let mut args = vec!["--user", &user, action, table, key];
    if let Some(value) = value {
        args.push(value);
    }
    cli("/system/bin/settings", &args)
}
fn vendor_refresh_value(raw: &str) -> bool {
    raw.parse::<f64>()
        .is_ok_and(|v| v.is_finite() && (30.0..=240.0).contains(&v))
}
fn capture_vendor_refresh(user: u32) -> Result<Value, String> {
    let mut entries = Vec::new();
    for table in ["system", "secure"] {
        for key in ["user_refresh_rate", "miui_refresh_rate"] {
            let value = refresh_setting(user, table, "get", key, None)?;
            if vendor_refresh_value(&value) {
                entries.push(json!({"table":table,"key":key,"value":value}));
            }
        }
    }
    if entries.iter().any(|entry| entry["key"] == "miui_refresh_rate") {
        let value = refresh_setting(user, "system", "get", "is_smart_fps", None)?;
        if matches!(value.as_str(), "0" | "1") {
            entries.push(json!({"table":"system","key":"is_smart_fps","value":value}));
        }
    }
    Ok(json!(entries))
}
fn capabilities() -> Result<Capabilities, String> {
    let mut caps = Capabilities::default();
    if touch_panel_code() != 0 || vendor_binder::touch_available() {
        caps.touch = BTreeSet::from([0, 1, 2, 3]);
    }
    if gpu_get_min_freq_mhz() > 0 && gpu_get_max_freq_mhz() > 0 {
        caps.gpu = BTreeSet::from([0, 1, 2, 3]);
    }
    if let Ok(display) = cli(
        "/system/bin/cmd",
        &["display", "get-displays", "-t", "internal"],
    ) {
        caps.refresh = refresh_modes(&display);
        if !caps.refresh.is_empty() {
            caps.refresh.insert(0);
        }
    }
    for p in [0, 4, 7] {
        let table = cpu_available_frequencies(p as i32)
            .into_iter()
            .map(|v| v as u32)
            .collect();
        caps.cpu_frequencies.insert(p, table);
        let governors = read_trimmed(format!(
            "/sys/devices/system/cpu/cpufreq/policy{p}/scaling_available_governors"
        ))
        .unwrap_or_default();
        caps.cpu_governors.insert(
            p,
            governors
                .split_whitespace()
                .filter(|g| {
                    [
                        "sugov_ext",
                        "conservative",
                        "powersave",
                        "performance",
                        "schedutil",
                    ]
                    .contains(g)
                })
                .map(str::to_owned)
                .collect(),
        );
    }
    caps.manual_cores = Path::new("/sys/devices/system/cpu/cpu1/online").exists();
    Ok(caps)
}
fn capabilities_json() -> Result<Value, String> {
    let caps = capabilities()?;
    let mut cpu = json!({});
    for p in [0, 4, 7] {
        cpu[p.to_string()] = json!({"frequencies":caps.cpu_frequencies.get(&p),"governors":caps.cpu_governors.get(&p)});
    }
    Ok(
        json!({"touch":caps.touch,"gpu":caps.gpu,"refresh":caps.refresh,"cpu":cpu,"cores":caps.manual_cores}),
    )
}
fn foreground() -> Result<Option<App>, String> {
    // First try the AccessibilityService-based tracker (more efficient)
    if let Some(info) = crate::foreground_app::ForegroundAppTracker::global().current_app() {
        if info.is_valid() {
            // Try to get user ID from dumpsys (lightweight check)
            if let Ok(raw) = cli("/system/bin/dumpsys", &["activity", "activities"]) {
                if let Ok(Some(app)) = parse_foreground(&raw) {
                    return Ok(Some(app));
                }
            }
            // Fallback: create App from package name (user 0)
            return App::new(0, &info.package_name).map(Some);
        }
    }
    
    // Fallback to dumpsys parsing
    let raw = cli("/system/bin/dumpsys", &["activity", "activities"])?;
    parse_foreground(&raw)
}
fn parse_foreground(raw: &str) -> Result<Option<App>, String> {
    if raw
        .lines()
        .any(|line| line.split_whitespace().any(|token| matches!(token, "mSleeping=true" | "mShuttingDown=true")))
    {
        return Ok(None);
    }
    let top: Vec<_> = raw
        .lines()
        .filter(|line| line.contains("topResumedActivity="))
        .collect();
    // Floating/split-screen tasks can each be resumed. Prefer the supervisor's
    // focused summary, never the first background/freeform task in Z order.
    let focused = raw
        .lines()
        .find(|line| line.trim_start().starts_with("mTopResumedActivity="))
        .or_else(|| {
            raw.lines()
                .find(|line| line.trim_start().starts_with("ResumedActivity:"))
        })
        .or_else(|| {
            raw.lines()
                .find(|line| line.trim_start().starts_with("mResumedActivity:"))
        })
        .or_else(|| if top.len() == 1 { Some(top[0]) } else { None });
    let Some(line) = focused else {
        if raw.contains("topResumedActivity=null") {
            return Ok(None);
        }
        return Err("top resumed activity unavailable on this ROM".into());
    };
    if line.contains("=null") || line.trim_end().ends_with(": null") {
        return Ok(None);
    }
    let parts: Vec<_> = line.split_whitespace().collect();
    let user = parts
        .iter()
        .find_map(|part| part.strip_prefix('u')?.parse::<u32>().ok())
        .ok_or("foreground user missing")?;
    let component = parts
        .iter()
        .find(|p| p.contains('/'))
        .ok_or("foreground package missing")?;
    App::new(
        user,
        component
            .split_once('/')
            .ok_or("foreground component malformed")?
            .0,
    )
    .map(Some)
}
/// The controls the per-app profile is holding right now.
///
/// The global guard restores the global profile, so it has to know when a
/// per-app profile is holding a control, or the two writers would fight
/// over it and the per-app profile would never be seen to work. The mask
/// is published in an atomic rather than read under the runtime lock,
/// because the guard already holds the GPU lock and taking the runtime lock
/// there would deadlock against the per-app apply.
static PER_APP_OWNED: AtomicU32 = AtomicU32::new(0);

/// Whether a per-app profile is holding the GPU control.
pub(super) fn owns_gpu() -> bool {
    PER_APP_OWNED.load(Ordering::Acquire) & (1 << 0) != 0
}

/// Whether a per-app profile is holding the touch control.
pub(super) fn owns_touch() -> bool {
    PER_APP_OWNED.load(Ordering::Acquire) & (1 << 1) != 0
}

/// Whether a per-app profile is holding a CPU cluster's governor.
pub(super) fn owns_cpu_governor() -> bool {
    PER_APP_OWNED.load(Ordering::Acquire) & (1 << 5) != 0
}

/// Whether a per-app profile is holding a CPU cluster's frequency range.
pub(super) fn owns_cpu_range() -> bool {
    PER_APP_OWNED.load(Ordering::Acquire) & (1 << 4) != 0
}

/// Whether a per-app profile is holding the CPU core mask.
pub(super) fn owns_cpu_cores() -> bool {
    PER_APP_OWNED.load(Ordering::Acquire) & (1 << 3) != 0
}

/// Publish what the per-app profile is holding.
fn publish_owned(mask: u32) {
    PER_APP_OWNED.store(mask, Ordering::Release);
}

fn reconcile(r: &mut Runtime, hint: Wake) -> Result<(), String> {
    // The foreground is only read while the service and the profiles are
    // both on. There is nothing to read for a profile that is not watched.
    let watching = service_control::enabled() && r.config.enabled;
    reconcile_with(r, hint, watching, foreground)
}

/// The reconciliation, with the reader of the foreground as an argument.
///
/// The reader is an argument so that the answer of the framework can be
/// replaced in a test: a framework that does not answer must not take the
/// profile of the app away.
fn reconcile_with<F>(r: &mut Runtime, hint: Wake, watching: bool, read: F) -> Result<(), String>
where
    F: FnOnce() -> Result<Option<App>, String>,
{
    if r.config_error.is_some() {
        return Err("per-app configuration is damaged; reset profiles to recover".into());
    }
    if watching {
        // A resume event already names the app that took the foreground, so
        // the framework is only asked when the event could not say. Reading
        // the framework costs hundreds of milliseconds, and a per-app profile
        // that lands half a second late is a per-app profile that appears
        // broken.
        r.foreground = match hint {
            Wake::Resumed(app) => {
                r.source = "resume-event";
                Some(app)
            }
            Wake::Changed => match read() {
                Ok(app) => {
                    r.source = "framework-summary";
                    app
                }
                Err(error) => {
                    // The framework did not answer. The app that is in the
                    // foreground is not known, but the profile that is in
                    // effect still belongs to the last app that was known to
                    // be in the foreground. Giving it back here is what
                    // makes a per-app profile look like it was never
                    // applied: the profile is released and taken again
                    // within a second, and the user sees the device's own
                    // profile instead of the app's.
                    return Err(error);
                }
            },
        };
    } else {
        r.foreground = None;
    }
    let profile = r
        .foreground
        .as_ref()
        .and_then(|app| r.config.profiles.get(app));
    let result = r.owner.transition(
        &mut Adapter::new(),
        r.foreground.as_ref(),
        profile,
        watching,
        watching,
    );
    publish_owned(r.owner.applied_mask());
    result
}
fn status(r: &Runtime) -> Value {
    let (state, owner) = match &r.owner.status {
        Status::Global => ("global", None),
        Status::Active(app) => (
            "active",
            Some(json!({"user":app.user,"package":app.package})),
        ),
        Status::RecoveryRequired(_) => ("recovery", None),
    };
    let controls: Vec<_> = originals()
        .lock()
        .map(|s| s.keys().cloned().collect())
        .unwrap_or_default();
    let foreground = r
        .foreground
        .as_ref()
        .map(|app| json!({"user":app.user,"package":app.package}));
    // The controls that were asked for and that the hardware refused. They
    // are reported one by one, because a control the hardware cannot take
    // must not hide the controls that did land.
    let refused: Vec<_> = r
        .owner
        .refused()
        .into_iter()
        .map(|(control, error)| json!({"control":control,"error":error}))
        .collect();
    json!({"config":r.config.json(),"status":state,"owner":owner,"foreground":foreground,"controls":controls,"refused":refused,"error":r.config_error.as_ref().or(r.error.as_ref()),"watching":r.watching,"serviceEnabled":service_control::enabled()})
}
/// A per-app health summary for the diagnostics view.
///
/// The ownership map is the part that matters: it is what tells the global
/// controls whether a foreground app currently owns them, and it is what a
/// user needs to see when a global control refuses to apply.
/// Whether the hardware still holds a target.
///
/// The answer is about the hardware, so it is asked of the adapter rather
/// than of the profile, and it is what tells a profile that was applied
/// apart from a profile that is in effect.
fn target_live(control: &super::app_controls::Control, target: &super::app_controls::Target) -> Option<bool> {
    let mut adapter = Adapter::new();
    adapter.is_live(control, target)
}

pub(super) fn health_json() -> Value {
    let runtime = runtime().lock().ok();
    let foreground = runtime.as_ref().and_then(|r| {
        r.foreground
            .as_ref()
            .map(|app| json!({"user":app.user,"package":app.package}))
    });
    let owned = originals()
        .lock()
        .map(|entries| entries.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let configured = runtime
        .as_ref()
        .map(|r| r.config.profiles.len())
        .unwrap_or(0);
    let error = runtime
        .as_ref()
        .and_then(|r| r.config_error.clone().or_else(|| r.error.clone()));
    let watching = runtime.as_ref().is_some_and(|r| r.watching);
    let enabled = runtime.as_ref().is_some_and(|r| r.config.enabled);
    // The controls the foreground app is holding, and whether the hardware
    // still has them. A profile that is listed but that the platform has
    // taken back is a profile that is not in effect, and the difference
    // between the two is what a user cannot see.
    let held: Vec<Value> = runtime
        .as_ref()
        .map(|r| {
            r.owner
                .held()
                .iter()
                .map(|(control, target)| {
                    json!({
                        "control": format!("{control:?}"),
                        "target": format!("{target:?}"),
                        "live": crate::app_runtime::target_live(control, target),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let owner = runtime.as_ref().and_then(|r| match &r.owner.status {
        super::app_controls::Status::Active(app) => {
            Some(json!({"user": app.user, "package": app.package}))
        }
        _ => None,
    });

    json!({
        "owner": owner,
        "held": held,
        "foreground": foreground,
        "source": runtime.as_ref().map(|r| r.source).unwrap_or("none"),
        "configuredProfiles": configured,
        "ownedControls": owned,
        "watching": watching,
        "enabled": enabled,
        "serviceEnabled": service_control::enabled(),
        "error": error,
    })
}

pub(super) fn is_command(cmd: &str) -> bool {
    cmd == "GET app.controls"
        || cmd == "GET app.capabilities"
        || cmd == "GET app.list"
        || cmd.starts_with("SET app.profile ")
        || cmd.starts_with("SET app.enabled ")
        || cmd.starts_with("ACTION app.reset ")
        || cmd == "ACTION app.reset_all"
}
pub(super) fn command(cmd: &str) -> Result<Value, String> {
    if cmd == "GET app.capabilities" {
        return capabilities_json();
    }
    if cmd == "GET app.list" {
        let user = current_user()?;
        let user_text = user.to_string();
        let packages = cli(
            "/system/bin/cmd",
            &["package", "list", "packages", "--user", &user_text],
        )?;
        let system = cli(
            "/system/bin/cmd",
            &["package", "list", "packages", "-s", "--user", &user_text],
        )?;
        let launchers = cli(
            "/system/bin/cmd",
            &[
                "package",
                "query-activities",
                "--brief",
                "--user",
                &user_text,
                "-a",
                "android.intent.action.MAIN",
                "-c",
                "android.intent.category.LAUNCHER",
            ],
        )
        .ok();
        let launcher_packages: BTreeSet<_> = launchers
            .as_deref()
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().split_once('/').map(|v| v.0))
            .filter(|name| App::new(user, name).is_ok())
            .collect();
        let apps:Vec<_>=packages.lines().filter_map(|line|line.strip_prefix("package:")).filter(|name|App::new(user,name).is_ok())
            .map(|package|json!({"user":user,"package":package,"system":system.lines().any(|s|s.strip_prefix("package:")==Some(package)),
                "launchable":launchers.as_ref().map(|_|launcher_packages.contains(package))})).collect();
        return Ok(json!(apps));
    }
    let mut r = runtime()
        .lock()
        .map_err(|_| "per-app runtime lock poisoned")?;
    if cmd == "GET app.controls" {
        return Ok(status(&r));
    }
    if !RECOVERY_READY.load(Ordering::Acquire) {
        return Err("Per-App Controls is waiting for recovery; no new override was applied".into());
    }
    let mut config = r.config.clone();
    if let Some(raw) = cmd.strip_prefix("SET app.profile ") {
        if raw.len() > 3000 {
            return Err("app profile too large".into());
        }
        let v: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        let app = App::new(
            v["user"]
                .as_u64()
                .filter(|v| *v <= 100_000)
                .ok_or("invalid user")? as u32,
            v["package"].as_str().ok_or("missing package")?,
        )?;
        let mut profile = Profile::parse(&v["profile"])?;
        // Merge with existing profile to preserve controls not explicitly overridden
        if let Some(existing) = config.profiles.get(&app) {
            let mut merged = existing.clone();
            merged.merge(profile);
            profile = merged;
        }
        capabilities()?.validate(&profile)?;
        config.profiles.insert(app, profile);
    } else if let Some(arg) = cmd.strip_prefix("SET app.enabled ") {
        config.enabled = match arg {
            "1" => true,
            "0" => false,
            _ => return Err("invalid profile switch".into()),
        };
    } else if let Some(arg) = cmd.strip_prefix("ACTION app.reset ") {
        let (user, package) = arg.split_once(' ').ok_or("missing app identity")?;
        config.profiles.remove(&App::new(
            user.parse().map_err(|_| "invalid user")?,
            package,
        )?);
    } else if cmd == "ACTION app.reset_all" {
        config = Config::default();
    } else {
        return Err("unknown app command".into());
    }
    config.save(&state_dir().join("app-profiles.json"))?;
    r.config = config;
    r.config_error = None;
    if r.config.enabled && !r.config.profiles.is_empty() && service_control::enabled() {
        service_control::set_intent(true, true)?;
    }
    r.error = reconcile(&mut r, Wake::Changed).err();
    wake(Wake::Changed);
    Ok(status(&r))
}
pub(super) fn check_global_command(cmd: &str) -> Result<(), String> {
    let entries = originals()
        .lock()
        .map_err(|_| "per-app ownership lock poisoned")?;
    let owned = if cmd.starts_with("SET touch ") {
        entries.contains_key("touch")
    } else if cmd.starts_with("SET perf ")
        || cmd.starts_with("SET gpu.")
        || cmd.starts_with("ACTION gpu.")
    {
        entries.contains_key("gpu")
    } else if cmd.starts_with("SET cpu.manual ") || cmd.starts_with("SET cpu.core ") {
        entries.contains_key("cores")
    } else if cmd.starts_with("SET cpu.") || cmd.starts_with("ACTION cpu.") {
        let parts: Vec<_> = cmd.split_whitespace().collect();
        let prefix = if cmd.starts_with("SET cpu.gov ") {
            "governor"
        } else {
            "range"
        };
        parts
            .get(2)
            .is_some_and(|p| entries.contains_key(&format!("{prefix}{p}")))
    } else {
        false
    };
    if owned {
        Err("This control is owned by the foreground app's Per-App Controls profile; edit that profile or leave the app".into())
    } else {
        Ok(())
    }
}
pub(super) fn release() -> Result<(), String> {
    if !RECOVERY_READY.load(Ordering::Acquire) {
        recover()?;
        RECOVERY_READY.store(true, Ordering::Release);
    }
    let mut r = runtime()
        .lock()
        .map_err(|_| "per-app runtime lock poisoned")?;
    r.owner
        .transition(&mut Adapter::new(), None, None, false, false)?;
    publish_owned(r.owner.applied_mask());
    r.foreground = None;
    r.error = None;
    Ok(())
}
pub(super) fn reset_all() -> Result<(), String> {
    release()?;
    let mut r = runtime()
        .lock()
        .map_err(|_| "per-app runtime lock poisoned")?;
    let config = Config::default();
    config.save(&state_dir().join("app-profiles.json"))?;
    r.config = config;
    r.config_error = None;
    Ok(())
}
/// The hint that carries the most information.
///
/// A resume event names the app that took the foreground, so it is preferred
/// over a bare change that only says something moved.
fn stronger_hint(current: Wake, next: Wake) -> Wake {
    match (current, next) {
        (Wake::Resumed(_), Wake::Resumed(other)) => Wake::Resumed(other),
        (Wake::Resumed(app), Wake::Changed) => Wake::Resumed(app),
        (Wake::Changed, Wake::Resumed(app)) => Wake::Resumed(app),
        (Wake::Changed, Wake::Changed) => Wake::Changed,
    }
}

pub(super) fn wake(hint: Wake) {
    if let Some(sender) = WAKE.get() {
        // The channel holds one pending hint. A full channel means a change
        // is already waiting to be handled, and the pending one is replaced
        // by the stronger hint.
        let _ = sender.try_send(hint);
    }
}
fn recover() -> Result<(), String> {
    let path = state_dir().join("app-recovery.json");
    let raw = match fs::read(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if raw.len() > 512 * 1024 {
        return Err("per-app recovery journal too large".into());
    }
    let saved: BTreeMap<String, Saved> =
        serde_json::from_slice(&raw).map_err(|e| format!("per-app recovery journal: {e}"))?;
    *originals().lock().map_err(|_| "originals lock poisoned")? = saved.clone();
    for (name, token) in saved {
        Adapter::new().restore(&control(&name).ok_or("unknown recovery control")?, &token)?;
    }
    Ok(())
}
pub(super) fn start() {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let (sender, receiver) = mpsc::sync_channel(1);
    let _ = WAKE.set(sender);
    std::thread::spawn(move || {
        // The global profile is restored first, and the per-app profile is
        // applied after it. The two write the same controls, so a per-app
        // profile that is applied first and then overwritten by the global
        // one is a per-app profile that never takes effect.
        let deadline = Instant::now() + Duration::from_secs(20);
        while !super::global_restore_done() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        // Recover prior temporary overrides before allowing foreground writes.
        let recovered = {
            let _gate = service_control::GATE.write().expect("service gate");
            recover()
        };
        if let Err(error) = recovered {
            if let Ok(mut r) = runtime().lock() {
                r.error = Some(error);
            }
            return;
        }
        RECOVERY_READY.store(true, Ordering::Release);
        #[cfg(target_os = "android")]
        std::thread::spawn(event_reader);
        wake(Wake::Changed);
        while let Ok(first) = receiver.recv() {
            // Keep the strongest hint while events arrive. A resume event
            // names the app outright, so it is preferred over a bare change.
            let mut hint = first;
            let settle = Instant::now();
            while settle.elapsed() < Duration::from_millis(120) {
                match receiver.recv_timeout(Duration::from_millis(40)) {
                    Ok(next) => hint = stronger_hint(hint, next),
                    Err(_) => break,
                }
            }

            // A named app is applied straight away. Only a change the event
            // could not describe needs the framework's own summary, and that
            // is read with a bounded settling window rather than a poll.
            if let Wake::Resumed(app) = hint {
                let _gate = service_control::GATE.write().expect("service gate");
                if let Ok(mut r) = runtime().lock() {
                    r.error = reconcile(&mut r, Wake::Resumed(app)).err();
                    if !r.config.enabled || !service_control::enabled() {
                        break;
                    }
                }
                continue;
            }

            for delay in [0, 150, 350] {
                if delay > 0
                    && receiver.recv_timeout(Duration::from_millis(delay)).is_ok() {
                        // A newer event supersedes the pending settling check.
                        while receiver.try_recv().is_ok() {}
                    }
                let _gate = service_control::GATE.write().expect("service gate");
                if let Ok(mut r) = runtime().lock() {
                    r.error = reconcile(&mut r, Wake::Changed).err();
                    if !r.config.enabled || !service_control::enabled() {
                        break;
                    }
                }
            }
        }
    });
}
// The reader is compiled on every target so that a mistake in it is caught
// by the ordinary build rather than only by the Android cross build. Only the
// reader itself is Android specific: it watches the framework's activity
// events, and the host has no such log.
//
// The function is only called on Android, so the ordinary build reports it as
// unused. That is expected.
#[allow(dead_code)]
fn event_reader() {
    loop {
        let child = ProcessCommand::new("/system/bin/logcat")
            .args([
                "-b",
                "events",
                "-v",
                "tag",
                "-T",
                "1",
                "wm_set_resumed_activity:I",
                "wm_resume_activity:I",
                "wm_on_top_resumed_gained_called:I",
                "wm_focused_root_task:I",
                "wm_pause_activity:I",
                "wm_stop_activity:I",
                "wm_on_top_resumed_lost_called:I",
                "*:S",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(mut child) => {
                if let Ok(mut r) = runtime().lock() {
                    r.watching = true;
                }
                wake(Wake::Changed);
                if let Some(output) = child.stdout.take() {
                    for line in BufReader::new(output).lines().map_while(Result::ok) {
                        // A resume event names the app that took the
                        // foreground. Carrying it through avoids a round
                        // trip to the framework for the common case.
                        if let Some(app) = super::app_controls::resumed_app(&line) {
                            wake(Wake::Resumed(app));
                            continue;
                        }
                        if line.contains("wm_on_top_resumed_gained_called")
                            || line.contains("wm_focused_root_task")
                            || line.contains("wm_pause_activity")
                            || line.contains("wm_stop_activity")
                            || line.contains("wm_on_top_resumed_lost_called")
                        {
                            wake(Wake::Changed);
                        }
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
            }
            Err(e) => {
                if let Ok(mut r) = runtime().lock() {
                    r.error = Some(format!("foreground events unavailable: {e}"));
                }
            }
        }
        if let Ok(mut r) = runtime().lock() {
            r.watching = false;
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn vendor_refresh_only_accepts_existing_numeric_rates() {
        for value in ["60", "90", "120.0", "240"] {
            assert!(vendor_refresh_value(value));
        }
        for value in ["null", "NaN", "inf", "0", "1", "-1", "999", "auto"] {
            assert!(!vendor_refresh_value(value));
        }
    }
    use super::*;
    use super::app_controls::control_bit;
    #[test]
    fn internal_modes_are_detected_not_assumed() {
        assert_eq!(
            refresh_modes("{fps=60.0}{fps=120.00001}{fps=30.0}{fps=90.0}"),
            BTreeSet::from([60, 90, 120])
        );
        assert!(refresh_modes("display unavailable").is_empty());
    }
    #[test]
    fn a_named_resume_event_is_preferred_over_a_bare_change() {
        let app = App::new(0, "com.example.game").expect("app");
        assert!(matches!(
            stronger_hint(Wake::Changed, Wake::Resumed(app.clone())),
            Wake::Resumed(_)
        ));
        assert!(matches!(
            stronger_hint(Wake::Resumed(app.clone()), Wake::Changed),
            Wake::Resumed(_)
        ));
        assert!(matches!(
            stronger_hint(Wake::Changed, Wake::Changed),
            Wake::Changed
        ));
        // The most recent named event wins over an earlier one.
        let other = App::new(0, "com.example.other").expect("app");
        assert!(matches!(
            stronger_hint(Wake::Resumed(app), Wake::Resumed(other.clone())),
            Wake::Resumed(ref current) if current.package == "com.example.other"
        ));
    }

    #[test]
    fn foreground_requires_user_and_full_package() {
        assert_eq!(
            parse_foreground(
                "topResumedActivity=ActivityRecord{abcd u10 com.example.game/.Main t2}"
            )
            .unwrap(),
            Some(App::new(10, "com.example.game").unwrap())
        );
        assert!(parse_foreground("mResumedActivity=classOnly").is_err());
        assert_eq!(parse_foreground("topResumedActivity=null").unwrap(), None);
    }
    #[test]
    fn legacy_resumed_summary_and_sleep_are_handled() {
        let raw = "mResumedActivity: ActivityRecord{abcd u0 com.example.game/.Main t2}";
        assert_eq!(
            parse_foreground(raw).unwrap(),
            Some(App::new(0, "com.example.game").unwrap())
        );
        assert_eq!(
            parse_foreground(&format!("mSleeping=true\n{raw}")).unwrap(),
            None
        );
    }
    #[test]
    fn focused_activity_wins_over_a_resumed_floating_window() {
        let raw = "topResumedActivity=ActivityRecord{x u0 io.github.neeschal.dashessential/android.app.NativeActivity t1}\n\
                 topResumedActivity=ActivityRecord{y u0 com.android.settings/.MiuiSettings t2}\n\
                 ResumedActivity: ActivityRecord{y u0 com.android.settings/.MiuiSettings t2}";
        assert_eq!(
            parse_foreground(raw).unwrap(),
            Some(App::new(0, "com.android.settings").unwrap())
        );
        assert!(parse_foreground(raw.split("ResumedActivity:").next().unwrap()).is_err());
    }
    #[test]
    fn gpu_baseline_copy_does_not_touch_cpu_or_touch() {
        let global = PersistedState { cpu_min_freq0: 1000, touch: 2, ..PersistedState::default() };
        let mut effective = global.clone();
        effective.perf = 3;
        effective.cpu_min_freq0 = 2100;
        effective.touch = 3;
        copy_preference(&Control::Gpu, &global, &mut effective);
        assert_eq!(effective.perf, global.perf);
        assert_eq!(effective.cpu_min_freq0, 2100);
        assert_eq!(effective.touch, 3);
    }
    #[test]
    fn cpu_range_baseline_does_not_change_governor() {
        let global = PersistedState { cpu0: "schedutil".into(), ..PersistedState::default() };
        let mut effective = global.clone();
        effective.cpu0 = "performance".into();
        copy_preference(&Control::CpuRange(0), &global, &mut effective);
        assert_eq!(effective.cpu0, "performance");
    }
    #[test]
    fn transient_scope_unwinds_and_is_thread_local() {
        assert!(!transient_write());
        {
            let _scope = Temporary::begin();
            assert!(transient_write());
            assert!(!std::thread::spawn(transient_write).join().unwrap());
        }
        assert!(!transient_write());
    }
    #[test]
    fn preferred_mode_parses_only_the_framework_reply() {
        assert_eq!(
            preferred_mode("User preferred display mode: null").unwrap(),
            (-1, -1, 0.0)
        );
        assert_eq!(
            preferred_mode("User preferred display mode: -1 -1 0.0").unwrap(),
            (-1, -1, 0.0)
        );
        assert!(preferred_mode("Command not supported").is_err());
        assert!(preferred_mode("User preferred display mode: 1280 2772 NaN").is_err());
    }
    #[test]
    fn physical_mode_matches_a_supported_rate_and_not_canvas_size() {
        assert_eq!(physical_mode("supportedModes [{id=1, width=1280, height=2772, fps=60.0}, {id=3, width=1280, height=2772, fps=120.00001}]",120).unwrap(),(1280,2772));
        assert!(physical_mode("{width=1280, height=2772, fps=60.0}", 90).is_err());
    }

    /// A hardware that accepts every control, so that the ownership map can
    /// be driven without the platform.
    #[derive(Default)]
    struct MockHardware {
        values: BTreeMap<Control, Target>,
    }
    impl Hardware for MockHardware {
        type Saved = Saved;
        fn capture(&mut self, _control: &Control) -> Result<Saved, String> {
            Ok(Saved {
                state: PersistedState::default(),
                live: Value::Null,
            })
        }
        fn apply(&mut self, control: &Control, target: &Target) -> Result<(), String> {
            self.values.insert(control.clone(), target.clone());
            Ok(())
        }
        fn restore(&mut self, control: &Control, _saved: &Saved) -> Result<(), String> {
            self.values.remove(control);
            Ok(())
        }
    }

    #[test]
    fn a_borrowed_write_does_not_change_the_state_of_the_device() {
        let before = persisted_state()
            .lock()
            .expect("state lock")
            .clone();
        {
            // A per-app profile borrows the control: the preferences of the
            // device stay what they are.
            let _scope = Temporary::begin();
            mutate_persisted_state(|state| {
                state.perf = 2;
                state.gpu = "powersave".to_string();
                state.gpu_governor = "powersave".to_string();
            })
            .expect("mutate");
        }
        let after = persisted_state().lock().expect("state lock").clone();
        assert_eq!(before.perf, after.perf);
        assert_eq!(before.gpu, after.gpu);
        assert_eq!(before.gpu_governor, after.gpu_governor);
    }

    #[test]
    fn a_foreground_that_does_not_answer_keeps_the_profile_of_the_app() {
        let app = App::new(0, "com.example.kept").expect("app");
        let profile = Profile::parse(&json!({"enabled":true,"touch":2})).expect("profile");
        let mut hardware = MockHardware::default();
        let mut owner = Ownership::<Saved>::default();
        owner
            .transition(&mut hardware, Some(&app), Some(&profile), true, true)
            .expect("apply");
        assert!(!owner.held().is_empty());
        let mut config = Config { enabled: true, ..Config::default() };
        config.profiles.insert(app.clone(), profile);
        let mut runtime = Runtime {
            config,
            config_error: None,
            owner,
            foreground: Some(app.clone()),
            error: None,
            watching: true,
            source: "test",
        };
        // The framework does not answer.
        let result = reconcile_with(&mut runtime, Wake::Changed, true, || {
            Err("framework silent".to_string())
        });
        assert!(result.is_err());
        // The profile of the app is still in effect: it was not given back
        // because the platform was slow.
        assert!(!runtime.owner.held().is_empty());
        assert!(matches!(runtime.owner.status, Status::Active(_)));
    }

    #[test]
    fn a_per_app_profile_keeps_the_gpu_from_the_global_writers() {
        assert!(global_may_write_gpu());
        publish_owned(control_bit(&Control::Gpu));
        assert!(!global_may_write_gpu());
        assert!(owns_gpu());
        publish_owned(0);
        assert!(global_may_write_gpu());
        assert!(!owns_gpu());
    }
}
