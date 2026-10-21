//! Bounded diagnostic history for the daemon.
//!
//! The daemon always knew why a control did not take effect, but that
//! knowledge only reached a logcat line that scrolled away within seconds.
//! A user who cannot tell which feature is broken has nothing to act on, so
//! the daemon keeps a small structured history that the application can read
//! and show.
//!
//! The log is deliberately cheap and bounded. Recording is a lock, a
//! timestamp and a bounded copy of the message; nothing here is allowed to
//! block a hardware transaction, and no event may grow without limit.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// The number of events kept. A session's worth of transitions fits; the
/// buffer never grows with the number of guard ticks because the guard only
/// records when it actually changes something.
const MAX_EVENTS: usize = 400;

/// The longest message a single event may carry.
const MAX_DETAIL: usize = 600;

/// An area groups events by the feature they describe.
pub const AREA_GPU: &str = "gpu";
pub const AREA_PER_APP: &str = "per-app";
pub const AREA_TOUCH: &str = "touch";
pub const AREA_DISPLAY: &str = "display";
pub const AREA_CHARGING: &str = "charging";
pub const AREA_CPU: &str = "cpu";
pub const AREA_SERVICE: &str = "service";
pub const AREA_CONFLICT: &str = "conflict";

#[derive(Clone)]
pub struct Event {
    pub seq: u64,
    pub at_ms: u64,
    pub area: &'static str,
    pub level: &'static str,
    pub what: String,
    pub detail: String,
}

/// A bounded, ordered history of events.
#[derive(Default)]
pub struct Log {
    events: Mutex<Vec<Event>>,
    sequence: AtomicU64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn bound(value: &str) -> String {
    let mut text = value.replace(['\n', '\r'], " ");
    if text.chars().count() > MAX_DETAIL {
        text = text.chars().take(MAX_DETAIL).collect::<String>() + "…";
    }
    text
}

impl Log {
    /// Record an event. The level is one of `info`, `warn` or `error`.
    pub fn record(&self, area: &'static str, level: &'static str, what: &str, detail: &str) {
        let mut log = match self.events.lock() {
            Ok(log) => log,
            Err(poisoned) => poisoned.into_inner(),
        };

        // The sequence number is taken under the same lock as the append, so
        // the history is always ordered by the sequence it reports. A reader
        // can therefore rely on the order without sorting.
        let seq = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        log.push(Event {
            seq,
            at_ms: now_ms(),
            area,
            level,
            what: bound(what),
            detail: bound(detail),
        });

        if log.len() > MAX_EVENTS {
            let excess = log.len() - MAX_EVENTS;
            log.drain(0..excess);
        }
    }

    /// The recorded events, oldest first.
    pub fn history(&self) -> Vec<Event> {
        match self.events.lock() {
            Ok(log) => log.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// The most recent events, newest first, at most `limit` of them.
    pub fn recent(&self, limit: usize) -> Vec<Event> {
        let mut history = self.history();
        history.reverse();
        history.truncate(limit);
        history
    }

    /// Forget the recorded history.
    pub fn clear(&self) {
        let mut log = match self.events.lock() {
            Ok(log) => log,
            Err(poisoned) => poisoned.into_inner(),
        };
        log.clear();
    }
}

static LOG: OnceLock<Log> = OnceLock::new();

fn log() -> &'static Log {
    LOG.get_or_init(Log::default)
}

/// Record an event in the daemon's history.
pub fn record(area: &'static str, level: &'static str, what: &str, detail: &str) {
    log().record(area, level, what, detail);
}

/// The recorded events, oldest first.
pub fn history() -> Vec<Event> {
    log().history()
}

/// The most recent events, newest first.
pub fn recent(limit: usize) -> Vec<Event> {
    log().recent(limit)
}

/// Forget the recorded history.
pub fn clear() {
    log().clear();
}

/// The effective limits the vendor's own services are applying to the GPU.
///
/// The MediaTek power service and the thermal service both publish a ceiling
/// and a floor over the GPU's operating-point ladder. When one of them holds
/// the GPU below a requested target, the target is not unreachable because
/// the request was wrong, and the difference has to be visible to the user.
#[derive(Clone, Copy, Debug, Default)]
pub struct VendorLimits {
    /// The slowest index the vendor still allows, if it published one.
    pub ceiling_index: Option<i32>,
    /// The service that set the ceiling.
    pub ceiling_limiter: Option<&'static str>,
    /// The fastest index the vendor still allows.
    pub floor_index: Option<i32>,
    /// The service that set the floor.
    pub floor_limiter: Option<&'static str>,
}

impl VendorLimits {
    /// The name of the service that set the ceiling, if any.
    pub fn ceiling_name(&self) -> &str {
        self.ceiling_limiter.unwrap_or("unknown")
    }

