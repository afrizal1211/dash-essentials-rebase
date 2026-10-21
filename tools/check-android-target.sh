#!/usr/bin/env bash
#
# Verify that the daemon builds for the target it actually ships on.
#
# The daemon has no build script and no native dependency, so the Android
# target can be checked without the NDK. Only the daemon is checked here: the
# native host library is built with the NDK by the module build, which covers
# it.
#
# This exists because the ordinary build compiles a materially smaller program
# than the shipped one. Anything the daemon keeps behind a platform gate is
# invisible to `cargo build` on a host, so a mistake in it is only found by a
# full Android build. Checking the target here turns that into a local,
# seconds-long failure instead of a remote one.

set -euo pipefail

DASH_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_TARGET="aarch64-linux-android"

command -v rustup >/dev/null 2>&1 || {
    echo "rustup is required to add the Android target" >&2
    exit 1
}

if ! rustup target list --installed | grep -Fxq "$DASH_TARGET"; then
    echo "Adding the $DASH_TARGET target"
    rustup target add "$DASH_TARGET"
fi

cd "$DASH_ROOT"
cargo check --release --target "$DASH_TARGET" -p dash-essential-daemon

echo "ANDROID_TARGET_CHECK=PASS ($DASH_TARGET)"
