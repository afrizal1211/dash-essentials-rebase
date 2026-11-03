//! FPS session recording over the same snapshot the overlay reads.
//!
//! While recording is armed, one sample is taken every [`SAMPLE_INTERVAL`]
//! from `foreground_fps` together with the hardware counters the detail screen
//! graphs. Stopping folds the samples into one session and appends it to a
//! compact JSON-lines file beside the daemon's other state.
//!
//! The recorder never invents data: samples the platform could not measure
//! are skipped, and a session with fewer than two valid samples is discarded
//! instead of being stored as a row of zeroes.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::telemetry::{self, Telemetry};

/// Two samples a second: fast enough to catch a stutter, slow enough to keep
/// an hour of play small.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);
const SAMPLE_INTERVAL_MS: u32 = 500;

/// An hour of samples. Older ones roll out of the ring; the session keeps
/// recording so a long play session is never truncated silently.
const MAX_SAMPLES: usize = 7_200;

/// Points kept in each stored graph. Each bucket keeps its maximum, so a
/// one-off hitch survives downsampling instead of being averaged away.
const SERIES_POINTS: usize = 180;

/// Sessions kept on disk. Beyond this the oldest rows are dropped.
const MAX_SESSIONS: usize = 200;

/// Prism-style description length. It lives in one JSON line, so a long note
/// also makes the history file harder to trim.
const MAX_NOTE_CHARS: usize = 200;

const FILE_NAME: &str = "fps_sessions.jsonl";
const AUTO_FILE_NAME: &str = "fps_auto";

/// Apps that are never a game session. The tracker reports the launcher and
/// system dialogs too; recording those would fill the history with noise.
const NOT_A_GAME: &[&str] = &[
    "com.android.systemui",
    "com.android.settings",
    "com.android.permissioncontroller",
    "com.google.android.permissioncontroller",
    "com.android.packageinstaller",
    "com.google.android.packageinstaller",
    "com.android.providers",
    "com.google.android.gms",
    "com.google.android.googlequicksearchbox",
    "com.android.quicksearchbox",
    "com.android.launcher",
    "com.google.android.apps.nexuslauncher",
    "com.sec.android.app.launcher",
    "com.oneplus.launcher",
    "com.oppo.launcher",
    "com.vivo.launcher",
    "com.bbk.launcher2",
    "com.huawei.android.launcher",
    "com.teslacoilsw.launcher",
    "com.microsoft.launcher",
    "com.miui.home",
    "com.miui.securitycenter",
    "com.transsion.xoslauncher",
];

const PACKAGE_NAME: &str = "io.github.neeschal.dashessential";

/// One measurement before it is folded into a session.
#[derive(Clone, Debug)]
struct Sample {
    fps_x10: i32,
    /// Epoch milliseconds when the sample was taken.
    at_ms: u64,
    package: Option<String>,
    hardware: Telemetry,
}

/// A finished session as it is stored and sent to the UI. Every rate is in
/// tenths of a frame per second so the JSON stays integral and compact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Session {
    /// Epoch milliseconds of the first sample; also the session identity.
    pub id: u64,
    /// Package that contributed the most samples, empty when unknown.
    pub package: String,
    pub started_at: u64,
    pub duration_ms: u64,
    pub samples: u32,
    pub avg_x10: i32,
    pub min_x10: i32,
    pub max_x10: i32,
    pub low1_x10: i32,
    /// Share of samples below 90% of the median, in tenths of a percent.
    pub slow_pct_x10: u32,
    /// Prism's 5% low: the fifth percentile of frame rate.
    #[serde(default)]
    pub low5_x10: i32,
    /// Variance of the frame rate in fps², times 100.
    #[serde(default)]
    pub variance_x100: u32,
    /// Share of samples within ten percent of the average, tenths of a percent.
    #[serde(default)]
    pub smooth_pct_x10: u32,
    /// Standard deviation of frame time over its mean, tenths of a percent.
    #[serde(default)]
    pub jitter_pct_x10: u32,
    /// 99th percentile frame time, in microseconds.
    #[serde(default)]
    pub p99_frame_us: u32,
    /// Samples slower than one and a half target frame times.
    #[serde(default)]
    pub jank: u32,
    /// User note. Empty when the session was never described.
    #[serde(default)]
    pub description: String,
    /// True when the foreground-following recorder started this session.
    #[serde(default)]
    pub auto: bool,
    /// Display resolution captured when the session started.
    #[serde(default)]
    pub resolution: String,
    #[serde(default)]
    pub cpu_avg: i32,
    #[serde(default)]
    pub cpu_max: i32,
    #[serde(default)]
    pub gpu_load_avg: i32,
    #[serde(default)]
    pub gpu_load_max: i32,
    #[serde(default)]
    pub gpu_mhz_avg: i32,
    #[serde(default)]
    pub gpu_mhz_max: i32,
    #[serde(default)]
    pub temp_avg_dc: i32,
    #[serde(default)]
    pub temp_max_dc: i32,
    #[serde(default)]
    pub power_avg_mw: i32,
    #[serde(default)]
    pub power_max_mw: i32,
    #[serde(default)]
    pub ram_avg_mb: i32,
    #[serde(default)]
    pub ram_max_mb: i32,
    #[serde(default)]
    pub cpu_temp_max_dc: i32,
    #[serde(default)]
    pub gpu_temp_max_dc: i32,
    #[serde(default)]
    pub package_ram_avg_mb: i32,
    #[serde(default)]
    pub package_ram_max_mb: i32,
    /// Metric keys that answered at least once, for graph selection.
    #[serde(default)]
    pub available: Vec<String>,
    pub series: Vec<i32>,
    /// Frame time in microseconds, max-pooled like the frame rate.
    #[serde(default)]
    pub frame_series: Vec<i32>,
    #[serde(default)]
    pub cpu_series: Vec<i32>,
    #[serde(default)]
    pub gpu_series: Vec<i32>,
    #[serde(default)]
    pub gpu_mhz_series: Vec<i32>,
    #[serde(default)]
    pub temp_series: Vec<i32>,
    #[serde(default)]
    pub cpu_temp_series: Vec<i32>,
    #[serde(default)]
    pub gpu_temp_series: Vec<i32>,
    #[serde(default)]
    pub power_series: Vec<i32>,
    #[serde(default)]
    pub ram_series: Vec<i32>,
    #[serde(default)]
    pub package_ram_series: Vec<i32>,
    pub series_step_ms: u32,
}

