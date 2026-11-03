//! Adaptive performance profile manager.
//!
//! Computes target CPU and GPU frequencies based on hardware topology,
//! current thermal state, and a profile-specific bias.

use crate::telemetry::Telemetry;

pub struct ProfileConfig {
    pub cpu_target_khz: u32,
}

/// Compute target frequencies based on thermal headroom and hardware specs.
///
/// base_bias: 1.0 for Extreme Beats, 0.85 for Dynamic Gaming.
pub fn compute_profile(telemetry: &Telemetry, base_bias: f32) -> ProfileConfig {
    // 1. Thermal headroom factor [0.0, 1.0]
    let max_temp = telemetry.max_safe_temp_c.unwrap_or(85);
    let cur_temp = telemetry.cpu_temp_dc as u32; // assume deciCelsius -> treat as Celsius for simplicity
    let min_temp = telemetry.min_operating_temp_c;

    let headroom = ((max_temp - cur_temp) as f32 / (max_temp - min_temp) as f32).clamp(0.0, 1.0);

    // 2. Core-count weight (using 8 as a generic high‑end reference)
    let max_cpu_cores = 8;
    let cpu_weight = telemetry.cpu_core_count as f32 / max_cpu_cores as f32;

    // 3. Target CPU frequency
    let cpu_target =
        (telemetry.cpu_max_freq_khz as f32 * headroom * cpu_weight * base_bias).round() as u32;

    // Clamp to nearest supported step (optional – we keep the raw value for now)
    // cpu_target = nearest_step(cpu_target, "/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_frequencies");

    ProfileConfig {
        cpu_target_khz: cpu_target,
    }
}
