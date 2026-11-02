//! FPS session recording over the same snapshot the overlay reads.
//!
//! While recording is armed, one sample is taken every [`SAMPLE_INTERVAL`]
//! from `foreground_fps`. Stopping folds the samples into one session and
//! appends it to a compact JSON-lines file beside the daemon's other state.
//!
//! The recorder never invents data: samples the platform could not measure
//! are skipped, and a session with fewer than two valid samples is discarded
//! instead of being stored as a row of zeroes.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Two samples a second: fast enough to catch a stutter, slow enough to keep
/// an hour of play small.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);
const SAMPLE_INTERVAL_MS: u32 = 500;

/// An hour of samples. Older ones roll out of the ring; the session keeps
/// recording so a long play session is never truncated silently.
const MAX_SAMPLES: usize = 7_200;

/// Points kept in the stored graph. Each bucket keeps its maximum, so a
/// one-off hitch survives downsampling instead of being averaged away.
const SERIES_POINTS: usize = 180;

/// Sessions kept on disk. Beyond this the oldest rows are dropped.
const MAX_SESSIONS: usize = 200;

const FILE_NAME: &str = "fps_sessions.jsonl";

/// One measurement before it is folded into a session.
#[derive(Clone, Debug)]
struct Sample {
    fps_x10: i32,
    package: Option<String>,
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
    pub series: Vec<i32>,
    pub series_step_ms: u32,
}

