#!/usr/bin/env bash
#
# The native palette tests, without the Flutter runtime.
#
# The native host library normally cannot be linked on a host: it expects the
# Android runtime. The test harness compiles it on its own with the JNI entry
# stubbed out, so the palette cache and protocol tests can run anywhere.
#
# The dependencies are taken from the workspace's dev profile, which is the
# profile the tests above were built with. The release profile aborts on
# panic, and a test harness needs to unwind.

set -euo pipefail

DASH_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_DEPS="$DASH_ROOT/target/debug/deps"
DASH_TEST_DIR="$(mktemp -d -t dash-system-colors.XXXXXXXX)"
trap 'rm -f "$DASH_TEST_DIR/host-tests"; rmdir "$DASH_TEST_DIR"' EXIT

cd "$DASH_ROOT"
cargo test --manifest-path "$DASH_ROOT/Cargo.toml" \
    -p dash-essential-daemon system_colors::tests

DASH_SERDE_JSON="$(ls -1 "$DASH_DEPS"/libserde_json-*.rlib 2>/dev/null | head -1)"
if [ -z "$DASH_SERDE_JSON" ]; then
    echo "serde_json was not built for the dev profile" >&2
    echo "run the cargo test above before this script" >&2
    exit 1
fi

rustc --edition=2024 --test "$DASH_ROOT/tools/tests/system-colors-host.rs" \
    -L "dependency=$DASH_DEPS" \
    --extern "serde_json=$DASH_SERDE_JSON" \
    -o "$DASH_TEST_DIR/host-tests"
"$DASH_TEST_DIR/host-tests"

echo "SYSTEM_COLORS_NATIVE_TEST=PASS"
