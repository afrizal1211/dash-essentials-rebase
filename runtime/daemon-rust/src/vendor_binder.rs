use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::OnceLock;

const STATUS_OK: i32 = 0;
const FLAG_PRIVATE_VENDOR: u32 = 0x1000_0000;

#[repr(C)]
pub struct AIBinder {
    _private: [u8; 0],
}
#[repr(C)]
pub struct AIBinder_Class {
    _private: [u8; 0],
}
#[repr(C)]
pub struct AParcel {
    _private: [u8; 0],
}
#[repr(C)]
pub struct AStatus {
    _private: [u8; 0],
}

type OnCreate = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type OnDestroy = unsafe extern "C" fn(*mut c_void);
type OnTransact = unsafe extern "C" fn(*mut AIBinder, u32, *const AParcel, *mut AParcel) -> i32;

#[link(name = "binder_ndk")]
unsafe extern "C" {
    fn AIBinder_Class_define(
        descriptor: *const c_char,
        on_create: OnCreate,
        on_destroy: OnDestroy,
        on_transact: OnTransact,
    ) -> *mut AIBinder_Class;
    fn AIBinder_associateClass(binder: *mut AIBinder, clazz: *const AIBinder_Class) -> bool;
    fn AIBinder_prepareTransaction(binder: *mut AIBinder, input: *mut *mut AParcel) -> i32;
    fn AIBinder_transact(
        binder: *mut AIBinder,
        code: u32,
        input: *mut *mut AParcel,
        output: *mut *mut AParcel,
        flags: u32,
    ) -> i32;
    fn AIBinder_decStrong(binder: *mut AIBinder);
    fn AIBinder_isAlive(binder: *mut AIBinder) -> bool;
    fn AParcel_writeInt32(parcel: *mut AParcel, value: i32) -> i32;
    fn AParcel_readInt32(parcel: *const AParcel, value: *mut i32) -> i32;
    fn AParcel_readStatusHeader(parcel: *const AParcel, status: *mut *mut AStatus) -> i32;
    fn AParcel_delete(parcel: *mut AParcel);
    fn AStatus_isOk(status: *const AStatus) -> bool;
    fn AStatus_delete(status: *mut AStatus);
}

unsafe extern "C" fn on_create(_: *mut c_void) -> *mut c_void {
    ptr::null_mut()
}

unsafe extern "C" fn on_destroy(_: *mut c_void) {}

unsafe extern "C" fn on_transact(
    _: *mut AIBinder,
    _: u32,
    _: *const AParcel,
    _: *mut AParcel,
) -> i32 {
    -38
}

const RTLD_NOW: i32 = 2;

type CheckServiceFn = unsafe extern "C" fn(instance: *const c_char) -> *mut AIBinder;

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

static CHECK_SERVICE_FN: OnceLock<Option<CheckServiceFn>> = OnceLock::new();
static TOUCH_CLASS: OnceLock<usize> = OnceLock::new();
static DISPLAY_CLASS: OnceLock<usize> = OnceLock::new();

fn service_manager_check_service(instance: *const c_char) -> *mut AIBinder {
    let resolved = CHECK_SERVICE_FN.get_or_init(|| unsafe {
        let handle = dlopen(c"libbinder_ndk.so".as_ptr(), RTLD_NOW);
        if handle.is_null() {
            return None;
        }

        let set_threads_sym = dlsym(
            handle,
            c"ABinderProcess_setThreadPoolMaxThreadCount".as_ptr(),
        );
        if !set_threads_sym.is_null() {
            let set_threads: unsafe extern "C" fn(u32) -> bool =
                std::mem::transmute(set_threads_sym);
            set_threads(4);
        }
        let start_threads_sym = dlsym(handle, c"ABinderProcess_startThreadPool".as_ptr());
        if !start_threads_sym.is_null() {
            let start_threads: unsafe extern "C" fn() = std::mem::transmute(start_threads_sym);
            start_threads();
        }

        let symbol = dlsym(handle, c"AServiceManager_checkService".as_ptr());
        if symbol.is_null() {
            return None;
        }

        Some(std::mem::transmute::<*mut c_void, CheckServiceFn>(symbol))
    });

    match *resolved {
        Some(function) => unsafe { function(instance) },
        None => ptr::null_mut(),
    }
}

