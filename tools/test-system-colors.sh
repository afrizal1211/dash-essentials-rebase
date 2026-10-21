#!/usr/bin/env bash
set -euo pipefail

DASH_PALETTE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_PALETTE_TEST_DIR="$(mktemp -d -t dash-system-colors.XXXXXXXX)"
trap 'rm -f "$DASH_PALETTE_TEST_DIR/host-tests"; rmdir "$DASH_PALETTE_TEST_DIR"' EXIT

cargo test --manifest-path "$DASH_PALETTE_ROOT/Cargo.toml" \
    -p dash-essential-daemon system_colors::tests
rustc --edition=2024 --test "$DASH_PALETTE_ROOT/tools/tests/system-colors-host.rs" \
    -o "$DASH_PALETTE_TEST_DIR/host-tests"
"$DASH_PALETTE_TEST_DIR/host-tests"

cd "$DASH_PALETTE_ROOT/ui/flutter"
flutter test test/system_colors_test.dart test/system_colors_preview_test.dart \
    test/system_colors_monitor_test.dart test/backend_connection_test.dart
