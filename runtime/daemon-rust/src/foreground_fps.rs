//! Foreground FPS from the platform's frame statistics.
//!
//! Flutter frame callbacks stop when Dash stops drawing, so they can never
//! see a game. The daemon runs as root and can read the vendor frame nodes
//! wherever the ROM exposes them; this module turns those nodes into one
//! stable reading for the system overlay.
//!
//! ROMs disagree about both the path and the format, and on a platform with
//! no frame node at all the absence is normal. Every parser below is therefore
//! tolerant: an unreadable or unknown node degrades to `valid: false` with the
//! probe that failed, never to a fabricated number.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Nodes that carry an aggregate FPS for the current scene, best first.
const AGGREGATE_NODES: &[&str] = &[
    "/sys/kernel/fpsgo/fps_info",
    "/sys/kernel/fpsgo/fstb/fps_info",
    "/sys/kernel/fpsgo/common/fps_info",
    "/proc/fpsgo/fps_info",
    "/sys/kernel/fpsgo/fstb/fps",
    "/sys/kernel/fpsgo/fstb/cur_fps",
    "/sys/class/display/fps",
];

/// Per-thread FPS table with the app name in each row. The only node that
/// gives both the rate and the attribution in one read, so it is tried
/// before the render-thread scan when no aggregate node answers.
const FPS_LIST_NODE: &str = "/sys/kernel/fpsgo/fstb/fstb_fps_list";

/// Per-render-thread nodes, scanned only when no aggregate node exists.
const RENDER_THREAD_ROOT: &str = "/sys/kernel/fpsgo/fstb";

/// A bare number at or below this is an instantaneous FPS; anything larger is
/// a frame counter that only becomes a rate once two samples are compared.
const INSTANT_CEILING: f64 = 1000.0;

/// The reading the overlay and the diagnostics view consume.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(super) struct FpsReading {
    /// Average frames per second for the sampled interval.
    pub fps: f64,
    /// Worst single-frame FPS when the node reports it, else `0.0`.
    pub min: f64,
    /// Worst one percent of frame intervals when the node reports it, else
    /// `0.0`. Zero means "not measured", not "no frames".
    pub low1: f64,
    /// Frames that missed their deadline when the node reports it, else `0`.
    pub dropped: u32,
    /// Which node answered, or `unavailable`.
    pub node: String,
    /// True only when a node was read and its value was plausible.
    pub valid: bool,
}

impl FpsReading {
    fn unavailable() -> Self {
        Self {
            fps: 0.0,
            min: 0.0,
            low1: 0.0,
            dropped: 0,
            node: "unavailable".into(),
            valid: false,
        }
    }
}

/// Fields lifted from one node's text before a bare counter is resolved.
#[derive(Default)]
struct Fields {
    fps: Option<f64>,
    min: Option<f64>,
    low1: Option<f64>,
    dropped: Option<f64>,
    /// The last bare number on a node that named no FPS field.
    bare: Option<f64>,
}

/// The first number that follows `key` as a whole word. Separators (`=`, `:`,
/// spaces) are skipped; the sign is parsed, so `fps=-1` stays negative instead
/// of becoming a plausible `1`.
fn number_after(text: &str, keys: &[&str]) -> Option<f64> {
    let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
    for key in keys {
        let mut start = 0;
        while let Some(offset) = text[start..].find(key) {
            let index = start + offset;
            let after = index + key.len();
            if boundary(text[..index].chars().next_back())
                && boundary(text[after..].chars().next())
                && let Some(value) = leading_float(&text[after..])
            {
                return Some(value);
            }
            start = after;
        }
    }
    None
}

fn leading_float(text: &str) -> Option<f64> {
    let trimmed = text.trim_start_matches(|c: char| c.is_whitespace() || c == '=' || c == ':');
    let end = trimmed
        .find(|c: char| {
            !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')
        })
        .unwrap_or(trimmed.len());
    trimmed[..end].parse().ok()
}

fn parse_fields(text: &str) -> Fields {
    let lowered = text.to_ascii_lowercase();
    let mut fields = Fields {
        fps: number_after(
            &lowered,
            &["fps", "average_fps", "avg_fps", "ins_fps", "cur_fps"],
        ),
        min: number_after(&lowered, &["min", "min_fps", "minimum_fps"]),
        low1: number_after(&lowered, &["low1", "low1_fps", "one_percent_low"]),
        dropped: number_after(&lowered, &["dropped", "drop", "dropped_frames"]),
        bare: None,
    };
    if fields.fps.is_none() {
        // Some nodes are a single number. Keep it; `reading_from_node`
        // decides whether it is a rate or a counter.
        fields.bare = lowered
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
            .filter(|token| !token.is_empty())
            .find_map(|token| token.parse::<f64>().ok());
    }
    fields
}