fn class_for(
    descriptor: &'static [u8],
    slot: &'static OnceLock<usize>,
) -> *const AIBinder_Class {
    let raw = *slot.get_or_init(|| unsafe {
        AIBinder_Class_define(
            descriptor.as_ptr().cast(),
            on_create,
            on_destroy,
            on_transact,
        ) as usize
    });
    raw as *const AIBinder_Class
}

/// Look a service up without associating a class with it.
///
/// A class makes the NDK treat incoming transactions to the proxy as
/// local ones and dispatch them into the class. A daemon that only
/// wants to know whether a service is there must not do that: the
/// dispatch has been seen to fault, and it faults on a binder thread.
///
/// A service that is not there is answered with a binder that never
/// becomes one. Such a binder is dropped here, because a transaction
/// to it is the one that faults.
fn find_service(service: &'static [u8]) -> Option<*mut AIBinder> {
    let binder = service_manager_check_service(service.as_ptr().cast());
    if binder.is_null() {
        return None;
    }
    if !unsafe { AIBinder_isAlive(binder) } {
        unsafe { AIBinder_decStrong(binder) };
        return None;
    }
    Some(binder)
}

fn check_service(
    service: &'static [u8],
    descriptor: &'static [u8],
    slot: &'static OnceLock<usize>,
) -> Option<*mut AIBinder> {
    let binder = service_manager_check_service(service.as_ptr().cast());
    if binder.is_null() {
        return None;
    }
    if !unsafe { AIBinder_isAlive(binder) } {
        unsafe { AIBinder_decStrong(binder) };
        return None;
    }

    let clazz = class_for(descriptor, slot);
    if clazz.is_null() || !unsafe { AIBinder_associateClass(binder, clazz) } {
        unsafe { AIBinder_decStrong(binder) };
        return None;
    }

    Some(binder)
}

/// Whether the daemon may talk to the vendor services at all.
///
/// The module's daemon runs in the module's own context. The vendor
/// service manager may not be visible there, so a service it asks for
/// can come back as a binder that never becomes one. A binder that is
/// not alive is dropped before anything is sent to it, so the daemon
/// can ask and find out instead of refusing to ask at all: on a device
/// where the module's context does see the vendor services, the display
/// features are then in reach of the module as well.
///
/// The environment still switches the route off for a device where the
/// vendor services are not wanted.
fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("DASH_VENDOR_BINDER").as_deref() != Ok("0"))
}


/// What the service manager answered the last time it was asked.
///
/// The answer is kept for a while: the daemon cannot reach the vendor
/// services from the module's context, and every attempt to ask for one
/// is a fork, so the answer is not asked for again until it is worth
/// asking.
static AVAILABILITY: OnceLock<
    std::sync::Mutex<(Option<std::time::Instant>, bool, bool)>,
> = OnceLock::new();
const AVAILABILITY_TTL: std::time::Duration = std::time::Duration::from_secs(30);

fn touch_available_inner() -> bool {
    const SERVICE: &[u8] = b"vendor.xiaomi.hw.touchfeature.ITouchFeature/default\0";
    let Some(binder) = find_service(SERVICE) else {
        return false;
    };
    unsafe { AIBinder_decStrong(binder) };
    true
}

fn display_available_inner() -> bool {
    const SERVICE: &[u8] =
        b"vendor.xiaomi.hardware.displayfeature_aidl.IDisplayFeature/default\0";
    let Some(binder) = find_service(SERVICE) else {
        return false;
    };
    unsafe { AIBinder_decStrong(binder) };
    true
}

