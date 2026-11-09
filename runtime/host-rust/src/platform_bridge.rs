//! Small Android framework adapter. No charging sysfs writes and no Flutter UI
//! work on Android's main thread. NEESCHAL's rule: report state, don't invent it.
use jni::{
    JNIEnv,
    objects::{JClass, JObject, JValue},
    sys::{jboolean, jfloat, jint, jlong, jstring},
};
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicI32, Ordering},
};

static ACTIVITY: OnceLock<Mutex<Option<(jni::JavaVM, jni::objects::GlobalRef)>>> = OnceLock::new();
static BACK_EVENT: AtomicI32 = AtomicI32::new(0);
static OPEN_BYPASS: AtomicI32 = AtomicI32::new(0);
static OPEN_FPS_RECORD: AtomicI32 = AtomicI32::new(0);

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_DashActivity_nativeOpenBypassSettings(
    _: JNIEnv,
    _: JClass,
) {
    OPEN_BYPASS.store(1, Ordering::Release);
}
#[unsafe(no_mangle)]
pub extern "C" fn dash_host_consume_bypass_settings_request() -> i32 {
    OPEN_BYPASS.swap(0, Ordering::AcqRel)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_DashActivity_openFpsRecordFromNotification(
    _: JNIEnv,
    _: JClass,
) {
    OPEN_FPS_RECORD.store(1, Ordering::Release);
}
#[unsafe(no_mangle)]
pub extern "C" fn dash_host_consume_fps_record_open_request() -> i32 {
    OPEN_FPS_RECORD.swap(0, Ordering::AcqRel)
}

/// Starts or stops the Prism-style status notification service.
#[unsafe(no_mangle)]
pub extern "C" fn dash_host_status_notification(enabled: i32) -> i32 {
    with_activity(|env, activity| {
        env.call_method(
            activity,
            "setStatusNotification",
            "(Z)I",
            &[JValue::Bool(u8::from(enabled != 0))],
        )?
        .i()
    })
    .unwrap_or(0)
}
// Registration and invocation share the lock: Dart can safely unregister before
// closing its callback, including when Android delivers a final lifecycle event.
static BACK_LISTENER: Mutex<Option<extern "C" fn(i32)>> = Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "C" fn dash_host_set_back_listener(listener: Option<extern "C" fn(i32)>) {
    *BACK_LISTENER.lock().unwrap_or_else(|e| e.into_inner()) = listener;
    BACK_EVENT.store(0, Ordering::Release);
    super::DASH_BACK_PENDING.store(false, Ordering::Release);
}

pub fn init(activity: *mut super::ANativeActivity) {
    let Some((vm, object, _)) = super::dash_photo_vm_and_object(activity) else {
        return;
    };
    let reference = {
        let Ok(env) = vm.attach_current_thread() else {
            return;
        };
        let object = unsafe { jni::objects::JObject::from_raw(object) };
        env.new_global_ref(&object).ok()
    };
    if let Some(reference) = reference {
        *ACTIVITY.get_or_init(|| Mutex::new(None)).lock().unwrap_or_else(|e| e.into_inner()) = Some((vm, reference));
    }
}
pub fn clear() {
    if let Some(activity) = ACTIVITY.get() {
        *activity.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    BACK_EVENT.store(0, Ordering::Release);
}

/// Runs one call against the captured activity. The lock is held only for the
/// duration of the call, so the overlay toggle cannot race activity teardown.
fn with_activity<R>(
    call: impl FnOnce(&mut JNIEnv, &JObject<'_>) -> jni::errors::Result<R>,
) -> Option<R> {
    let guard = ACTIVITY.get()?.lock().ok()?;
    let (vm, reference) = guard.as_ref()?;
    let mut env = vm.attach_current_thread().ok()?;
    let result = env
        .with_local_frame(8, |env| call(env, reference.as_obj()))
        .ok();
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result
}

/// Starts or stops the foreground FPS overlay. `0` means the window
/// permission is missing, so the caller must not persist "enabled".
#[unsafe(no_mangle)]
pub extern "C" fn dash_host_fps_overlay(enabled: i32) -> i32 {
    with_activity(|env, activity| {
        env.call_method(
            activity,
            "setFpsOverlay",
            "(Z)I",
            &[JValue::Bool(u8::from(enabled != 0))],
        )?
        .i()
    })
    .unwrap_or(0)
}

/// Whether Android will let this app draw over other apps.
#[unsafe(no_mangle)]
pub extern "C" fn dash_host_fps_overlay_granted() -> i32 {
    i32::from(
        with_activity(|env, activity| {
            env.call_method(activity, "hasFpsOverlayPermission", "()Z", &[])?
                .z()
        })
        .unwrap_or(false),
    )
}

/// Opens the system screen where the overlay permission is granted.
#[unsafe(no_mangle)]
pub extern "C" fn dash_host_fps_overlay_request() -> i32 {
    i32::from(
        with_activity(|env, activity| {
            env.call_method(activity, "requestFpsOverlayPermission", "()V", &[])?;
            Ok(true)
        })
        .unwrap_or(false),
    )
}

/// Pushes the overlay style (scale, opacity, corner radius, accent) into the
/// service's SharedPreferences. Four packed ints instead of a JSON string so
/// the caller needs no C-string allocation and no `package:ffi` dependency.
/// Window position is not configurable here: the service owns and persists it,
/// because only the service can observe a drag.
#[unsafe(no_mangle)]
pub extern "C" fn dash_host_fps_overlay_config(
    scale_milli: i32,
    opacity: i32,
    radius_milli: i32,
    accent: i32,
    per_game: i32,
) -> i32 {
    i32::from(
        with_activity(|env, activity| {
            env.call_method(
                activity,
                "configFpsOverlay",
                "(IIIII)Z",
                &[
                    JValue::Int(scale_milli),
                    JValue::Int(opacity),
                    JValue::Int(radius_milli),
                    JValue::Int(accent),
                    JValue::Int(per_game),
                ],
            )?
            .z()
        })
        .unwrap_or(false),
    )
}

/// The full foreground snapshot for the overlay chip. The service parses the
/// JSON so one poll draws the frame reading, the sensors and the package,
/// instead of one JNI call per value.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_FpsOverlayService_nativeSnapshot(
    env: JNIEnv,
    _: JClass,
) -> jstring {
    match env.new_string(super::backend_bridge::foreground_fps_json()) {
        Ok(text) => text.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
pub fn refresh_back() {
    let Some(activity) = ACTIVITY.get() else {
        return;
    };
    let guard = activity.lock().unwrap_or_else(|e| e.into_inner());
    let Some((vm, reference)) = guard.as_ref() else {
        return;
    };
    let Ok(mut env) = vm.attach_current_thread() else {
        return;
    };
    let _ = env.with_local_frame(4, |env| -> jni::errors::Result<()> {
        let class = env.get_object_class(reference.as_obj())?;
        env.call_static_method(class, "refreshBackFromNative", "()V", &[])?;
        Ok(())
    });
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn dash_host_consume_back_gesture() -> i32 {
    BACK_EVENT.swap(0, Ordering::AcqRel)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_DashActivity_nativeInterceptBack(
    _: JNIEnv,
    _: JClass,
) -> jboolean {
    super::DASH_BACK_INTERCEPT.load(Ordering::Acquire) as jboolean
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_DashActivity_nativeBack(
    _: JNIEnv,
    _: JClass,
    phase: jint,
    progress: jfloat,
    edge: jint,
) {
    if !(1..=4).contains(&phase) || !progress.is_finite() {
        return;
    }
    if phase != 2 {
        super::log_str(&format!(
            "PLATFORM_BACK phase={phase} progress={progress} edge={edge}"
        ));
    }
    let value = (progress.clamp(0.0, 1.0) * 10000.0).round() as i32;
    // Single atomic packet: phase, edge and progress cannot come from different frames.
    let packet = (phase << 16) | ((edge & 1) << 15) | value;
    // Copy the callback out and drop the lock before invoking: the callback is
    // Dart-facing and may re-register itself, which would deadlock a held lock.
    let listener = BACK_LISTENER.lock().unwrap_or_else(|e| e.into_inner());
    let callback = *listener;
    if let Some(callback) = callback {
        callback(packet);
        return;
    }
    BACK_EVENT.store(packet, Ordering::Release);
    if phase == 4 && super::DASH_BACK_INTERCEPT.load(Ordering::Acquire) {
        super::DASH_BACK_PENDING.store(true, Ordering::Release);
    }
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_BypassTileService_nativeStart(
    _: JNIEnv,
    _: JClass,
) {
    super::backend_bridge::dash_backend_start();
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_BypassTileService_nativeSnapshot(
    _: JNIEnv,
    _: JClass,
) -> jint {
    use super::backend_bridge::*;
    let phase = dash_backend_extended_get(120);
    let confirmed = super::bypass_tile::direct_power_confirmed(
        phase,
        dash_backend_extended_get(115),
        dash_backend_get_usb_online(),
        dash_backend_get_battery_current_ua(),
    );
    i32::from(dash_backend_ready() == 1)
        | (i32::from(dash_backend_extended_get(112) == 1) << 1)
        | (i32::from(dash_backend_extended_get(114) == 1) << 2)
        | (i32::from(confirmed) << 3)
        | (i32::from(dash_backend_get_charging_write_state() == 1) << 4)
        | (i32::from(dash_backend_extended_get(116) == 1) << 5)
        | (i32::from(
            dash_backend_extended_get(121) > 0
                || dash_backend_get_charging_write_state() < 0
                || phase == 5,
        ) << 6)
        | ((phase.clamp(0, 15)) << 7)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_BypassTileService_nativeToggle(
    _: JNIEnv,
    _: JClass,
) -> jboolean {
    use super::backend_bridge::*;
    if dash_backend_ready() != 1
        || dash_backend_extended_get(112) != 1
        || dash_backend_extended_get(116) != 1
        || dash_backend_get_charging_write_state() == 1
    {
        return 0;
    }
    let saved = dash_backend_extended_get(114);
    if !matches!(saved, 0 | 1) {
        return 0;
    }
    dash_backend_set_bypass_charging(1 - saved) as jboolean
}

/// Snapshot of FPS recording state for the Quick Settings tile.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_FpsRecordTileService_nativeState(
    _: JNIEnv,
    _: JClass,
) -> jint {
    super::backend_bridge::fps_record_state() as jint
}

/// Toggle manual FPS recording from the Quick Settings tile.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_FpsRecordTileService_nativeToggle(
    _: JNIEnv,
    _: JClass,
) -> jboolean {
    super::backend_bridge::fps_record_toggle() as jboolean
}

/// Writes one FPS export (CSV plus an optional graph PNG) to public storage
/// through `DashActivity.saveFpsExport`. Returns 1 when the activity saved
/// both files and showed the toast, 0 otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn dash_host_export_save(
    csv_ptr: *const u8,
    csv_len: i32,
    png_ptr: *const u8,
    png_len: i32,
) -> i32 {
    let Some(activity) = ACTIVITY.get() else {
        return 0;
    };
    let guard = activity.lock().unwrap_or_else(|e| e.into_inner());
    let Some((vm, reference)) = guard.as_ref() else {
        return 0;
    };
    let Ok(mut env) = vm.attach_current_thread() else {
        return 0;
    };
    // Hard cap the FFI-provided lengths to what a real export can be: an
    // oversized/negative length from a buggy caller must not turn into an
    // out-of-bounds slice read.
    const MAX_EXPORT_LEN: usize = 16 * 1024 * 1024;
    if csv_len < 0 || png_len < 0 || csv_len as usize > MAX_EXPORT_LEN || png_len as usize > MAX_EXPORT_LEN {
        return 0;
    }
    let csv = unsafe { std::slice::from_raw_parts(csv_ptr, csv_len as usize) };
    let png = unsafe { std::slice::from_raw_parts(png_ptr, png_len as usize) };
    let result = env.with_local_frame(16, |env| -> jni::errors::Result<i32> {
        let csv_jstring = env.new_string(String::from_utf8_lossy(csv))?;
        let png_array = env.byte_array_from_slice(png)?;
        env.call_method(
            reference.as_obj(),
            "saveFpsExport",
            "(Ljava/lang/String;[B)V",
            &[
                jni::objects::JValue::Object(&csv_jstring),
                jni::objects::JValue::Object(&png_array),
            ],
        )?;
        Ok(1)
    });
    match result {
        Ok(code) => code,
        Err(error) => {
            super::log_str(&format!("DASH_HOST_EXPORT_SAVE {error}"));
            let _ = env.exception_clear();
            0
        }
    }
}

// DashStatusNotificationService bridge. The daemon computes deep sleep and
// battery drain (it owns the privileged reads); these entry points only read
// the cached snapshot, so they are safe from any thread and never touch the
// activity. Values are -1 while the daemon has not answered yet.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_DashStatusNotificationService_nativeDeepSleepMs() -> jlong {
    super::backend_bridge::dash_backend_get_deep_sleep_ms()
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_DashStatusNotificationService_nativeDeepSleepPct() -> jint {
    super::backend_bridge::dash_backend_get_deep_sleep_pct()
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_DashStatusNotificationService_nativeDrainPctPerHour() -> jint {
    super::backend_bridge::dash_backend_get_drain_pct_per_h()
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_DashStatusNotificationService_nativeBatteryPowerMw() -> jlong {
    super::backend_bridge::dash_backend_get_battery_power_mw()
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_neeschal_dashessential_DashStatusNotificationService_nativeRequestRefresh() {
    let _ = super::backend_bridge::dash_backend_refresh();
}
