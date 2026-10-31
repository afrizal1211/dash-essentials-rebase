//! Small Android framework adapter. No charging sysfs writes and no Flutter UI
//! work on Android's main thread. NEESCHAL's rule: report state, don't invent it.
use jni::{
    JNIEnv,
    objects::{JClass, JObject, JValue},
    sys::{jboolean, jfloat, jint, jstring},
};
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicI32, Ordering},
};

static ACTIVITY: OnceLock<Mutex<Option<(jni::JavaVM, jni::objects::GlobalRef)>>> = OnceLock::new();
static BACK_EVENT: AtomicI32 = AtomicI32::new(0);
static OPEN_BYPASS: AtomicI32 = AtomicI32::new(0);

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
// Registration and invocation share the lock: Dart can safely unregister before
// closing its callback, including when Android delivers a final lifecycle event.
static BACK_LISTENER: Mutex<Option<extern "C" fn(i32)>> = Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "C" fn dash_host_set_back_listener(listener: Option<extern "C" fn(i32)>) {
    *BACK_LISTENER.lock().unwrap() = listener;
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
        *ACTIVITY.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some((vm, reference));
    }
}
pub fn clear() {
    if let Some(activity) = ACTIVITY.get() {
        *activity.lock().unwrap() = None;
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
    let guard = activity.lock().unwrap();
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
    let listener = BACK_LISTENER.lock().unwrap();
    if let Some(callback) = *listener {
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
