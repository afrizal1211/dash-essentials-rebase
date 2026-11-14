//! Scheduled background dexopt, ported from RedFox Prism's DexOpt scheduler.
//!
//! The Android runtime already re-optimises applications when it decides to.
//! A vendor ROM that disables its own background dexopt leaves every app
//! running from interpreted bytecode after an update, so the schedule here
//! asks the package manager to do it again on a cadence the user picks, while
//! the screen is off and the device is charging, and only for the packages
//! the user did not opt out of.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// /sys/class/power_supply/usb/online reads 1 while a charger is attached.
const CHARGER_ATTACHED: &str = "1";
/// The minimum gap between two runs, so a schedule never runs twice a day.
const DAY_SECS: u64 = 24 * 60 * 60;
/// Compile is slow: a large update set can take minutes per application.
const COMPILE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// The loop polls the schedule; it never sleeps through a day boundary.
const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);

static CONFIG: OnceLock<Mutex<Schedule>> = OnceLock::new();

/// How often the optimisation runs, and which day it runs on.
#[derive(Clone, Debug, Default)]
struct Schedule {
    cadence: String,
    day: u32,
    opt_out: Vec<String>,
    last_run: u64,
    last_error: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The weekday and day of month of today, from the device's wall clock.
///
/// Unix time has no calendar, and the user picks a weekday, so the days since
/// 1970 are converted with the civil calendar algorithm. A device whose clock
/// is wrong by more than a day merely shifts the schedule, which is safe.
fn calendar_from(seconds: u64) -> Option<(u32, u32)> {
    let days = (seconds / 86_400) as i64;
    let weekday = ((days + 4).rem_euclid(7)) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let day_of_year = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month + 2) / 5 + 1) as u32;
    let _ = year;
    Some((weekday, day))
}

fn path() -> PathBuf {
    crate::state_dir().join("dexopt.json")
}

