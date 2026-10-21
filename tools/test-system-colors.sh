#!/usr/bin/env bash
set -euo pipefail

DASH_PALETTE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# The native half runs anywhere and is part of the module build's preflight.
"$DASH_PALETTE_ROOT/tools/test-system-colors-native.sh"

# The interface half needs the Flutter SDK and the packages resolved.
cd "$DASH_PALETTE_ROOT/ui/flutter"
flutter pub get
flutter test test/system_colors_test.dart test/system_colors_preview_test.dart \
    test/system_colors_monitor_test.dart test/backend_connection_test.dart

echo "SYSTEM_COLORS_TEST=PASS"