fn plausible(fps: f64) -> bool {
    fps.is_finite() && (0.0..=INSTANT_CEILING).contains(&fps)
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

#[derive(Clone, Copy)]
struct CounterSample {
    frames: f64,
    at: Instant,
}

static COUNTERS: OnceLock<Mutex<HashMap<String, CounterSample>>> = OnceLock::new();

/// Turns a frame counter into a rate. The first sample only establishes a
/// baseline, so a counter node answers nothing until it has been read twice.
fn counter_rate(node: &str, frames: f64) -> Option<f64> {
    if frames < 0.0 {
        return None;
    }
    let now = Instant::now();
    let samples = COUNTERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut samples = samples.lock().ok()?;
    let previous = samples.get(node).copied();
    samples.insert(node.to_string(), CounterSample { frames, at: now });
    let previous = previous?;
    rate_from_delta(
        frames - previous.frames,
        now.duration_since(previous.at).as_secs_f64(),
    )
}

fn rate_from_delta(frames: f64, elapsed: f64) -> Option<f64> {
    // A counter that went backwards was reset (scene change or node
    // recreation); a window under 50 ms is too short to be a rate.
    if frames < 0.0 || elapsed < 0.05 {
        return None;
    }
    let fps = frames / elapsed;
    plausible(fps).then_some(fps)
}

fn reading_from_node(node_path: &str, text: &str) -> Option<FpsReading> {
    let fields = parse_fields(text);
    let fps = match fields.fps {
        Some(fps) => fps,
        None => {
            let bare = fields.bare?;
            if bare <= INSTANT_CEILING {
                bare
            } else {
                counter_rate(node_path, bare)?
            }
        }
    };
    if !plausible(fps) {
        return None;
    }
    Some(FpsReading {
        fps: round2(fps),
        min: fields
            .min
            .filter(|value| plausible(*value))
            .map_or(0.0, round2),
        low1: fields
            .low1
            .filter(|value| plausible(*value))
            .map_or(0.0, round2),
        dropped: fields
            .dropped
            .filter(|value| value.is_finite())
            .map_or(0, |value| value.max(0.0) as u32),
        node: node_path
            .trim_start_matches("/sys/kernel/")
            .trim_start_matches("/proc/")
            .to_string(),
        valid: true,
    })
}

/// The package a process (or thread) belongs to, as Android names it.
fn process_package(pid: u32) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let first = raw.split(|byte| *byte == 0).next().unwrap_or_default();
    let text = std::str::from_utf8(first).ok()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn same_app(process: &str, package: &str) -> bool {
    process == package
        || process
            .strip_prefix(package)
            .is_some_and(|rest| rest.starts_with(':'))
}

/// Whether a row name from the FPS list is the foreground package. The kernel
/// keeps only the tail of a process name, so `android.systemui` arrives as
/// `ndroid.systemui` and `com.inputmethod.latin` as `putmethod.latin`. A row
/// is therefore a suffix of the package. The name must be long enough to be
/// identifying (a tail like `ui` never attributes) and the package must lose
/// at most a few leading characters, so the row stays a plausible truncation
/// rather than a coincidental suffix.
fn name_matches(name: &str, package: &str) -> bool {
    let base = package.split(':').next().unwrap_or(package);
    if name == base || same_app(name, package) {
        return true;
    }
    let lost = base.len().saturating_sub(name.len());
    name.len() >= 8 && lost <= 8 && base.ends_with(name)
}

/// Rows that belong to the system shell rather than to a game. Their rates
/// are not the foreground app's rate.
fn is_system_row(name: &str) -> bool {
    [
        "systemui",
        "launcher",
        "inputmethod",
        "latin",
        "surfaceflinger",
    ]
    .iter()
    .any(|marker| name.contains(marker))
}

/// Whether `cols[2]` holds a process name rather than a buffer id. The node
/// opens with a few rows of `<index>\t<tid>\t<bufID>\t<fps>\t<target>` that
/// carry no process column at all; their `cols[2]` is a hex id and must not
/// be treated as a name.
fn is_named_row(cols: &[&str]) -> bool {
    let name = cols[2].trim();
    !name.is_empty() && !name.starts_with("0x")
}

/// Parse a per-thread FPS table given as text. Each named data row is
/// `<tid>\t<bufID>\t<name>\t<fps>\t...` where `name` is the process. Returns
/// the reading attributed to `foreground`, or the busiest non-system row when
/// the tracker is off or the app is not listed.
fn parse_fps_list(text: &str, foreground: Option<&str>) -> Option<(f64, String)> {
    let node = "fpsgo/fstb/fstb_fps_list".to_string();
    let mut attributed: Option<(f64, String)> = None;
    let mut busiest: Option<(f64, String)> = None;
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        // Data rows have a numeric tid first and at least four columns; the
        // header and the `dfps_ceiling` footer do not. Rows without a process
        // name (the index-prefixed opening block) are not candidates.
        if cols.len() < 4 || cols[0].parse::<u32>().is_err() || !is_named_row(&cols) {
            continue;
        }
        let Ok(fps) = cols[3].trim().parse::<f64>() else {
            continue;
        };
        if !plausible(fps) {
            continue;
        }
        let name = cols[2].trim();
        if let Some(package) = foreground {
            if name_matches(name, package) && attributed.as_ref().is_none_or(|best| fps > best.0) {
                attributed = Some((fps, node.clone()));
            }
        } else if !is_system_row(name) && busiest.as_ref().is_none_or(|best| fps > best.0) {
            busiest = Some((fps, node.clone()));
        }
    }
    if foreground.is_some() {
        // With a known foreground app, a row that is not that app is not
        // evidence about it. Falling back to the busiest row would attribute
        // another app's frame rate to the foreground app.
        return attributed;
    }
    attributed.or(busiest)
}