/// The summary numbers the UI prints beside a session.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stats {
    avg_x10: i32,
    min_x10: i32,
    max_x10: i32,
    low1_x10: i32,
    low5_x10: i32,
    slow_pct_x10: u32,
    smooth_pct_x10: u32,
    variance_x100: u32,
    jitter_pct_x10: u32,
    p99_frame_us: u32,
    jank: u32,
}

/// Frame time for one frame-rate sample, in microseconds.
fn frame_us(fps_x10: i32) -> u32 {
    if fps_x10 <= 0 {
        0
    } else {
        (10_000_000 / i64::from(fps_x10)).clamp(0, i64::from(u32::MAX)) as u32
    }
}

/// Percentile of an ascending slice, choosing the nearest rank.
fn percentile(sorted: &[i32], fraction: f64) -> i32 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// Folds one session's readings. `None` when there is nothing to describe.
fn stats(values: &[i32]) -> Option<Stats> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let count = sorted.len();
    let sum: i64 = sorted.iter().map(|value| i64::from(*value)).sum();
    // Round to the nearest tenth instead of truncating, so a 59.96 reading
    // does not read as 59.9.
    let avg_x10 = ((sum + count as i64 / 2) / count as i64) as i32;
    let low_count = (((count as f64) * 0.01).ceil() as usize).clamp(1, count);
    let low_sum: i64 = sorted[..low_count]
        .iter()
        .map(|value| i64::from(*value))
        .sum();
    let low1_x10 = (low_sum / low_count as i64) as i32;
    let low5_x10 = if count >= 20 {
        sorted[(((count as f64) * 0.05) as usize).min(count - 1)]
    } else {
        // Prism hides the 5% low until there is enough data to mean anything.
        0
    };
    let median = sorted[count / 2];
    let slow = values
        .iter()
        .filter(|value| **value * 100 < median * 90)
        .count();
    let slow_pct_x10 = ((slow as u64 * 1_000) / count as u64) as u32;

    let smooth = values
        .iter()
        .filter(|value| (i64::from(**value) - i64::from(avg_x10)).abs() * 10 <= i64::from(avg_x10))
        .count();
    let smooth_pct_x10 = ((smooth as u64 * 1_000) / count as u64) as u32;

    let mean = sum as f64 / count as f64;
    let variance = values
        .iter()
        .map(|value| {
            let delta = f64::from(*value) - mean;
            delta * delta
        })
        .sum::<f64>()
        / count as f64;
    // `values` are tenths of a frame per second, so this number is already
    // fps² times 100.
    let variance_x100 = variance.round().clamp(0.0, f64::from(u32::MAX)) as u32;

    let mut frame_times: Vec<i32> = values.iter().map(|value| frame_us(*value) as i32).collect();
    frame_times.sort_unstable();
    let mean_frame = if frame_times.is_empty() {
        0.0
    } else {
        frame_times
            .iter()
            .map(|value| f64::from(*value))
            .sum::<f64>()
            / frame_times.len() as f64
    };
    let jitter_pct_x10 = if mean_frame <= 0.0 {
        0
    } else {
        let deviation = (frame_times
            .iter()
            .map(|value| {
                let delta = f64::from(*value) - mean_frame;
                delta * delta
            })
            .sum::<f64>()
            / frame_times.len() as f64)
            .sqrt();
        (deviation / mean_frame * 1_000.0).round() as u32
    };
    let p99_frame_us = percentile(&frame_times, 0.99).clamp(0, i32::MAX) as u32;
    let jank_threshold = mean_frame * 1.5;
    let jank = frame_times
        .iter()
        .filter(|value| f64::from(**value) > jank_threshold)
        .count() as u32;

    Some(Stats {
        avg_x10,
        min_x10: sorted[0],
        max_x10: sorted[count - 1],
        low1_x10,
        low5_x10,
        slow_pct_x10,
        smooth_pct_x10,
        variance_x100,
        jitter_pct_x10,
        p99_frame_us,
        jank,
    })
}

