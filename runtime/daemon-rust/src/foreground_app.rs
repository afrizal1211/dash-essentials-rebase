//! Foreground app tracking module
//!
//! Tracks the currently foreground app via AccessibilityService events
//! and integrates with the per-app profile system.

use std::sync::{Arc, Mutex, OnceLock};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::app_controls::App;

/// Information about the currently foreground app
#[derive(Debug, Clone)]
pub struct ForegroundAppInfo {
    pub package_name: String,
    pub class_name: String,
    pub timestamp: Instant,
}

impl ForegroundAppInfo {
    pub fn new(package_name: String, class_name: String) -> Self {
        Self {
            package_name,
            class_name,
            timestamp: Instant::now(),
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.package_name.is_empty() && self.timestamp.elapsed() < Duration::from_secs(30)
    }

    pub fn package_name(&self) -> &str {
        &self.package_name
    }

    pub fn class_name(&self) -> &str {
        &self.class_name
    }
}

/// Tracks the foreground app and manages per-app profile activation
pub struct ForegroundAppTracker {
    current_app: Mutex<Option<ForegroundAppInfo>>,
    app_timestamps: Mutex<HashMap<String, Instant>>,
    last_known_app: Mutex<Option<String>>,
}

impl Default for ForegroundAppTracker {
    fn default() -> Self {
        Self {
            current_app: Mutex::new(None),
            app_timestamps: Mutex::new(HashMap::new()),
            last_known_app: Mutex::new(None),
        }
    }
}

impl ForegroundAppTracker {
    /// Get the singleton instance
    pub fn global() -> &'static ForegroundAppTracker {
        static INSTANCE: OnceLock<ForegroundAppTracker> = OnceLock::new();
        INSTANCE.get_or_init(ForegroundAppTracker::default)
    }

    /// Update the foreground app from AccessibilityService event
    pub fn on_foreground_app_changed(&self, package_name: String, class_name: String) {
        let info = ForegroundAppInfo::new(package_name.clone(), class_name.clone());

        // Update current app
        {
            let mut current = self.current_app.lock().unwrap();
            *current = Some(info.clone());
        }

        // Update timestamp
        {
            let mut timestamps = self.app_timestamps.lock().unwrap();
            timestamps.insert(package_name.clone(), Instant::now());
        }

        // Update last known
        {
            let mut last = self.last_known_app.lock().unwrap();
            *last = Some(package_name.clone());
        }

        // Trigger per-app profile update
        self.trigger_per_app_update(&package_name);
    }

    /// Get current foreground app info
    pub fn current_app(&self) -> Option<ForegroundAppInfo> {
        self.current_app.lock().unwrap().clone()
    }

    /// Get the last known foreground app package name
    pub fn last_known_app(&self) -> Option<String> {
        self.last_known_app.lock().unwrap().clone()
    }

    /// Check if a specific app is currently in foreground
    pub fn is_app_foreground(&self, package_name: &str) -> bool {
        self.current_app.lock().unwrap()
            .as_ref()
            .map(|info| info.package_name == package_name && info.is_valid())
            .unwrap_or(false)
    }

    /// Get timestamp of last seen for an app
    pub fn app_last_seen(&self, package_name: &str) -> Option<Instant> {
        self.app_timestamps.lock().unwrap().get(package_name).copied()
    }

    /// Trigger per-app profile re-evaluation
    fn trigger_per_app_update(&self, package_name: &str) {
        // Wake up the app runtime to re-evaluate per-app profiles
        crate::app_runtime::wake(crate::app_runtime::Wake::Changed);

        // Log the change
        crate::diagnostics::record(
            crate::diagnostics::AREA_PER_APP,
            "info",
            "foreground_app_changed",
            &format!("Foreground app changed to: {}", package_name),
        );
    }

    /// Clean up stale entries (call periodically)
    pub fn cleanup_stale_entries(&self) {
        let cutoff = Instant::now() - Duration::from_secs(300); // 5 minutes
        let mut timestamps = self.app_timestamps.lock().unwrap();
        timestamps.retain(|_, &mut time| time > cutoff);
    }
}

/// Called from JNI when foreground app changes
#[unsafe(no_mangle)]
pub extern "C" fn Java_com_redfox_daemon_jni_ForegroundDetectorJni_notifyForegroundAppChanged(
    _env: *mut std::os::raw::c_void,
    _class: *mut std::os::raw::c_void,
    package_name: *const std::os::raw::c_char,
    class_name: *const std::os::raw::c_char,
) -> std::os::raw::c_int {
    let package_name = unsafe { std::ffi::CStr::from_ptr(package_name).to_string_lossy().into_owned() };
    let class_name = unsafe { std::ffi::CStr::from_ptr(class_name).to_string_lossy().into_owned() };

    ForegroundAppTracker::global().on_foreground_app_changed(package_name, class_name);

    1 // Return true (success)
}

#[unsafe(no_mangle)]
pub extern "C" fn Java_com_redfox_daemon_jni_ForegroundDetectorJni_isAvailable(
    _env: *mut std::os::raw::c_void,
    _class: *mut std::os::raw::c_void,
) -> std::os::raw::c_int {
    1 // Always available
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_foreground_app_tracker() {
        let tracker = ForegroundAppTracker::default();

        // Initially no app
        assert!(tracker.current_app().is_none());
        assert!(!tracker.is_app_foreground("com.test.app"));

        // Set foreground app
        tracker.on_foreground_app_changed("com.test.app".to_string(), "com.test.app.MainActivity".to_string());

        // Should now be foreground
        assert!(tracker.is_app_foreground("com.test.app"));
        assert_eq!(tracker.current_app().unwrap().package_name, "com.test.app");

        // Different app
        tracker.on_foreground_app_changed("com.other.app".to_string(), "com.other.app.MainActivity".to_string());

        assert!(tracker.is_app_foreground("com.other.app"));
        assert!(!tracker.is_app_foreground("com.test.app"));
    }

    #[test]
    fn test_app_timestamps() {
        let tracker = ForegroundAppTracker::default();

        tracker.on_foreground_app_changed("com.test.app".to_string(), "MainActivity".to_string());

        let timestamp = tracker.app_last_seen("com.test.app");
        assert!(timestamp.is_some());

        // Wait a bit
        std::thread::sleep(std::time::Duration::from_millis(10));

        tracker.on_foreground_app_changed("com.other.app".to_string(), "MainActivity".to_string());

        let old_timestamp = tracker.app_last_seen("com.test.app");
        let new_timestamp = tracker.app_last_seen("com.other.app");

        assert!(old_timestamp.is_some());
        assert!(new_timestamp.is_some());
        assert!(new_timestamp.unwrap() > old_timestamp.unwrap());
    }
}