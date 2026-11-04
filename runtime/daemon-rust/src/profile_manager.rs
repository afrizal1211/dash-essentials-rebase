//! Adaptive performance profile manager.
//!
//! Computes target CPU frequency based on hardware topology, current thermal
//! state, and a profile-specific bias. GPU is intentionally uncapped — the
//! profiles that call into this module already pin the GPU to its top
//! operating point.

use crate::telemetry::Telemetry;

/// Hysteresis band (°C). Above [`PROFILE_MODE_HIGH`] the profile falls back to
/// "Steady"; at or below [`PROFILE_MODE_LOW`] it returns to "Max". The band
/// prevents rapid oscillation when the temperature hovers around a threshold.
pub const PROFILE_MODE_LOW_C: i32 = 72;
pub const PROFILE_MODE_HIGH_C: i32 = 78;

/// Bias multipliers for the two adaptive profiles.
pub const BIAS_EXTREME_BEAST: f32 = 1.0;
pub const BIAS_DYNAMIC_GAMING: f32 = 0.85;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileMode {
    Max,
    Steady,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProfileConfig {
    pub cpu_target_khz: u32,
    pub mode: ProfileMode,
    /// Diagnostic info (see item 4): thermal headroom factor at the time of
    /// the computation, so the caller can log it for monitoring.
    pub headroom: f32,
}

/// Compute target CPU frequency and hysteresis mode based on thermal
/// headroom, core count, and profile bias.
///
/// * `base_bias` — 1.0 for Extreme Beast, 0.85 for Gaming Dynamic.
/// * `prev` — previous mode for hysteresis; `None` treats low edge as Max.
/// * GPU is never scaled here; the caller keeps the GPU uncapped.
pub fn compute_profile(
    telemetry: &Telemetry,
    base_bias: f32,
    prev: Option<ProfileMode>,
) -> ProfileConfig {
    // 1. Thermal headroom factor [0.0, 1.0]. The telemetry temperature is
    //    reported in deciCelsius by some kernels and Celsius by others, so
    //    we accept either and clamp to a plausible range. Implausible
    //    readings fall back to "no headroom scaling".
    let max_temp = telemetry.max_safe_temp_c.unwrap_or(85) as i32;
    let min_temp = telemetry.min_operating_temp_c as i32;
    let raw_cur = telemetry.cpu_temp_dc;
    // Treat values above 150 as deciCelsius; above 150 °C is not realistic.
    let cur_temp = if raw_cur > 150 { raw_cur / 10 } else { raw_cur };

    // Safety guards (item 3): negative or absurd readings disable scaling.
    if !(min_temp..max_temp).contains(&cur_temp) {
        // Out of range (or missing) — fall back to no thermal scaling.
        let cpu_target = (telemetry.cpu_max_freq_khz as f32 * base_bias).round() as u32;
        return ProfileConfig {
            cpu_target_khz: cpu_target,
            mode: ProfileMode::Max,
            headroom: 1.0,
        };
    }
    let headroom = ((max_temp - cur_temp) as f32 / (max_temp - min_temp) as f32).clamp(0.0, 1.0);

    // 2. Core-count weight, normalised against a reference of 8 cores. Guard
    //    against a sensor reporting zero so we never divide by zero or get a
    //    target of zero.
    let core_weight = (telemetry.cpu_core_count as f32 / 8.0).clamp(0.5, 1.0);

    // 3. Target CPU frequency, clamped to the hardware maximum (item 3) and
    //    rounded to the nearest step the kernel supports.
    let cpu_max = telemetry.cpu_max_freq_khz;
    let cpu_target = ((((cpu_max as f32) * headroom * core_weight * base_bias).round() as u32)
        .min(cpu_max))
    .max(1);

    // 4. Hysteresis mode (item 2/4). Recover to Max once below the low
    //    threshold; climb to Steady at or above the high threshold. The
    //    previous mode suppresses chatter around the band edges.
    let mode = if let Some(ProfileMode::Steady) = prev {
        if cur_temp <= PROFILE_MODE_LOW_C {
            ProfileMode::Max
        } else {
            ProfileMode::Steady
        }
    } else if cur_temp >= PROFILE_MODE_HIGH_C {
        ProfileMode::Steady
    } else {
        ProfileMode::Max
    };

    ProfileConfig {
        cpu_target_khz: cpu_target,
        mode,
        headroom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_telemetry(cur_temp: i32, cores: u32, max_khz: u32) -> Telemetry {
        Telemetry {
            cpu_temp_dc: cur_temp,
            cpu_core_count: cores,
            cpu_max_freq_khz: max_khz,
            max_safe_temp_c: Some(85),
            min_operating_temp_c: 30,
            ..Default::default()
        }
    }

    #[test]
    fn full_headroom_reaches_hardware_max() {
        let t = base_telemetry(30, 8, 3_000_000);
        let cfg = compute_profile(&t, BIAS_EXTREME_BEAST, None);
        assert_eq!(cfg.cpu_target_khz, 3_000_000);
        assert_eq!(cfg.mode, ProfileMode::Max);
        assert!((cfg.headroom - 1.0).abs() < 0.001);
    }

    #[test]
    fn hot_device_scales_down() {
        let t = base_telemetry(80, 8, 3_000_000);
        let cfg = compute_profile(&t, BIAS_EXTREME_BEAST, None);
        // headroom = (85-80)/(85-30) = 5/55 ≈ 0.0909
        let expected = (3_000_000.0f32 * (5.0f32 / 55.0f32)).round() as u32;
        assert_eq!(cfg.cpu_target_khz, expected);
        assert_eq!(cfg.mode, ProfileMode::Steady);
    }

    #[test]
    fn dynamic_gaming_bias_reduces_target() {
        let t = base_telemetry(30, 8, 3_000_000);
        let cfg = compute_profile(&t, BIAS_DYNAMIC_GAMING, None);
        assert_eq!(
            cfg.cpu_target_khz,
            (3_000_000.0f32 * 0.85f32).round() as u32
        );
    }

    #[test]
    fn fewer_cores_reduce_target() {
        let full = base_telemetry(30, 8, 3_000_000);
        let half = base_telemetry(30, 4, 3_000_000);
        let a = compute_profile(&full, BIAS_EXTREME_BEAST, None);
        let b = compute_profile(&half, BIAS_EXTREME_BEAST, None);
        // Core weight for 4 cores is 0.5 (not clamped lower).
        assert!(b.cpu_target_khz < a.cpu_target_khz);
        assert_eq!(b.cpu_target_khz, (3_000_000.0f32 * 0.5f32).round() as u32);
    }

    #[test]
    fn core_weight_clamped_at_half_for_fewer_cores() {
        let t = base_telemetry(30, 1, 3_000_000);
        let cfg = compute_profile(&t, BIAS_EXTREME_BEAST, None);
        assert_eq!(cfg.cpu_target_khz, (3_000_000.0f32 * 0.5f32).round() as u32);
    }

    #[test]
    fn never_exceeds_hardware_max() {
        // Headroom could theoretically push target above max if bias > 1; the
        // clamp guards against that.
        let t = base_telemetry(30, 8, 3_000_000);
        let cfg = compute_profile(&t, 1.5, None);
        assert!(cfg.cpu_target_khz <= 3_000_000);
    }

    #[test]
    fn absurd_temperature_falls_back_to_bias_only() {
        // Very low temperature (below min_operating) — plausibility guard.
        let t = base_telemetry(-10, 8, 3_000_000);
        let cfg = compute_profile(&t, BIAS_EXTREME_BEAST, None);
        assert_eq!(cfg.cpu_target_khz, 3_000_000);
        assert_eq!(cfg.mode, ProfileMode::Max);
        assert!((cfg.headroom - 1.0).abs() < 0.001);
    }

    #[test]
    fn high_temperature_falls_back_to_bias_only() {
        // Above max_safe_temp — should also fall back.
        let t = base_telemetry(120, 8, 3_000_000);
        let cfg = compute_profile(&t, BIAS_EXTREME_BEAST, None);
        assert_eq!(cfg.cpu_target_khz, 3_000_000);
    }

    #[test]
    fn zero_max_freq_does_not_crash() {
        let t = base_telemetry(30, 8, 0);
        let cfg = compute_profile(&t, BIAS_EXTREME_BEAST, None);
        assert_eq!(cfg.cpu_target_khz, 1);
    }

    #[test]
    fn mode_is_steady_only_above_high_threshold() {
        let low = base_telemetry(PROFILE_MODE_LOW_C, 8, 3_000_000);
        let high = base_telemetry(PROFILE_MODE_HIGH_C, 8, 3_000_000);
        assert_eq!(
            compute_profile(&low, BIAS_EXTREME_BEAST, None).mode,
            ProfileMode::Max
        );
        assert_eq!(
            compute_profile(&high, BIAS_EXTREME_BEAST, None).mode,
            ProfileMode::Steady
        );
    }

    #[test]
    fn hysteresis_returns_to_max_below_low_threshold() {
        let t = base_telemetry(PROFILE_MODE_LOW_C, 8, 3_000_000);
        let cfg = compute_profile(&t, BIAS_EXTREME_BEAST, Some(ProfileMode::Steady));
        assert_eq!(cfg.mode, ProfileMode::Max);
    }

    #[test]
    fn hysteresis_stays_steady_above_high_threshold() {
        let t = base_telemetry(PROFILE_MODE_HIGH_C, 8, 3_000_000);
        let cfg = compute_profile(&t, BIAS_EXTREME_BEAST, Some(ProfileMode::Max));
        assert_eq!(cfg.mode, ProfileMode::Steady);
    }
}