/// Max-pools `values` into at most `points` buckets, so the stored graph keeps
/// its spikes while staying small enough to send over the socket.
fn downsample(values: &[i32], points: usize) -> Vec<i32> {
    if points == 0 {
        return Vec::new();
    }
    if values.len() <= points {
        return values.to_vec();
    }
    let mut series = Vec::with_capacity(points);
    for bucket in 0..points {
        let start = bucket * values.len() / points;
        let end = ((bucket + 1) * values.len() / points).max(start + 1);
        series.push(values[start..end].iter().copied().max().unwrap_or(0));
    }
    series
}

fn average_positive(values: &[i32]) -> i32 {
    let mut sum = 0i64;
    let mut count = 0i64;
    for value in values {
        if *value > 0 {
            sum += i64::from(*value);
            count += 1;
        }
    }
    if count == 0 {
        0
    } else {
        ((sum + count / 2) / count) as i32
    }
}

fn max_value(values: &[i32]) -> i32 {
    values.iter().copied().max().unwrap_or(0)
}

fn series_available(key: &str, values: &[i32], available: &mut BTreeSet<String>) {
    if values.iter().any(|value| *value > 0) {
        available.insert(key.to_string());
    }
}

/// The package that contributed the most samples. A session that starts in
/// Settings and moves into a game is labelled with the game, and the stats
/// cover only that package's samples. Ties resolve alphabetically so the
/// result does not change with hash order.
fn dominant_package(samples: &[Sample]) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for sample in samples {
        if let Some(package) = sample.package.as_deref()
            && !package.is_empty()
            && package != "null"
        {
            *counts.entry(package).or_default() += 1;
        }
    }
    let mut best = "";
    let mut best_count = 0;
    for (package, count) in counts {
        if count > best_count {
            best = package;
            best_count = count;
        }
    }
    best.to_string()
}

/// Whether automatic recording should follow this package. Launchers, system
/// dialogs and Dash itself are not games; everything else is fair game.
fn auto_recordable(package: &str) -> bool {
    if package.is_empty() || package == "null" || package == PACKAGE_NAME {
        return false;
    }
    let lower = package.to_ascii_lowercase();
    if lower.contains("launcher") {
        return false;
    }
    !NOT_A_GAME.contains(&lower.as_str())
}

fn start_buffer(recorder: &mut Recorder, auto: bool) -> String {
    recorder.active = true;
    recorder.auto = auto;
    recorder.samples.clear();
    recorder.auto_package = None;
    recorder.auto_candidate = None;
    recorder.auto_misses = 0;
    telemetry::reset();
    telemetry::resolution()
}

fn finish_buffer(recorder: &mut Recorder, session_auto: bool) -> Option<Session> {
    recorder.active = false;
    recorder.auto_package = None;
    recorder.auto_candidate = None;
    recorder.auto_misses = 0;
    let samples: Vec<Sample> = recorder.samples.drain(..).collect();
    build_session(&samples, session_auto, &recorder.resolution)
}

