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
#
# Cargo moved the dev-profile artifacts from target/debug/deps to
# target/debug/build/<crate>/<hash>/out in 1.100, and the CI runners follow
# stable, so neither the rlib lookup nor the rustc library search may assume
# one layout. `find` is used instead of a glob because a glob that matches
# nothing makes ls exit 2, which under `set -o pipefail` aborts this script
# before the guard below can say why.

set -euo pipefail

DASH_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_DEBUG="$DASH_ROOT/target/debug"
DASH_DEPS="$DASH_DEBUG/deps"
DASH_TEST_DIR="$(mktemp -d -t dash-system-colors.XXXXXXXX)"
trap 'rm -f "$DASH_TEST_DIR/host-tests"; rmdir "$DASH_TEST_DIR"' EXIT

cd "$DASH_ROOT"
cargo test --manifest-path "$DASH_ROOT/Cargo.toml" \
    -p dash-essential-daemon system_colors::tests

DASH_SERDE_JSON_FILES=()
while IFS= read -r DASH_RLIB; do
    DASH_SERDE_JSON_FILES+=("$DASH_RLIB")
done < <(find "$DASH_DEBUG" -type f -name 'libserde_json-*.rlib' -print 2>/dev/null | sort)
DASH_SERDE_JSON="${DASH_SERDE_JSON_FILES[0]-}"
if [ -z "$DASH_SERDE_JSON" ]; then
    echo "serde_json was not built for the dev profile" >&2
    echo "run the cargo test above before this script" >&2
    exit 1
fi

# rustc resolves serde_json's own dependencies from the same directories cargo
# used, whatever their layout is.
DASH_RUSTC_LIBS=(-L "dependency=$DASH_DEPS")
while IFS= read -r DASH_OUT_DIR; do
    DASH_RUSTC_LIBS+=(-L "dependency=$DASH_OUT_DIR")
done < <(find "$DASH_DEBUG/build" -mindepth 3 -type d -name out -print 2>/dev/null | sort)

rustc --edition=2024 --test "$DASH_ROOT/tools/tests/system-colors-host.rs" \
    "${DASH_RUSTC_LIBS[@]}" \
    --extern "serde_json=$DASH_SERDE_JSON" \
    -o "$DASH_TEST_DIR/host-tests"
"$DASH_TEST_DIR/host-tests"

echo "SYSTEM_COLORS_NATIVE_TEST=PASS"
