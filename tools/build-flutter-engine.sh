#!/usr/bin/env bash
set -euo pipefail

DASH_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_FLUTTER_BIN="$(command -v flutter 2>/dev/null || true)"

[ -n "$DASH_FLUTTER_BIN" ] || {
    echo "Flutter is not in PATH" >&2
    exit 1
}

DASH_FLUTTER_BIN="$(readlink -f "$DASH_FLUTTER_BIN")"
DASH_FLUTTER_ROOT="$(cd "$(dirname "$DASH_FLUTTER_BIN")/.." && pwd)"
DASH_ENGINE_SRC="${DASH_FLUTTER_ENGINE_SRC:-$DASH_FLUTTER_ROOT/engine/src}"
DASH_ENGINE_OUT="$DASH_ENGINE_SRC/out/android_release_arm64"
DASH_PREBUILT="$DASH_ROOT/runtime/flutter-engine/prebuilt/android-arm64"

[ -x "$DASH_ENGINE_SRC/flutter/tools/gn" ] || {
    echo "Flutter engine checkout not found: $DASH_ENGINE_SRC" >&2
    echo "Set DASH_FLUTTER_ENGINE_SRC to the engine src directory" >&2
    exit 1
}

command -v ninja >/dev/null 2>&1 || {
    echo "ninja is required" >&2
    exit 1
}

DASH_EXPECTED_REVISION="$(tr -d '[:space:]' <"$DASH_FLUTTER_ROOT/bin/internal/engine.version")"
DASH_ACTUAL_REVISION="$(git -C "$DASH_ENGINE_SRC" rev-parse HEAD 2>/dev/null || true)"

if [ -n "$DASH_ACTUAL_REVISION" ] && [ "$DASH_ACTUAL_REVISION" != "$DASH_EXPECTED_REVISION" ]; then
    echo "Flutter framework and engine revisions do not match" >&2
    echo "framework engine: $DASH_EXPECTED_REVISION" >&2
    echo "engine checkout:  $DASH_ACTUAL_REVISION" >&2
    exit 1
fi

(
    cd "$DASH_ENGINE_SRC"
    ./flutter/tools/gn \
        --android \
        --android-cpu arm64 \
        --runtime-mode release \
        --no-lto
)

ninja -C "$DASH_ENGINE_OUT" \
    flutter/shell/platform/embedder:flutter_engine_library

for DASH_ARTIFACT in \
    "$DASH_ENGINE_OUT/libflutter_engine.so" \
    "$DASH_ENGINE_OUT/icudtl.dat" \
    "$DASH_ENGINE_SRC/flutter/shell/platform/embedder/embedder.h"; do
    [ -f "$DASH_ARTIFACT" ] || {
        echo "Missing engine artifact: $DASH_ARTIFACT" >&2
        exit 1
    }
done

mkdir -p "$DASH_PREBUILT" "$DASH_ROOT/runtime/flutter-engine/include"
install -m 0644 "$DASH_ENGINE_OUT/libflutter_engine.so" "$DASH_PREBUILT/libflutter_engine.so"
install -m 0644 "$DASH_ENGINE_OUT/icudtl.dat" "$DASH_PREBUILT/icudtl.dat"
install -m 0644 \
    "$DASH_ENGINE_SRC/flutter/shell/platform/embedder/embedder.h" \
    "$DASH_ROOT/runtime/flutter-engine/include/embedder.h"

echo "FLUTTER_ENGINE_BUILD=PASS"
echo "ENGINE_REVISION=$DASH_EXPECTED_REVISION"
echo "PREBUILT_DIR=$DASH_PREBUILT"