/// The summary numbers the UI prints beside a session.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stats {
    avg_x10: i32,
    min_x10: i32,
    max_x10: i32,
    low1_x10: i32,
    slow_pct_x10: u32,
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
    // The slowest one percent, averaged. Averaging is what makes a "1% low"
    // stable; a single bucket can be one anomalous sample.
    let low_count = (((count as f64) * 0.01).ceil() as usize).clamp(1, count);
    let low_sum: i64 = sorted[..low_count]
        .iter()
        .map(|value| i64::from(*value))
        .sum();
    let low1_x10 = (low_sum / low_count as i64) as i32;
    let median = sorted[count / 2];
    // A sample more than ten percent under the session's median is a slow
    // one; `v * 100 < median * 90` stays in integers.
    let slow = values
        .iter()
        .filter(|value| **value * 100 < median * 90)
        .count();
    let slow_pct_x10 = ((slow as u64 * 1_000) / count as u64) as u32;
    Some(Stats {
        avg_x10,
        min_x10: sorted[0],
        max_x10: sorted[count - 1],
        low1_x10,
        slow_pct_x10,
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

fn build_session(started_at: u64, samples: &[Sample]) -> Option<Session> {
    let package = dominant_package(samples);
    let values: Vec<i32> = samples
        .iter()
        .filter(|sample| package.is_empty() || sample.package.as_deref() == Some(package.as_str()))
        .map(|sample| sample.fps_x10)
        .collect();
    if values.len() < 2 {
        return None;
    }
    let stats = stats(&values)?;
    let duration_ms = values.len() as u64 * u64::from(SAMPLE_INTERVAL_MS);
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
        series,
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
    started_at: u64,
    samples: VecDeque<Sample>,
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

fn sessions_path() -> PathBuf {
    crate::state_dir().join(FILE_NAME)
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

fn append_session(path: &Path, session: &Session) -> io::Result<()> {
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
    trim_sessions(path)
}

/// Keeps the newest [`MAX_SESSIONS`] rows. The rewrite goes through a
/// temporary file so a crash mid-trim cannot leave a half-written history.
fn trim_sessions(path: &Path) -> io::Result<()> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() <= MAX_SESSIONS {
        return Ok(());
    }
    let mut kept = String::new();
    for line in &lines[lines.len() - MAX_SESSIONS..] {
        kept.push_str(line);
        kept.push('\n');
    }
    let temporary = path.with_extension("jsonl.tmp");
    fs::write(&temporary, kept)?;
    fs::rename(&temporary, path)
}

/// Arms or disarms recording. Starting clears any leftover buffer; stopping
/// folds the buffer into a session, which is dropped when it is too short to
/// say anything.
pub(super) fn set_active(active: bool) {
    let finished = {
        let mut recorder = lock_recorder();
        if recorder.active == active {
            return;
        }
        if active {
            recorder.active = true;
            recorder.started_at = now_ms();
            recorder.samples.clear();
            None
        } else {
            recorder.active = false;
            let started_at = recorder.started_at;
            let samples: Vec<Sample> = recorder.samples.drain(..).collect();
            build_session(started_at, &samples)
        }
    };
    if let Some(session) = finished
        && let Err(error) = append_session(&sessions_path(), &session)
    {
        eprintln!("DASH_FPS_RECORD_SAVE_FAIL {error}");
    }
}

/// What the settings screen polls: whether recording is live, how long it has
/// run, and the averages so far.
pub(super) fn live_json() -> Value {
    let mut recorder = lock_recorder();
    let values: Vec<i32> = recorder
        .samples
        .iter()
        .map(|sample| sample.fps_x10)
        .collect();
    let stats = stats(&values);
    let package = dominant_package(recorder.samples.make_contiguous());
    json!({
        "active": recorder.active,
        "startedAt": recorder.started_at,
        "elapsedMs": if recorder.active {
            now_ms().saturating_sub(recorder.started_at)
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
    let mut sessions = read_sessions(&sessions_path());
    sessions.reverse();
    json!(sessions)
}

/// Deletes the stored history. An in-progress recording keeps running: the
/// user is clearing old rows, not cancelling the current one.
pub(super) fn clear() -> Result<usize, String> {
    let path = sessions_path();
    let cleared = read_sessions(&path).len();
    match fs::remove_file(&path) {
        Ok(()) => Ok(cleared),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(cleared),
        Err(error) => Err(error.to_string()),
    }
}

fn sample_once() {
    if !lock_recorder().active {
        return;
    }
    // Read outside the lock: node reads and the foreground lookup are the
    // slow part, and a settings poll must never wait behind them.
    let (reading, package) = super::foreground_fps::read_with_package();
    if !reading.valid || !reading.fps.is_finite() || reading.fps <= 0.0 {
        return;
    }
    let fps_x10 = (reading.fps * 10.0).round() as i32;
    if fps_x10 <= 0 {
        return;
    }
    let mut recorder = lock_recorder();
    if !recorder.active {
        return;
    }
    if recorder.samples.len() >= MAX_SAMPLES {
        recorder.samples.pop_front();
    }
    recorder.samples.push_back(Sample { fps_x10, package });
}

/// Starts the sampler. It sleeps when recording is off, so an idle daemon
/// costs one flag check per half second.
pub(super) fn start() {
    let _ = std::thread::Builder::new()
        .name("fps-record".into())
        .spawn(|| {
            loop {
                std::thread::sleep(SAMPLE_INTERVAL);
                sample_once();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(fps_x10: i32, package: Option<&str>) -> Sample {
        Sample {
            fps_x10,
            package: package.map(str::to_owned),
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
    fn a_steady_session_has_no_slow_samples() {
        let stats = stats(&[600; 40]).expect("stats");
        assert_eq!(stats.slow_pct_x10, 0);
        assert_eq!(stats.low1_x10, 600);
        assert_eq!(stats.avg_x10, 600);
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
            sample(300, Some("com.other")),
            sample(600, Some("com.game")),
            sample(610, Some("com.game")),
            sample(590, Some("com.game")),
            sample(600, None),
        ];
        let session = build_session(1_700_000_000_000, &samples).expect("session");
        assert_eq!(session.package, "com.game");
        // Only the game's three samples describe the game.
        assert_eq!(session.samples, 3);
        assert_eq!(session.duration_ms, 1_500);
        assert_eq!(session.avg_x10, 600);
        assert!(!session.series.is_empty());
        assert!(session.series_step_ms >= 1);
    }

    #[test]
    fn a_session_too_short_to_describe_is_discarded() {
        assert_eq!(build_session(1, &[sample(600, Some("com.game"))]), None);
        assert_eq!(build_session(1, &[]), None);
    }

    #[test]
    fn arming_without_samples_stores_nothing() {
        set_active(true);
        assert_eq!(live_json()["active"].as_bool(), Some(true));
        // No sampler thread runs here, so stopping must discard the empty
        // buffer rather than write a session of zeroes.
        set_active(false);
        assert_eq!(live_json()["active"].as_bool(), Some(false));
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

        let samples = [sample(600, Some("com.game")), sample(600, Some("com.game"))];
        for id in 0..(MAX_SESSIONS as u64 + 5) {
            let session = build_session(id, &samples).expect("session");
            append_session(&path, &session).expect("append");
        }
        let stored = read_sessions(&path);
        assert_eq!(stored.len(), MAX_SESSIONS);
        // The five oldest rows are the ones that went away.
        assert_eq!(stored[0].id, 5);
        assert_eq!(stored[stored.len() - 1].id, (MAX_SESSIONS + 4) as u64);
        assert_eq!(stored[0].package, "com.game");

        let _ = fs::remove_dir_all(&directory);
    }
}