fn build_session(samples: &[Sample], auto: bool, resolution: &str) -> Option<Session> {
    let package = dominant_package(samples);
    let kept: Vec<&Sample> = samples
        .iter()
        .filter(|sample| package.is_empty() || sample.package.as_deref() == Some(package.as_str()))
        .collect();
    if kept.len() < 2 {
        return None;
    }
    let values: Vec<i32> = kept.iter().map(|sample| sample.fps_x10).collect();
    let stats = stats(&values)?;
    let duration_ms = kept
        .last()
        .map(|sample| sample.at_ms)
        .unwrap_or(0)
        .saturating_sub(kept[0].at_ms)
        .saturating_add(u64::from(SAMPLE_INTERVAL_MS));
    // The session starts at its first kept sample, not when recording was
    // armed: a switch flipped in Settings and carried into a game must be
    // stamped when the game appeared.
    let started_at = kept[0].at_ms;
    let frame_times: Vec<i32> = values.iter().map(|value| frame_us(*value) as i32).collect();
    let cpu: Vec<i32> = kept.iter().map(|sample| sample.hardware.cpu_load).collect();
    let gpu: Vec<i32> = kept.iter().map(|sample| sample.hardware.gpu_load).collect();
    let gpu_mhz: Vec<i32> = kept.iter().map(|sample| sample.hardware.gpu_mhz).collect();
    let temp: Vec<i32> = kept
        .iter()
        .map(|sample| sample.hardware.battery_temp_dc)
        .collect();
    let cpu_temp: Vec<i32> = kept
        .iter()
        .map(|sample| sample.hardware.cpu_temp_dc)
        .collect();
    let gpu_temp: Vec<i32> = kept
        .iter()
        .map(|sample| sample.hardware.gpu_temp_dc)
        .collect();
    let power: Vec<i32> = kept.iter().map(|sample| sample.hardware.power_mw).collect();
    let ram: Vec<i32> = kept
        .iter()
        .map(|sample| sample.hardware.ram_used_mb)
        .collect();
    let package_ram: Vec<i32> = kept
        .iter()
        .map(|sample| sample.hardware.package_ram_mb)
        .collect();
    let mut available = BTreeSet::new();
    available.insert("frameTime".to_string());
    series_available("cpuLoad", &cpu, &mut available);
    series_available("gpuLoad", &gpu, &mut available);
    series_available("gpuMhz", &gpu_mhz, &mut available);
    series_available("batteryTemp", &temp, &mut available);
    series_available("power", &power, &mut available);
    series_available("ram", &ram, &mut available);
    series_available("cpuTemp", &cpu_temp, &mut available);
    series_available("gpuTemp", &gpu_temp, &mut available);
    series_available("packageRam", &package_ram, &mut available);
    let series = downsample(&values, SERIES_POINTS);
    let series_step_ms = (duration_ms / series.len().max(1) as u64).max(1) as u32;
    Some(Session {
        id: started_at,
        package,
        started_at,
        duration_ms,
        samples: values.len() as u32,
        avg_x10: stats.avg_x10,
        min_x10: stats.min_x10,
        max_x10: stats.max_x10,
        low1_x10: stats.low1_x10,
        slow_pct_x10: stats.slow_pct_x10,
        low5_x10: stats.low5_x10,
        variance_x100: stats.variance_x100,
        smooth_pct_x10: stats.smooth_pct_x10,
        jitter_pct_x10: stats.jitter_pct_x10,
        p99_frame_us: stats.p99_frame_us,
        jank: stats.jank,
        description: String::new(),
        auto,
        resolution: resolution.to_string(),
        cpu_avg: average_positive(&cpu),
        cpu_max: max_value(&cpu),
        gpu_load_avg: average_positive(&gpu),
        gpu_load_max: max_value(&gpu),
        gpu_mhz_avg: average_positive(&gpu_mhz),
        gpu_mhz_max: max_value(&gpu_mhz),
        temp_avg_dc: average_positive(&temp),
        temp_max_dc: max_value(&temp),
        power_avg_mw: average_positive(&power),
        power_max_mw: max_value(&power),
        ram_avg_mb: average_positive(&ram),
        ram_max_mb: max_value(&ram),
        cpu_temp_max_dc: max_value(&cpu_temp),
        gpu_temp_max_dc: max_value(&gpu_temp),
        package_ram_avg_mb: average_positive(&package_ram),
        package_ram_max_mb: max_value(&package_ram),
        available: available.into_iter().collect(),
        series,
        frame_series: downsample(&frame_times, SERIES_POINTS),
        cpu_series: downsample(&cpu, SERIES_POINTS),
        gpu_series: downsample(&gpu, SERIES_POINTS),
        gpu_mhz_series: downsample(&gpu_mhz, SERIES_POINTS),
        temp_series: downsample(&temp, SERIES_POINTS),
        cpu_temp_series: downsample(&cpu_temp, SERIES_POINTS),
        gpu_temp_series: downsample(&gpu_temp, SERIES_POINTS),
        power_series: downsample(&power, SERIES_POINTS),
        ram_series: downsample(&ram, SERIES_POINTS),
        package_ram_series: downsample(&package_ram, SERIES_POINTS),
        series_step_ms,
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
struct Recorder {
    active: bool,
    /// Automatic mode follows the foreground app instead of the UI switch.
    auto: bool,
    samples: VecDeque<Sample>,
    /// Resolution captured when the current session started.
    resolution: String,
    auto_package: Option<String>,
    auto_candidate: Option<(String, u8)>,
    auto_misses: u8,
}

fn recorder() -> &'static Mutex<Recorder> {
    static RECORDER: OnceLock<Mutex<Recorder>> = OnceLock::new();
    RECORDER.get_or_init(|| Mutex::new(Recorder::default()))
}

/// A poisoned recorder still owns valid samples; recovering the guard is
/// kinder than making recording permanently impossible.
fn lock_recorder() -> MutexGuard<'static, Recorder> {
    recorder()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Serialises history rewrites with appends. The socket thread can delete a
/// row while the sampler is saving a finished session.
fn history_lock() -> MutexGuard<'static, ()> {
    static HISTORY: OnceLock<Mutex<()>> = OnceLock::new();
    HISTORY
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn sessions_path() -> PathBuf {
    crate::state_dir().join(FILE_NAME)
}

fn auto_path() -> PathBuf {
    crate::state_dir().join(AUTO_FILE_NAME)
}

fn read_auto(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|text| text.trim() == "1")
        .unwrap_or(false)
}

fn write_auto(path: &Path, auto: bool) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let _ = fs::remove_file(&temporary);
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(if auto { b"1\n" } else { b"0\n" })?;
        file.sync_all()?;
    }
    fs::rename(&temporary, path)
}

