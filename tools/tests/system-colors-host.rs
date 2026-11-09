// Run the native cache/protocol tests on a host without linking Android or
// Flutter. The URL bridge is unrelated to palette transactions and is stubbed.
// backend_bridge also reads the activity-visibility gate from the crate root;
// tests always run as a visible app so the idle refresh path stays active.
#![allow(dead_code)]
fn dash_host_open_url_jni(_: i32) -> bool {
    false
}

fn app_visible() -> bool {
    true
}

#[path = "../../runtime/host-rust/src/backend_bridge.rs"]
mod backend_bridge;