/// Read the per-thread FPS table from the node and resolve a reading.
fn read_fps_list(foreground: Option<&str>) -> Option<FpsReading> {
    let text = crate::read_node_text(FPS_LIST_NODE).ok()?;
    let (fps, node) = parse_fps_list(&text, foreground)?;
    Some(FpsReading {
        fps: round2(fps),
        // A per-thread rate; this node carries no frame distribution.
        min: 0.0,
        low1: 0.0,
        dropped: 0,
        node,
        valid: true,
    })
}

/// Attribute a render thread to the tracked foreground package when the node
/// name is the thread id, and otherwise fall back to the busiest thread.
fn read_render_threads(foreground: Option<&str>) -> Option<FpsReading> {
    let entries = std::fs::read_dir(RENDER_THREAD_ROOT).ok()?;
    let mut attributed: Option<(f64, String)> = None;
    let mut busiest: Option<(f64, String)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(id) = name.strip_prefix("renderpid_") else {
            continue;
        };
        let path = entry.path().join("fps");
        let path_text = path.to_string_lossy().to_string();
        let Ok(text) = crate::read_node_text(&path) else {
            continue;
        };
        let Some(reading) = reading_from_node(&path_text, &text) else {
            continue;
        };
        let fps = reading.fps;
        let node = format!("{name}/fps");
        let matches_foreground = foreground.is_some_and(|package| {
            id.parse::<u32>()
                .ok()
                .and_then(process_package)
                .is_some_and(|process| same_app(&process, package))
        });
        if matches_foreground {
            if attributed.as_ref().is_none_or(|(best, _)| fps > *best) {
                attributed = Some((fps, node));
            }
        } else if busiest.as_ref().is_none_or(|(best, _)| fps > *best) {
            busiest = Some((fps, node));
        }
    }
    let (fps, node) = attributed.or(busiest)?;
    Some(FpsReading {
        fps,
        // A per-thread node is one rate, not a frame distribution.
        min: 0.0,
        low1: 0.0,
        dropped: 0,
        node,
        valid: true,
    })
}

/// The package currently in front, when the accessibility tracker knows it.
fn foreground_package() -> Option<String> {
    crate::foreground_app::ForegroundAppTracker::global()
        .current_app()
        .filter(|info| info.is_valid())
        .map(|info| info.package_name)
}

/// The fastest online CPU in MHz. The overlay shows one clock, not eight; the
/// prime core is the one that moves when a game starts or throttles.
fn cpu_mhz() -> i32 {
    (0..8)
        .filter_map(|cpu| crate::cpu_freq(cpu).trim().parse::<i32>().ok())
        .max()
        .map(|khz| khz / 1000)
        .unwrap_or(0)
}

/// Battery temperature in tenths of a degree, `0` when the ROM hides it.
fn battery_temp_deci_c() -> i32 {
    crate::battery("temp").trim().parse().unwrap_or(0)
}