fn read_sessions(path: &Path) -> Vec<Session> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Writes the whole history through a temporary file so a crash cannot leave
/// a half-trimmed list. The caller holds [`history_lock`].
fn rewrite_sessions(path: &Path, sessions: &[Session]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("jsonl.tmp");
    let _ = fs::remove_file(&temporary);
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        for session in sessions {
            serde_json::to_writer(&mut file, session).map_err(io::Error::other)?;
            file.write_all(b"\n")?;
        }
        file.sync_all()?;
    }
    fs::rename(&temporary, path)
}

/// Keeps the newest [`MAX_SESSIONS`] rows. The caller holds the history lock.
fn trim_sessions(path: &Path, sessions: &[Session]) -> io::Result<()> {
    if sessions.len() <= MAX_SESSIONS {
        return Ok(());
    }
    rewrite_sessions(path, &sessions[sessions.len() - MAX_SESSIONS..])
}

/// Appends one session and trims the file. The caller holds the history lock.
fn append_session_locked(path: &Path, session: &Session) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(session).map_err(io::Error::other)?;
    {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        writeln!(file, "{line}")?;
    }
    trim_sessions(path, &read_sessions(path))
}

fn append_session(path: &Path, session: &Session) -> io::Result<()> {
    let _history = history_lock();
    append_session_locked(path, session)
}

/// Arms or disarms manual recording. Starting clears any leftover buffer;
/// stopping folds the buffer into a session, which is dropped when it is too
/// short to say anything.
pub(super) fn set_active(active: bool) {
    let finished = {
        let mut recorder = lock_recorder();
        if active {
            // A manual start is an explicit override: stop following the
            // foreground app and keep recording until the switch is turned off.
            if recorder.auto || !recorder.active {
                let was_auto = recorder.auto;
                let previous = if recorder.active {
                    finish_buffer(&mut recorder, was_auto)
                } else {
                    None
                };
                recorder.auto = false;
                recorder.resolution = start_buffer(&mut recorder, false);
                drop(recorder);
                if let Some(session) = previous {
                    save(session);
                }
                let _ = write_auto(&auto_path(), false);
                return;
            }
            return;
        }
        if !recorder.auto && !recorder.active {
            return;
        }
        let auto = recorder.auto;
        recorder.auto = false;
        finish_buffer(&mut recorder, auto)
    };
    if let Some(session) = finished {
        save(session);
    }
    let _ = write_auto(&auto_path(), false);
}

fn save(session: Session) {
    if let Err(error) = append_session(&sessions_path(), &session) {
        eprintln!("DASH_FPS_RECORD_SAVE_FAIL {error}");
    }
}

/// Turns Prism-style automatic recording on or off. Starting it saves a
/// manual session first; the next foreground game opens a fresh one.
pub(super) fn set_auto(active: bool) -> Result<Value, String> {
    let (finished, unchanged) = {
        let mut recorder = lock_recorder();
        if recorder.auto == active {
            (None, true)
        } else {
            let was_auto = recorder.auto;
            let previous = if recorder.active {
                finish_buffer(&mut recorder, was_auto)
            } else {
                None
            };
            recorder.auto = active;
            if active {
                recorder.active = false;
                recorder.resolution.clear();
            } else {
                recorder.auto_package = None;
                recorder.auto_candidate = None;
                recorder.auto_misses = 0;
            }
            (previous, false)
        }
    };
    if unchanged {
        return Ok(live_json());
    }
    if let Some(session) = finished {
        save(session);
    }
    write_auto(&auto_path(), active).map_err(|error| error.to_string())?;
    Ok(live_json())
}