    /// The name of the service that set the floor, if any.
    pub fn floor_name(&self) -> &str {
        self.floor_limiter.unwrap_or("unknown")
    }
}

/// The effective GPU ceiling the thermal framework publishes, in hertz.
///
/// The driver exposes the same ceiling twice: as an index in the PPM status
/// dump, and directly in hertz here. The second form is simpler and does not
/// need the status dump to be read whole, so it is preferred.
///
/// The file reads `<temp_mC>,<ceiling_hz>,<floor_hz>`. A ceiling below the
/// lowest operating point is the deep-idle marker rather than a real cap.
pub fn gpu_thermal_ceiling_hz(lowest_hz: u64) -> Option<u64> {
    let raw = std::fs::read_to_string("/sys/kernel/thermal/gpu_info").ok()?;
    let mut parts = raw.trim().split(',');
    let _temperature = parts.next()?;
    let ceiling = parts.next()?.trim().parse::<u64>().ok()?;
    if ceiling == 0 || ceiling < lowest_hz {
        return None;
    }
    Some(ceiling)
}

fn parse_limit_table(text: &str) -> Vec<(i32, &'static str)> {
    let mut names = Vec::new();
    for line in text.lines() {
        // The table is column aligned by hand, so the identifier column is
        // one space for a single digit and two for a double digit. Parsing on
        // whitespace rather than on a fixed prefix keeps the two digit
        // limiters, which include POWERHAL, from being dropped.
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let mut fields = trimmed.split_whitespace();
        let Some(id) = fields.next().and_then(|v| v.parse::<i32>().ok()) else {
            continue;
        };
        let Some(name) = fields.next() else {
            continue;
        };
        // The table's names are a fixed vocabulary of MediaTek limiters.
        let name: &'static str = match name {
            "SEGMENT" => "SEGMENT",
            "DEBUG" => "DEBUG",
            "GPM_3.0" => "GPM_3.0",
            "PEAK_POWER" => "PEAK_POWER",
            "THERMAL_AP" => "THERMAL_AP",
            "THERMAL_EB" => "THERMAL_EB",
            "SRAMRC" => "SRAMRC",
            "BATT_OC" => "BATT_OC",
            "BATT_PERCENT" => "BATT_PERCENT",
            "LOW_BATT" => "LOW_BATT",
            "PBM" => "PBM",
            "APIBOOST" => "APIBOOST",
            "POWERHAL" => "POWERHAL",
            "FIXCMD" => "FIXCMD",
            _ => continue,
        };
        names.push((id, name));
    }
    names
}

fn parse_limit_index(line: &str, key: &str) -> Option<i32> {
    let rest = line.split_once(key)?.1;
    let value = rest.split_once("LimitIndex:")?.1;
    value
        .split(',')
        .next()?
        .split_whitespace()
        .next()?
        .parse::<i32>()
        .ok()
}

fn parse_limiter_id(line: &str) -> Option<i32> {
    let rest = line.split_once("Limiter:")?.1;
    rest.split(',')
        .next()?
        .split_whitespace()
        .next()?
        .parse::<i32>()
        .ok()
}

/// Read the effective GPU limits the vendor's services are applying.
pub fn gpu_vendor_limits() -> VendorLimits {
    let mut limits = VendorLimits::default();

    let Ok(status) = std::fs::read_to_string("/proc/gpufreqv2/gpufreq_status") else {
        return limits;
    };
    let Ok(table) = std::fs::read_to_string("/proc/gpufreqv2/limit_table") else {
        return limits;
    };
    let names = parse_limit_table(&table);

    for line in status.lines() {
        let trimmed = line.trim();
        if let Some(index) = parse_limit_index(trimmed, "[PPM Ceiling]") {
            limits.ceiling_index = Some(index);
            let id = parse_limiter_id(trimmed);
            limits.ceiling_limiter = id.and_then(|id| {
                names
                    .iter()
                    .find(|(candidate, _)| *candidate == id)
                    .map(|(_, name)| *name)
            });
        } else if let Some(index) = parse_limit_index(trimmed, "[PPM Floor]") {
            limits.floor_index = Some(index);
            let id = parse_limiter_id(trimmed);
            limits.floor_limiter = id.and_then(|id| {
                names
                    .iter()
                    .find(|(candidate, _)| *candidate == id)
                    .map(|(_, name)| *name)
            });
        }
    }

    limits
}

/// What the platform's own game services are doing to the hardware.
///
/// MediaTek's game driver and Xiaomi's task turbo both act on the same GPU
/// and CPU controls the daemon owns. A game in the foreground therefore
/// competes with a per-app profile at exactly the moment the per-app profile
/// is applied, and the conflict has to be visible rather than inferred.
#[derive(Clone, Copy, Debug, Default)]
pub struct GameServices {
    /// The GPU ceiling the game driver published, in kHz.
    pub gpu_max_limit_khz: Option<u64>,
    /// The GPU floor the game driver published, in kHz.
    pub gpu_min_limit_khz: Option<u64>,
    /// The frame rate the game driver is driving towards.
    pub expected_fps: Option<i32>,
    /// The process the task turbo is currently boosting.
    pub turbo_pid: Option<i32>,
    /// How many tasks the scheduler interface has configured.
    pub loom_tasks: Option<usize>,
}

impl GameServices {
    /// Whether any game service is currently holding a control.
    pub fn active(&self) -> bool {
        self.gpu_max_limit_khz.is_some_and(|v| v > 0)
            || self.gpu_min_limit_khz.is_some_and(|v| v > 0)
            || self.expected_fps.is_some_and(|v| v > 0)
            || self.turbo_pid.is_some_and(|v| v > 0)
            || self.loom_tasks.is_some_and(|v| v > 0)
    }