/// Whether the vendor services can be reached, answered in a child.
///
/// The daemon may run in the module's own context, and the vendor
/// service manager may not be visible there. The NDK's completion for a
/// binder that never becomes one has been seen to fault, and it faults
/// on a binder thread, which takes the whole daemon with it. The
/// question is therefore asked in a child, and the answer is kept for a
/// while.
fn vendor_availability() -> (bool, bool) {
    let cache = AVAILABILITY.get_or_init(|| std::sync::Mutex::new((None, false, false)));
    let Ok(mut guard) = cache.lock() else {
        return (false, false);
    };
    let now = std::time::Instant::now();
    if let Some(at) = guard.0
        && now.duration_since(at) < AVAILABILITY_TTL
    {
        return (guard.1, guard.2);
    }
    let touch = guarded_call(touch_available_inner);
    let display = guarded_call(display_available_inner);
    *guard = (Some(now), touch, display);
    (touch, display)
}

pub fn touch_available() -> bool {
    if !enabled() {
        return false;
    }
    vendor_availability().0
}

pub fn display_available() -> bool {
    if !enabled() {
        return false;
    }
    vendor_availability().1
}

/// Run a vendor call in a child process.
///
/// The vendor services and the NDK's own machinery have been seen to
/// fault on this platform. A fault in the daemon would take the whole
/// service down and leave the device with no controls at all, so the
/// call runs in a forked child and only its result is taken back. A
/// child that dies is reported as a failure.
fn guarded_call(call: impl FnOnce() -> bool) -> bool {
    unsafe extern "C" {
        fn pipe(fds: *mut i32) -> i32;
        fn fork() -> i32;
        fn read(fd: i32, buf: *mut c_void, count: usize) -> isize;
        fn write(fd: i32, buf: *const c_void, count: usize) -> isize;
        fn close(fd: i32) -> i32;
        fn kill(pid: i32, signal: i32) -> i32;
        fn _exit(code: i32) -> !;
        fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    }
    #[repr(C)]
    struct PollFd {
        fd: i32,
        events: i16,
        revents: i16,
    }

    let mut fds = [0i32; 2];
    if unsafe { pipe(fds.as_mut_ptr()) } != 0 {
        return call();
    }
    let pid = unsafe { fork() };
    if pid < 0 {
        unsafe { close(fds[0]); close(fds[1]) };
        return call();
    }
    if pid == 0 {
        unsafe { close(fds[0]) };
        let byte = [u8::from(call())];
        unsafe { write(fds[1], byte.as_ptr().cast(), 1) };
        unsafe { close(fds[1]) };
        unsafe { _exit(0) };
    }

    unsafe { close(fds[1]) };
    let mut waiting = PollFd {
        fd: fds[0],
        events: 0x0001,
        revents: 0,
    };
    let ready = unsafe { poll(&mut waiting, 1, 2000) };
    let mut byte = [0u8; 1];
    let read_bytes = if ready > 0 {
        unsafe { read(fds[0], byte.as_mut_ptr().cast(), 1) }
    } else {
        // The transaction did not answer in time. The child is killed so
        // it cannot linger, and the transaction counts as a failure.
        unsafe { kill(pid, 9) };
        -1
    };
    unsafe { close(fds[0]) };
    let mut status = 0i32;
    unsafe { waitpid(pid, &mut status, 0) };
    read_bytes == 1 && byte[0] != 0
}

pub fn set_touch_mode(display_id: i32, mode: i32, value: i32) -> bool {
    if !enabled() {
        return false;
    }
    guarded_call(|| touch_mode_transaction(display_id, mode, value))
}