/// What the settings screen polls: whether recording is live, how long it has
/// run, and the averages so far.
pub(super) fn live_json() -> Value {
    let mut recorder = lock_recorder();
    // Report the session that would be saved, not a blend of the app the
    // switch was flipped in and the game that followed it.
    let package = dominant_package(recorder.samples.make_contiguous());
    let kept: Vec<&Sample> = recorder
        .samples
        .iter()
        .filter(|sample| package.is_empty() || sample.package.as_deref() == Some(package.as_str()))
        .collect();
    let values: Vec<i32> = kept.iter().map(|sample| sample.fps_x10).collect();
    let stats = stats(&values);
    let started_at = kept.first().map_or(0, |sample| sample.at_ms);
    json!({
        "active": recorder.active,
        "auto": recorder.auto,
        "startedAt": started_at,
        "elapsedMs": if recorder.active && started_at > 0 {
            now_ms().saturating_sub(started_at)
        } else {
            0
        },
        "samples": values.len(),
        "package": if package.is_empty() { Value::Null } else { Value::String(package) },
        "avgX10": stats.map_or(0, |stats| stats.avg_x10),
        "lastFpsX10": values.last().copied().unwrap_or(0),
    })
}

/// Newest first, because that is how the history list reads.
pub(super) fn sessions_json() -> Value {
    let mut sessions = {
        let _history = history_lock();
        read_sessions(&sessions_path())
    };
    sessions.reverse();
    json!(sessions)
}

/// Deletes the stored history. An in-progress recording keeps running: the
/// user is clearing old rows, not cancelling the current one.
pub(super) fn clear() -> Result<usize, String> {
    let _history = history_lock();
    let path = sessions_path();
    let cleared = read_sessions(&path).len();
    match fs::remove_file(&path) {
        Ok(()) => Ok(cleared),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(cleared),
        Err(error) => Err(error.to_string()),
    }
}

/// Deletes one session by its start timestamp.
pub(super) fn delete(id: u64) -> Result<bool, String> {
    let _history = history_lock();
    let path = sessions_path();
    let mut sessions = read_sessions(&path);
    let before = sessions.len();
    sessions.retain(|session| session.id != id);
    if sessions.len() == before {
        return Ok(false);
    }
    rewrite_sessions(&path, &sessions).map_err(|error| error.to_string())?;
    Ok(true)
}

/// Stores the user's note on one session. An empty note clears it.
pub(super) fn set_note(id: u64, note: &str) -> Result<bool, String> {
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(format!("a note is limited to {MAX_NOTE_CHARS} characters"));
    }
    let _history = history_lock();
    let path = sessions_path();
    let mut sessions = read_sessions(&path);
    let Some(session) = sessions.iter_mut().find(|session| session.id == id) else {
        return Ok(false);
    };
    session.description = note.trim().to_string();
    rewrite_sessions(&path, &sessions).map_err(|error| error.to_string())?;
    Ok(true)
}

fn sample_once() {
    // Read outside the lock: node reads and the foreground lookup are the
    // slow part, and a settings poll must never wait behind them.
    let (reading, package) = super::foreground_fps::read_with_package();
    let package_name = package.unwrap_or_default();
    let reading_valid = reading.valid && reading.fps.is_finite() && reading.fps > 0.0;
    let fps_x10 = (reading.fps * 10.0).round() as i32;
    let NowSample { finished } = {
        let mut recorder = lock_recorder();
        let mut finished = None;

        if recorder.auto {
            if recorder.active {
                let same = recorder.auto_package.as_deref() == Some(package_name.as_str())
                    && auto_recordable(&package_name);
                if same {
                    recorder.auto_misses = 0;
                } else {
                    recorder.auto_misses = recorder.auto_misses.saturating_add(1);
                }
                if !reading_valid || recorder.auto_misses >= 2 {
                    let auto = recorder.auto;
                    let resolution = recorder.resolution.clone();
                    recorder.active = false;
                    recorder.auto_package = None;
                    recorder.auto_candidate = None;
                    recorder.auto_misses = 0;
                    let samples: Vec<Sample> = recorder.samples.drain(..).collect();
                    finished = build_session(&samples, auto, &resolution);
                }
            }
            if !recorder.active && auto_recordable(&package_name) {
                let candidate = recorder.auto_candidate.take();
                let hits = match candidate {
                    Some((previous, hits)) if previous == package_name => hits.saturating_add(1),
                    _ => 1,
                };
                if hits >= 2 {
                    recorder.resolution = start_buffer(&mut recorder, true);
                    recorder.auto_package = Some(package_name.clone());
                } else {
                    recorder.auto_candidate = Some((package_name.clone(), hits));
                }
            }
        }

        if recorder.active && reading_valid && fps_x10 > 0 {
            if recorder.samples.len() >= MAX_SAMPLES {
                recorder.samples.pop_front();
            }
            let hardware = telemetry::sample(if package_name.is_empty() {
                None
            } else {
                Some(&package_name)
            });
            recorder.samples.push_back(Sample {
                fps_x10,
                at_ms: now_ms(),
                package: if package_name.is_empty() {
                    None
                } else {
                    Some(package_name.clone())
                },
                hardware,
            });
        }
        NowSample { finished }
    };
    if let Some(session) = finished {
        save(session);
    }
}