/// Reads the foreground FPS once, together with the package it belongs to.
/// Never blocks on the runtime lock and never invents a value when no node
/// answers.
fn read_with_package() -> (FpsReading, Option<String>) {
    let foreground = foreground_package();
    let reading = read_for(foreground.as_deref());
    (reading, foreground)
}

fn read_for(foreground: Option<&str>) -> FpsReading {
    for node in AGGREGATE_NODES {
        if let Ok(text) = crate::read_node_text(node)
            && let Some(reading) = reading_from_node(node, &text)
        {
            return reading;
        }
    }
    read_fps_list(foreground)
        .or_else(|| read_render_threads(foreground))
        .unwrap_or_else(FpsReading::unavailable)
}

/// One snapshot for the overlay and the diagnostics view: the frame reading
/// plus the few sensors a player watches next to it. A single daemon request
/// serves the whole chip, so a 500 ms poll is one round-trip, not six.
///
/// A sensor the ROM does not expose is omitted, not reported as a fake zero,
/// so the consumer can tell "not measured" from "idle". The FPS keys keep
/// their names and meanings, because the host FFI already reads `fps` and
/// `valid`.
pub(super) fn overlay_json() -> serde_json::Value {
    let (reading, package) = read_with_package();
    let mut object = match serde_json::to_value(&reading) {
        Ok(serde_json::Value::Object(object)) => object,
        _ => serde_json::Map::new(),
    };
    object.insert(
        "package".into(),
        package.map_or(serde_json::Value::Null, serde_json::Value::String),
    );
    // Zero is "the node said zero" for the GPU load, but a missing node also
    // reads as zero. The sensors are omitted in that case; the FPS reading
    // itself is never hidden because `valid` already carries that signal.
    for (key, value) in [
        ("gpuUtil", crate::gpu_get_loading()),
        ("gpuMHz", crate::gpu_get_cur_freq_mhz()),
        ("cpuMHz", cpu_mhz()),
        ("tempDeciC", battery_temp_deci_c()),
    ] {
        if value > 0 {
            object.insert(key.into(), serde_json::json!(value));
        }
    }
    serde_json::Value::Object(object)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_key_value_aggregate() {
        let fields = parse_fields("fps = 119, min = 118, low1 = 115, dropped = 3");
        assert_eq!(fields.fps, Some(119.0));
        assert_eq!(fields.min, Some(118.0));
        assert_eq!(fields.low1, Some(115.0));
        assert_eq!(fields.dropped, Some(3.0));
    }

    #[test]
    fn reads_colon_and_space_separators() {
        assert_eq!(parse_fields("FPS: 60").fps, Some(60.0));
        assert_eq!(parse_fields("average_fps 90.5").fps, Some(90.5));
        assert_eq!(parse_fields("cur_fps=144").fps, Some(144.0));
    }

    #[test]
    fn keeps_a_lone_number_as_a_candidate() {
        let fields = parse_fields("120\n");
        assert_eq!(fields.fps, None);
        assert_eq!(fields.bare, Some(120.0));
    }

    #[test]
    fn rejects_implausible_rates() {
        assert!(reading_from_node("/test", "fps=0").is_some());
        assert!(reading_from_node("/test", "fps=1001").is_none());
        assert!(reading_from_node("/test", "fps=-1").is_none());
        assert!(reading_from_node("/test", "fps=nan").is_none());
    }

    #[test]
    fn computes_a_rate_only_from_two_counter_samples() {
        assert_eq!(rate_from_delta(30.0, 0.5), Some(60.0));
        assert_eq!(rate_from_delta(-5.0, 0.5), None);
        assert_eq!(rate_from_delta(5.0, 0.01), None);
    }

    #[test]
    fn labels_the_node_that_answered() {
        let reading = reading_from_node("/sys/kernel/fpsgo/fps_info", "fps=120").unwrap();
        assert_eq!(reading.node, "fpsgo/fps_info");
        assert_eq!(reading.fps, 120.0);
        assert!(reading.valid);
    }

    #[test]
    fn reports_unavailable_instead_of_guessing() {
        // On a host with no vendor nodes this must stay honest; on a device
        // with FPSGO it must stay inside a real refresh range.
        let reading = read_with_package().0;
        if reading.valid {
            assert!(reading.fps > 0.0 && reading.fps <= INSTANT_CEILING);
            assert_ne!(reading.node, "unavailable");
        } else {
            assert_eq!(reading.node, "unavailable");
            assert_eq!(reading.fps, 0.0);
        }
    }

    // Exact mt6991 (Dimensity 9400) output from `/sys/kernel/fpsgo/fstb/fstb_fps_list`.
    // The first 4 rows carry `<index>\t<tid>\t<bufID>\t<fps>\t<target>` with no
    // name column; `is_named_row` must skip them because cols[2] is a hex id.
    const MT6991_LIST: &str = "\
1\t3345\t0xbc000000003\t120\t0
2\t3345\t0xbc000000001\t120\t0
3\t18516\t0x390f00000068\t3\t0
4\t5713\t0x129d00000325\t17\t0
tid\tbufID\tname\tcurrentFPS\ttargetFPS\tFPS_margin\tsbe_state\tHWUI\tt_gpu\tpolicy
3345\t0xbc000000003\tndroid.systemui\t120\t120\t0\t0\t1\t-1\t(0,0,1)
3345\t0xbc000000001\tndroid.systemui\t120\t120\t0\t0\t1\t-1\t(0,0,1)
18516\t0x390f00000068\tcom.termux\t3\t3\t0\t0\t1\t-1\t(0,0,1)
5713\t0x129d00000325\tputmethod.latin\t17\t17\t0\t0\t1\t-1\t(0,0,1)
dfps_ceiling:120";

    #[test]
    fn fps_list_skips_the_hex_prefixed_block() {
        // The first 4 rows have no name column; cols[2] is a hex bufID, not a
        // process name. Skip them, leaving only com.termux at 3 fps.
        let (fps, _) = parse_fps_list(MT6991_LIST, None).expect("a reading");
        assert_eq!(fps, 3.0);
    }

    #[test]
    fn is_named_row_rejects_hex_bufid() {
        let hex: Vec<&str> = "3\t18516\t0x390f00000068\t3\t0".split('\t').collect();
        assert!(!is_named_row(&hex));
        let named: Vec<&str> = "18516\t0x39\tcom.termux\t3\t3".split('\t').collect();
        assert!(is_named_row(&named));
    }

    #[test]
    fn fps_list_attributes_the_foreground_row() {
        let (fps, node) = parse_fps_list(MT6991_LIST, Some("com.termux")).expect("a reading");
        assert_eq!(fps, 3.0);
        assert_eq!(node, "fpsgo/fstb/fstb_fps_list");
    }

    #[test]
    fn fps_list_matches_a_truncated_name() {
        // The kernel keeps fifteen characters: `android.systemui` is truncated
        // to `ndroid.systemui` and `com.inputmethod.latin` to
        // `putmethod.latin`.
        let (fps, _) = parse_fps_list(MT6991_LIST, Some("android.systemui")).expect("a reading");
        assert_eq!(fps, 120.0);
        let (fps, _) =
            parse_fps_list(MT6991_LIST, Some("com.inputmethod.latin")).expect("a reading");
        assert_eq!(fps, 17.0);
    }

    #[test]
    fn fps_list_falls_back_to_the_busiest_game_row_only_without_a_foreground() {
        // No foreground match: the system rows are ignored, so the game row is
        // the only candidate.
        let (fps, _) = parse_fps_list(MT6991_LIST, None).expect("a reading");
        assert_eq!(fps, 3.0);
    }

    #[test]
    fn fps_list_never_attributes_another_app_rate_to_the_foreground() {
        // The foreground app is not in the list (it spawned no window). A row
        // belonging to another app must not be reported as the foreground
        // rate, so the answer is none, not 120.
        assert!(parse_fps_list(MT6991_LIST, Some("com.some.game")).is_none());
    }

    #[test]
    fn fps_list_ignores_a_generic_suffix_tail() {
        // `ui` is a plausible row name but not a plausible truncation of the
        // package `com.myapp`; nothing may match.
        assert!(parse_fps_list("18516\t0x39\tui\t90\t90\t0\t0\t1\t-1\t(0,0,1)", None,).is_some());
        assert!(
            parse_fps_list(
                "18516\t0x39\tui\t90\t90\t0\t0\t1\t-1\t(0,0,1)",
                Some("com.myapp"),
            )
            .is_none()
        );
        // A long tail that is not the app's own name must not match either.
        assert!(
            parse_fps_list(
                "5713\t0x12\tinputmethod.latin\t90\t90\t0\t0\t1\t-1\t(0,0,1)",
                Some("com.other.app"),
            )
            .is_none()
        );
    }
}