fn load() -> Schedule {
    let Ok(raw) = fs::read_to_string(path()) else {
        return Schedule::default();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Schedule::default();
    };
    Schedule {
        cadence: value["cadence"].as_str().unwrap_or_default().to_owned(),
        day: value["day"].as_u64().unwrap_or_default() as u32,
        opt_out: value["opt_out"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        last_run: value["last_run"].as_u64().unwrap_or_default(),
        last_error: value["last_error"].as_str().unwrap_or_default().to_owned(),
    }
}

fn save(schedule: &Schedule) -> Result<(), String> {
    let raw = serde_json::json!({
        "cadence": schedule.cadence,
        "day": schedule.day,
        "opt_out": schedule.opt_out,
        "last_run": schedule.last_run,
        "last_error": schedule.last_error,
    });
    let dir = crate::state_dir();
    if !dir.exists() {
        return Err("state directory missing".into());
    }
    let text = serde_json::to_string(&raw).map_err(|e| e.to_string())?;
    // Write to a sibling temp file, then rename, so a fault never leaves a
    // truncated schedule in place of a good one.
    let tmp = dir.join("dexopt.json.tmp");
    let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Run the closure with the schedule held, keeping the lock short.
fn with_schedule<R>(action: impl FnOnce(&mut Schedule) -> R) -> Result<R, String> {
    let mut guard = CONFIG
        .get_or_init(|| Mutex::new(load()))
        .lock()
        .map_err(|_| "schedule lock poisoned")?;
    Ok(action(&mut guard))
}

/// Whether today is a scheduled day, and the run has not happened yet today.
fn due_from(schedule: &Schedule, seconds: u64) -> bool {
    let Some((weekday, day_of_month)) = calendar_from(seconds) else {
        return false;
    };
    if schedule.cadence.is_empty() || seconds.saturating_sub(schedule.last_run) < DAY_SECS {
        return false;
    }
    match schedule.cadence.as_str() {
        // Weekly: the weekday the user picked.
        "weekly" => weekday == schedule.day,
        // Biweekly: the same weekday on every other week. Week parity is
        // derived from the day count, so it needs no week-number source.
        "biweekly" => weekday == schedule.day && (seconds / 86_400 / 7).is_multiple_of(2),
        // Monthly: a day of the month the user picked, clamped to 28 so the
        // short months never silently skip.
        "monthly" => day_of_month == schedule.day.clamp(1, 28),
        _ => false,
    }
}

/// Whether today is a scheduled day, and the run has not happened yet today.
fn due(schedule: &Schedule) -> bool {
    due_from(schedule, now())
}

/// Every installed package the user did not opt out of.
fn compile_list(schedule: &Schedule) -> Result<Vec<String>, String> {
    let output = crate::run_process_with_timeout(
        "/system/bin/pm",
        &["list", "packages"],
        Duration::from_secs(20),
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        return Err(format!("pm list packages rejected: {}", stdout.trim()));
    }
    Ok(stdout
        .lines()
        .filter_map(|line| line.strip_prefix("package:").map(str::trim))
        .filter(|package| !schedule.opt_out.iter().any(|out| out == *package))
        .map(str::to_owned)
        .collect())
}

/// Ask the package manager to re-optimise the selected applications.
///
/// bg-dexopt is the background entry point: it compiles with the profiles the
/// runtime collected instead of forcing a full speed compile, and it takes the
/// package names as arguments so the opt-out list is honoured by the caller.
fn run_now(schedule: &Schedule) -> Result<u64, String> {
    let packages = compile_list(schedule)?;
    if packages.is_empty() {
        return Ok(0);
    }
    let mut args: Vec<&str> = vec!["package", "bg-dexopt"];
    args.extend(packages.iter().map(String::as_str));
    let output = crate::run_process_with_timeout("/system/bin/cmd", &args, COMPILE_TIMEOUT)?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() || stderr.contains("Exception") {
        return Err(format!("bg-dexopt rejected: {}", stderr.trim()));
    }
    Ok(packages.len() as u64)
}

/// The schedule the app reads and edits.
pub(super) fn status_json() -> Result<serde_json::Value, String> {
    with_schedule(|schedule| {
        serde_json::json!({
            "cadence": schedule.cadence,
            "day": schedule.day,
            "opt_out": schedule.opt_out,
            "last_run": schedule.last_run,
            "last_error": schedule.last_error,
            "next_due": due(schedule),
        })
    })
}

/// Store a schedule the app sent.
pub(super) fn set_schedule(raw: &str) -> Result<(), String> {
    if raw.len() > 32_768 {
        return Err("dexopt schedule too large".into());
    }
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("dexopt JSON: {e}"))?;
    let cadence = value["cadence"].as_str().unwrap_or_default();
    if !matches!(cadence, "" | "weekly" | "biweekly" | "monthly") {
        return Err("unknown dexopt cadence".into());
    }
    let day = value["day"].as_u64().unwrap_or_default() as u32;
    let max_day = if cadence == "monthly" { 28 } else { 6 };
    if day > max_day {
        return Err("dexopt day out of range".into());
    }
    let opt_out = value["opt_out"]
        .as_array()
        .ok_or("missing opt_out array")?
        .iter()
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if opt_out.len() > 512 {
        return Err("too many opt-out packages".into());
    }
    with_schedule(|schedule| {
        schedule.cadence = cadence.to_owned();
        schedule.day = day;
        schedule.opt_out = opt_out;
    })?;
    let schedule = with_schedule(|schedule| schedule.clone())?;
    save(&schedule)
}

/// The worker loop, spawned once at daemon start.
pub(super) fn start() {
    loop {
        std::thread::sleep(POLL_INTERVAL);
        let Some(_gate) = crate::service_control::GATE.read().ok() else {
            continue;
        };
        if !crate::service_control::active() {
            continue;
        }
        // dexopt competes with whatever the foreground is doing, so it waits
        // for a dark screen on a charger, the same conditions the platform's
        // own background optimiser waits for.
        if crate::screen_is_on() != Some(false) || crate::usb("online") != CHARGER_ATTACHED {
            continue;
        }
        let mut ran = None;
        let outcome = with_schedule(|schedule| -> Result<(), String> {
            if !due(schedule) {
                return Ok(());
            }
            match run_now(schedule) {
                Ok(count) => {
                    schedule.last_run = now();
                    schedule.last_error.clear();
                    ran = Some(count);
                    Ok(())
                }
                Err(error) => {
                    schedule.last_error = error.clone();
                    Err(error)
                }
            }
        });
        let outcome = outcome.and_then(|result| result);
        match outcome {
            Ok(()) => {
                if let Some(count) = ran {
                    crate::diagnostics::record(
                        crate::diagnostics::AREA_SERVICE,
                        "info",
                        "dexopt.ran",
                        &format!("{count} packages"),
                    );
                }
            }
            Err(error) => {
                crate::diagnostics::record(
                    crate::diagnostics::AREA_SERVICE,
                    "warn",
                    "dexopt.failed",
                    &error,
                );
                let _ = with_schedule(|schedule| save(schedule));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1970-01-01 and 2026-01-01 were Thursdays.
    #[test]
    fn the_civil_conversion_knows_the_weekday_and_day_of_month() {
        assert_eq!(calendar_from(0), Some((4, 1)));
        // 2025-01-01, a Wednesday.
        assert_eq!(calendar_from(1_735_689_600), Some((3, 1)));
        // 2026-01-01, again a Thursday.
        assert_eq!(calendar_from(1_767_225_600), Some((4, 1)));
        // 2030-01-01, a Tuesday.
        assert_eq!(calendar_from(1_893_456_000), Some((2, 1)));
    }

    #[test]
    fn a_schedule_is_due_on_its_weekday_once_a_day() {
        // 2026-01-01, Thursday (weekday 4).
        let thursday = 1_767_225_600;
        let mut schedule = Schedule {
            cadence: "weekly".into(),
            day: 4,
            ..Default::default()
        };
        assert!(due_from(&schedule, thursday));
        // A Friday schedule is not due on a Thursday.
        schedule.day = 5;
        assert!(!due_from(&schedule, thursday));
        // The same day, after a run, is not due again.
        schedule.day = 4;
        schedule.last_run = thursday;
        assert!(!due_from(&schedule, thursday));
        // A week later it is.
        assert!(due_from(&schedule, thursday + 7 * 86_400));
    }

    #[test]
    fn biweekly_skips_every_other_week_and_monthly_honours_the_day() {
        let thursday = 1_767_225_600;
        let biweekly = Schedule {
            cadence: "biweekly".into(),
            day: 4,
            ..Default::default()
        };
        // Exactly one of two consecutive matching Thursdays is due: week
        // parity is what separates them.
        let pair = (
            due_from(&biweekly, thursday),
            due_from(&biweekly, thursday + 7 * 86_400),
        );
        assert!(pair != (false, false) && pair != (true, true));
        // The fortnight after repeats the first verdict.
        let next = thursday + 14 * 86_400;
        assert_eq!(due_from(&biweekly, next), due_from(&biweekly, thursday));
        let monthly = Schedule {
            cadence: "monthly".into(),
            day: 17,
            ..Default::default()
        };
        let seventeenth = thursday + 16 * 86_400;
        let (_, day) = calendar_from(seventeenth).unwrap();
        assert_eq!(day, 17);
        assert!(due_from(&monthly, seventeenth));
        // Day 0 clamps to the first of the month.
        let clamped = Schedule {
            cadence: "monthly".into(),
            day: 0,
            ..Default::default()
        };
        assert!(due_from(&clamped, thursday));
    }

    #[test]
    fn an_unconfigured_or_unknown_schedule_is_never_due() {
        assert!(!due_from(&Schedule::default(), 1_767_225_600));
        let unknown = Schedule {
            cadence: "hourly".into(),
            day: 0,
            ..Default::default()
        };
        assert!(!due_from(&unknown, 1_767_225_600));
    }
}