struct NowSample {
    finished: Option<Session>,
}

/// Starts the sampler. It sleeps when recording is off, so an idle daemon
/// costs one flag check per half second.
pub(super) fn start() {
    {
        let mut recorder = lock_recorder();
        recorder.auto = read_auto(&auto_path());
    }
    if let Err(error) = std::thread::Builder::new()
        .name("fps-record".into())
        .spawn(|| {
            loop {
                std::thread::sleep(SAMPLE_INTERVAL);
                sample_once();
            }
        })
    {
        eprintln!("DASH_FPS_RECORD_START_FAIL {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at_ms: u64, fps_x10: i32, package: Option<&str>) -> Sample {
        Sample {
            fps_x10,
            at_ms,
            package: package.map(str::to_owned),
            hardware: Telemetry::default(),
        }
    }

    #[test]
    fn folds_known_samples_into_stats() {
        let stats = stats(&[600, 600, 600, 600, 300]).expect("stats");
        assert_eq!(stats.avg_x10, 540);
        assert_eq!(stats.min_x10, 300);
        assert_eq!(stats.max_x10, 600);
        assert_eq!(stats.low1_x10, 300);
        // One of five samples is under 90% of the 60 fps median.
        assert_eq!(stats.slow_pct_x10, 200);
        // The 60 fps samples sit 11% over the 54 fps average, so none of them
        // is inside the ten percent band that relative smoothness measures.
        assert_eq!(stats.smooth_pct_x10, 0);
        // 60 fps for 100 ms then 30 fps: the 99th percentile is the hitch.
        assert_eq!(stats.p99_frame_us, 33_333);
        assert!(stats.jank > 0);
        assert!(stats.variance_x100 > 0);
    }

    #[test]
    fn one_percent_low_averages_the_slowest_samples() {
        let mut values = vec![600; 198];
        values.extend([200, 400]);
        let stats = stats(&values).expect("stats");
        // 1% of 200 is two samples: (20 + 40) / 2 = 30 fps.
        assert_eq!(stats.low1_x10, 300);
    }

    #[test]
    fn five_percent_low_needs_enough_samples() {
        let mut values = vec![600; 20];
        values[0] = 100;
        values[1] = 100;
        let folded = stats(&values).expect("stats");
        // Prism reads the fifth-percent index: 20 samples -> sorted[1].
        assert_eq!(folded.low5_x10, 100);
        assert_eq!(stats(&[600; 4]).expect("stats").low5_x10, 0);
    }

    #[test]
    fn smoothness_counts_samples_within_ten_percent() {
        let mut values = vec![600; 10];
        values[9] = 300;
        // The average of nine 60 fps samples and one 30 fps sample is 57 fps;
        // only the slow sample falls outside its ten percent band.
        let folded = stats(&values).expect("stats");
        assert_eq!(folded.avg_x10, 570);
        assert_eq!(folded.smooth_pct_x10, 900);
    }

    #[test]
    fn a_steady_session_has_no_slow_samples() {
        let stats = stats(&[600; 40]).expect("stats");
        assert_eq!(stats.slow_pct_x10, 0);
        assert_eq!(stats.low1_x10, 600);
        assert_eq!(stats.avg_x10, 600);
        assert_eq!(stats.variance_x100, 0);
        assert_eq!(stats.jitter_pct_x10, 0);
        assert_eq!(stats.smooth_pct_x10, 1_000);
        assert_eq!(stats.jank, 0);
    }

    #[test]
    fn empty_samples_have_no_stats() {
        assert_eq!(stats(&[]), None);
    }

    #[test]
    fn downsample_keeps_spikes_and_bucket_count() {
        let mut values: Vec<i32> = (1..=1_000).collect();
        values[500] = 10_000;
        let series = downsample(&values, 10);
        assert_eq!(series.len(), 10);
        assert!(series.contains(&10_000));
        // A short series is returned untouched.
        assert_eq!(downsample(&[600, 610], 10), vec![600, 610]);
    }

    #[test]
    fn session_is_labelled_by_the_dominant_package() {
        let samples = [
            sample(1_000, 300, Some("com.other")),
            sample(2_000, 600, Some("com.game")),
            sample(2_500, 610, Some("com.game")),
            sample(3_000, 590, Some("com.game")),
            sample(3_500, 600, None),
        ];
        let session = build_session(&samples, false, "1080x2400").expect("session");
        assert_eq!(session.package, "com.game");
        // Only the game's three samples describe the game.
        assert_eq!(session.samples, 3);
        assert_eq!(session.duration_ms, 1_500);
        // The session starts with the first sample of the game it is filed
        // under, not with the app the recording was armed in.
        assert_eq!(session.started_at, 2_000);
        assert_eq!(session.id, 2_000);
        assert_eq!(session.avg_x10, 600);
        assert_eq!(session.resolution, "1080x2400");
        assert!(!session.series.is_empty());
        assert!(session.series_step_ms >= 1);
        assert!(session.available.contains(&"frameTime".to_string()));
    }

    #[test]
    fn a_session_too_short_to_describe_is_discarded() {
        assert_eq!(
            build_session(&[sample(1_000, 600, Some("com.game"))], false, ""),
            None
        );
        assert_eq!(build_session(&[], false, ""), None);
    }

    #[test]
    fn only_non_launcher_apps_are_auto_recorded() {
        assert!(auto_recordable("com.example.game"));
        assert!(!auto_recordable("com.miui.home"));
        assert!(!auto_recordable("com.example.launcher"));
        assert!(!auto_recordable(PACKAGE_NAME));
        assert!(!auto_recordable(""));
        assert!(!auto_recordable("com.android.systemui"));
    }

    #[test]
    fn notes_and_deletes_rewrite_the_history() {
        let directory = std::env::temp_dir().join(format!(
            "dash-fps-notes-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("directory");
        let path = directory.join(FILE_NAME);
        let session = build_session(
            &[
                sample(1_000, 600, Some("com.game")),
                sample(1_500, 610, Some("com.game")),
            ],
            false,
            "",
        )
        .expect("session");
        append_session(&path, &session).expect("append");
        assert!(set_note_at(&path, session.id, "boss fight").expect("note"));
        let stored = read_sessions(&path);
        assert_eq!(stored[0].description, "boss fight");
        assert!(delete_at(&path, session.id).expect("delete"));
        assert!(read_sessions(&path).is_empty());
        let _ = fs::remove_dir_all(&directory);
    }

    fn set_note_at(path: &Path, id: u64, note: &str) -> Result<bool, String> {
        let _history = history_lock();
        let mut sessions = read_sessions(path);
        let Some(session) = sessions.iter_mut().find(|session| session.id == id) else {
            return Ok(false);
        };
        session.description = note.to_string();
        rewrite_sessions(path, &sessions).map_err(|error| error.to_string())?;
        Ok(true)
    }

    fn delete_at(path: &Path, id: u64) -> Result<bool, String> {
        let _history = history_lock();
        let mut sessions = read_sessions(path);
        let before = sessions.len();
        sessions.retain(|session| session.id != id);
        if sessions.len() == before {
            return Ok(false);
        }
        rewrite_sessions(path, &sessions).map_err(|error| error.to_string())?;
        Ok(true)
    }

    #[test]
    fn jsonl_round_trip_trims_to_the_newest_rows() {
        let directory = std::env::temp_dir().join(format!(
            "dash-fps-record-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&directory);
        let path = directory.join(FILE_NAME);

        let mut samples = [
            sample(0, 600, Some("com.game")),
            sample(500, 600, Some("com.game")),
        ];
        for id in 0..(MAX_SESSIONS as u64 + 5) {
            samples[0].at_ms = id;
            samples[1].at_ms = id + 500;
            let session = build_session(&samples, false, "").expect("session");
            append_session(&path, &session).expect("append");
        }
        let stored = read_sessions(&path);
        assert_eq!(stored.len(), MAX_SESSIONS);
        // The five oldest rows are the ones that went away.
        assert_eq!(stored[0].id, 5);
        assert_eq!(stored[stored.len() - 1].id, (MAX_SESSIONS + 4) as u64);
        assert_eq!(stored[0].package, "com.game");
        // Trimming rewrites the file; it must not relax the 0600 mode.
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn old_rows_without_telemetry_still_load() {
        let line = r#"{"id":1,"package":"com.game","startedAt":1,"durationMs":1000,"samples":2,"avgX10":600,"minX10":590,"maxX10":610,"low1X10":590,"slowPctX10":0,"series":[600,610],"seriesStepMs":500}"#;
        let session: Session = serde_json::from_str(line).expect("old session");
        assert_eq!(session.description, "");
        assert!(session.available.is_empty());
        assert!(session.frame_series.is_empty());
        assert!(!session.auto);
    }
}