fn touch_mode_transaction(display_id: i32, mode: i32, value: i32) -> bool {
    const SERVICE: &[u8] = b"vendor.xiaomi.hw.touchfeature.ITouchFeature/default\0";
    const DESCRIPTOR: &[u8] = b"vendor.xiaomi.hw.touchfeature.ITouchFeature\0";
    let Some(binder) = check_service(SERVICE, DESCRIPTOR, &TOUCH_CLASS) else {
        return false;
    };

    let mut input: *mut AParcel = ptr::null_mut();
    let mut output: *mut AParcel = ptr::null_mut();
    let mut ok = unsafe { AIBinder_prepareTransaction(binder, &mut input) } == STATUS_OK;

    if ok {
        ok &= unsafe { AParcel_writeInt32(input, display_id) } == STATUS_OK;
        ok &= unsafe { AParcel_writeInt32(input, mode) } == STATUS_OK;
        ok &= unsafe { AParcel_writeInt32(input, value) } == STATUS_OK;
    }

    if ok {
        ok &= unsafe {
            AIBinder_transact(binder, 9, &mut input, &mut output, FLAG_PRIVATE_VENDOR)
        } == STATUS_OK;
    } else if !input.is_null() {
        unsafe { AParcel_delete(input) };
    }

    if ok && !output.is_null() {
        let mut status: *mut AStatus = ptr::null_mut();
        ok &= unsafe { AParcel_readStatusHeader(output, &mut status) } == STATUS_OK;
        if !status.is_null() {
            ok &= unsafe { AStatus_isOk(status) };
            unsafe { AStatus_delete(status) };
        } else {
            ok = false;
        }

        let mut result = -1i32;
        if ok {
            ok &= unsafe { AParcel_readInt32(output, &mut result) } == STATUS_OK;
            ok &= result == 0;
        }
    } else {
        ok = false;
    }

    if !output.is_null() {
        unsafe { AParcel_delete(output) };
    }
    unsafe { AIBinder_decStrong(binder) };
    ok
}

pub fn set_display_feature(case_id: i32, mode_id: i32, cookie: i32) -> bool {
    if !enabled() {
        return false;
    }
    let applied = guarded_call(|| {
        // A caller that is not part of the vendor domain has to ask for
        // the private form of the transaction. The ordinary form is tried
        // first, because a caller that is part of the vendor domain may
        // not use the private form.
        display_feature_transaction(case_id, mode_id, cookie, 0)
            || display_feature_transaction(case_id, mode_id, cookie, FLAG_PRIVATE_VENDOR)
    });

    if !applied {
        crate::diagnostics::record(
            crate::diagnostics::AREA_DISPLAY,
            "warn",
            "route.vendor",
            "the vendor display service did not answer",
        );
    }

    applied
}

fn display_feature_transaction(case_id: i32, mode_id: i32, cookie: i32, flags: u32) -> bool {
    const SERVICE: &[u8] =
        b"vendor.xiaomi.hardware.displayfeature_aidl.IDisplayFeature/default\0";
    const DESCRIPTOR: &[u8] = b"vendor.xiaomi.hardware.displayfeature_aidl.IDisplayFeature\0";
    let Some(binder) = check_service(SERVICE, DESCRIPTOR, &DISPLAY_CLASS) else {
        return false;
    };

    let mut input: *mut AParcel = ptr::null_mut();
    let mut output: *mut AParcel = ptr::null_mut();
    let mut ok = unsafe { AIBinder_prepareTransaction(binder, &mut input) } == STATUS_OK;

    for value in [0, case_id, mode_id, cookie] {
        if ok {
            ok &= unsafe { AParcel_writeInt32(input, value) } == STATUS_OK;
        }
    }

    if ok {
        ok &= unsafe { AIBinder_transact(binder, 7, &mut input, &mut output, flags) } == STATUS_OK;
    } else if !input.is_null() {
        unsafe { AParcel_delete(input) };
    }

    if ok && !output.is_null() {
        let mut status: *mut AStatus = ptr::null_mut();
        ok &= unsafe { AParcel_readStatusHeader(output, &mut status) } == STATUS_OK;
        if !status.is_null() {
            ok &= unsafe { AStatus_isOk(status) };
            unsafe { AStatus_delete(status) };
        } else {
            ok = false;
        }
    } else {
        ok = false;
    }

    if !output.is_null() {
        unsafe { AParcel_delete(output) };
    }
    unsafe { AIBinder_decStrong(binder) };
    ok
}