    /// A short description of what is being held.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if let Some(limit) = self.gpu_max_limit_khz {
            parts.push(format!("gpu_max={limit}kHz"));
        }
        if let Some(limit) = self.gpu_min_limit_khz {
            parts.push(format!("gpu_min={limit}kHz"));
        }
        if let Some(fps) = self.expected_fps {
            parts.push(format!("expected_fps={fps}"));
        }
        if let Some(pid) = self.turbo_pid {
            parts.push(format!("turbo_pid={pid}"));
        }
        if let Some(tasks) = self.loom_tasks {
            parts.push(format!("scheduler_tasks={tasks}"));
        }
        if parts.is_empty() {
            "idle".to_string()
        } else {
            parts.join(" ")
        }
    }
}

fn read_khz(path: &str) -> Option<u64> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|value| value.trim().parse().ok())
}

fn read_int(path: &str) -> Option<i32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|value| value.trim().parse().ok())
}

/// Read what the platform's game services are currently holding.
pub fn game_services() -> GameServices {
    let loom_tasks = std::fs::read_to_string("/sys/kernel/game/loom_task_cfg")
        .ok()
        .and_then(|raw| {
            raw.split("loom_task_cfg_list_length:")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|value| value.parse().ok())
        });
    GameServices {
        gpu_max_limit_khz: read_khz("/sys/module/mtk_game/parameters/g_limit_max_freq"),
        gpu_min_limit_khz: read_khz("/sys/module/mtk_game/parameters/g_limit_min_freq"),
        expected_fps: read_int("/sys/module/mtk_game/parameters/g_expected_fps"),
        turbo_pid: read_int("/sys/module/task_turbo/parameters/turbo_pid"),
        loom_tasks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_bounded_and_ordered() {
        let log = Log::default();
        for index in 0..MAX_EVENTS + 50 {
            log.record(AREA_GPU, "info", "test", &format!("event {index}"));
        }
        let history = log.history();
        assert_eq!(history.len(), MAX_EVENTS);
        assert_eq!(history[0].seq, 51);
        assert!(history.windows(2).all(|w| w[0].seq < w[1].seq));
    }

    #[test]
    fn messages_are_bounded() {
        let log = Log::default();
        let long = "x".repeat(MAX_DETAIL * 2);
        log.record(AREA_CPU, "warn", "long", &long);
        let event = log.history().pop().expect("event");
        assert!(event.detail.chars().count() <= MAX_DETAIL + 1);
    }

    #[test]
    fn newlines_do_not_break_a_single_line_event() {
        let log = Log::default();
        log.record(AREA_TOUCH, "error", "multi", "first\nsecond");
        let event = log.history().pop().expect("event");
        assert!(!event.detail.contains('\n'));
    }

    #[test]
    fn the_recent_events_come_back_newest_first() {
        let log = Log::default();
        for index in 0..5 {
            log.record(AREA_GPU, "info", "test", &format!("event {index}"));
        }
        let recent = log.recent(2);
        assert_eq!(recent.len(), 2);
        assert!(recent[0].seq > recent[1].seq);
    }

    #[test]
    fn the_vendor_limit_table_is_parsed_by_name() {
        let table = "\
[id]          [name]  [priority]  [ceiling]  [floor]  [c_enable]  [f_enable]
   0         SEGMENT           9          0       51           1           1
   5      THERMAL_EB           6          7       -1           1           1
  12         POWERHAL           2          0        0           1           1
";
        let names = parse_limit_table(table);
        assert_eq!(names.len(), 3);
        assert_eq!(names[0], (0, "SEGMENT"));
        assert_eq!(names[1], (5, "THERMAL_EB"));
        // The two digit rows share the table with the one digit rows.
        assert_eq!(names[2], (12, "POWERHAL"));
    }

    #[test]
    fn the_effective_ceiling_is_read_with_its_limiter() {
        let status = "\
[GPU   OPP]      Index:  7, Freq: 1430000, Volt:  78125, Vsram:  78125
[PPM Ceiling]    LimitIndex:  7, Limiter:  5, Priority:  6
[PPM Floor]      LimitIndex:  7, Limiter: 12, Priority:  2
";
        assert_eq!(parse_limit_index(status, "[PPM Ceiling]"), Some(7));
        assert_eq!(parse_limiter_id(status), Some(5));
    }

    #[test]
    fn a_limit_for_another_key_is_not_read_as_a_ceiling() {
        assert_eq!(
            parse_limit_index("[PPM Ceiling] LimitIndex: 7", "[PPM Floor]"),
            None
        );
    }
}
