#!/usr/bin/env bash
#
# Install the pinned release-mode Flutter engine that Dash Essential links
# against.
#
# Dash Essential does not use the Flutter Android embedding. The APK is a
# zero-DEX shell that loads libflutter_engine.so, libapp.so and the custom
# embedder host from runtime/host-rust, so it needs the *embedder* library
# rather than the libflutter.so Flutter ships for Android applications.
#
# Flutter publishes release-mode embedder libraries only from an engine
# checkout (tools/build-flutter-engine.sh needs a ~25 GB flutter/engine tree);
# the SDK artifact service carries the debug embedder only
# (android-arm64/android-arm64-embedder.zip). This script instead reuses the
# exact release engine that upstream Rodin Essential v1.18.5 shipped in its
# KernelSU Next / Magisk module. That is the engine this tree's custom
# embedder was written against, and this port does not change the Flutter
# runtime, so the same pin applies.
#
# The SDK pin is baked into that binary, so it is verified rather than assumed:
#
#   framework revision  058e0af2c2b57e369d905a03ac9748b0ebf543c6  Flutter 3.44.8 (stable)
#   engine revision     0cd610717bde95fd88343c64f81c11ba4e5c0010  Flutter 3.44.8 engine
#   Dart revision       e9ed4fc9f1544c58d8a9347c1fc9471d8dd7c465  Dart 3.12.2
#
# runtime/flutter-engine/include/embedder.h is byte-identical to the
# flutter_embedder.h published for engine 0cd61071..., so the host in
# runtime/host-rust is ABI-compatible with this engine.
#
# Build the UI with Flutter 3.44.8. `flutter assemble` AOT-compiles libapp.so
# using that SDK's gen_snapshot, which must come from the same Dart revision as
# the engine or the snapshot is rejected at launch.
#
# Usage:
#   tools/fetch-flutter-engine-prebuilt.sh [--force]
#
# Environment:
#   DASH_ENGINE_MODULE_URL  override the module archive URL
#   DASH_ENGINE_ARCHIVE     use a local module archive instead of downloading
#   DASH_ENGINE_CACHE       download/extract cache directory
#   DASH_SKIP_ABI_PIN_CHECK set to 1 to skip the embedder.h digest check
set -euo pipefail

DASH_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_PREBUILT="$DASH_ROOT/runtime/flutter-engine/prebuilt/android-arm64"
DASH_HEADER="$DASH_ROOT/runtime/flutter-engine/include/embedder.h"
DASH_CACHE="${DASH_ENGINE_CACHE:-$DASH_ROOT/runtime/flutter-engine/.cache}"

# --- pinned runtime ----------------------------------------------------------
DASH_FLUTTER_VERSION="3.44.8"
DASH_FLUTTER_CHANNEL="stable"
DASH_FRAMEWORK_REVISION="058e0af2c2b57e369d905a03ac9748b0ebf543c6"
DASH_ENGINE_REVISION="0cd610717bde95fd88343c64f81c11ba4e5c0010"
DASH_DART_REVISION="e9ed4fc9f1544c58d8a9347c1fc9471d8dd7c465"
DASH_MODULE_DEFAULT_URL="https://github.com/NEESCHAL-3/Rodin-Essential/releases/download/v1.18.5/Rodin-Essential-KernelSU-Next-Magisk-v1.18.5.zip"
DASH_MODULE_SHA256="88670a54485f01ee55918cc388a47698a2bb19e87535a976ae796f0d7d988e37"
DASH_ENGINE_SHA256="3a8950c7c2f722a4971b418b7328fd6c44c517e62e9473ff3daca43b3c01f704"
DASH_ENGINE_BYTES="13239712"
DASH_ICU_SHA256="e4133e09a6a24c3c3ba33aec770713418c19133314dd93309ae1538f649d678b"
DASH_ICU_BYTES="9242032"
DASH_HEADER_SHA256="166626fb689d4e77e720c925f18e814a3cd55280999a443d9d1cc244384e37af"
DASH_HEADER_BYTES="168433"

DASH_FORCE=0
if [ "${1:-}" = "--force" ]; then
    DASH_FORCE=1
fi

digest() { sha256sum "$1" | awk '{print $1}'; }

# verify <file> <expected sha256> <expected bytes|""> <label>
verify() {
    local file="$1" want_sha="$2" want_bytes="$3" label="$4" bytes got
    if [ ! -f "$file" ]; then
        echo "$label is missing: $file" >&2
        return 1
    fi
    bytes="$(wc -c <"$file")"
    got="$(digest "$file")"
    if [ "$got" != "$want_sha" ] \
        || { [ -n "$want_bytes" ] && [ "$bytes" != "$want_bytes" ]; }; then
        echo "$label does not match the pinned runtime" >&2
        echo "  expected  ${want_bytes:-<any size>} bytes  $want_sha" >&2
        echo "  actual    $bytes bytes  $got" >&2
        return 1
    fi
}

echo "Dash Essential pinned Flutter runtime"
echo "  flutter=$DASH_FLUTTER_VERSION ($DASH_FLUTTER_CHANNEL)"
echo "  framework=$DASH_FRAMEWORK_REVISION"
echo "  engine=$DASH_ENGINE_REVISION"
echo "  dart=$DASH_DART_REVISION"
echo

if [ "$DASH_FORCE" = 0 ] \
    && verify "$DASH_PREBUILT/libflutter_engine.so" "$DASH_ENGINE_SHA256" "$DASH_ENGINE_BYTES" libflutter_engine.so 2>/dev/null \
    && verify "$DASH_PREBUILT/icudtl.dat" "$DASH_ICU_SHA256" "$DASH_ICU_BYTES" icudtl.dat 2>/dev/null; then
    echo "Prebuilt engine already installed and verified"
else
    mkdir -p "$DASH_CACHE" "$DASH_PREBUILT"

    if [ -n "${DASH_ENGINE_ARCHIVE:-}" ]; then
        DASH_ARCHIVE="$DASH_ENGINE_ARCHIVE"
        [ -f "$DASH_ARCHIVE" ] || {
            echo "DASH_ENGINE_ARCHIVE does not exist: $DASH_ARCHIVE" >&2
            exit 1
        }
    else
        DASH_MODULE_URL="${DASH_ENGINE_MODULE_URL:-$DASH_MODULE_DEFAULT_URL}"
        DASH_ARCHIVE="$DASH_CACHE/$(basename "$DASH_MODULE_URL")"
        if [ "$DASH_FORCE" = 1 ] || [ ! -f "$DASH_ARCHIVE" ]; then
            echo "Downloading $DASH_MODULE_URL"
            curl -fL --retry 3 --retry-delay 2 --connect-timeout 30 \
                -o "$DASH_ARCHIVE.part" "$DASH_MODULE_URL"
            mv -f "$DASH_ARCHIVE.part" "$DASH_ARCHIVE"
        fi
        verify "$DASH_ARCHIVE" "$DASH_MODULE_SHA256" "" "module archive" || {
            echo "Refusing to use an unverified module archive." >&2
            echo "Remove $DASH_ARCHIVE or pass DASH_ENGINE_ARCHIVE for your own copy." >&2
            exit 1
        }
    fi

    DASH_WORK="$(mktemp -d "$DASH_CACHE/extract.XXXXXX")"
    trap 'rm -rf "$DASH_WORK"' EXIT

    unzip -q -o "$DASH_ARCHIVE" -d "$DASH_WORK"
    DASH_APK="$(find "$DASH_WORK" -name '*.apk' -print -quit)"
    [ -n "$DASH_APK" ] || {
        echo "No APK payload inside $DASH_ARCHIVE" >&2
        exit 1
    }

    unzip -q -o "$DASH_APK" \
        'lib/arm64-v8a/libflutter_engine.so' 'assets/icudtl.dat' \
        -d "$DASH_WORK/apk"
    verify "$DASH_WORK/apk/lib/arm64-v8a/libflutter_engine.so" \
        "$DASH_ENGINE_SHA256" "$DASH_ENGINE_BYTES" libflutter_engine.so
    verify "$DASH_WORK/apk/assets/icudtl.dat" "$DASH_ICU_SHA256" "$DASH_ICU_BYTES" icudtl.dat

    install -m 0644 "$DASH_WORK/apk/lib/arm64-v8a/libflutter_engine.so" \
        "$DASH_PREBUILT/libflutter_engine.so"
    install -m 0644 "$DASH_WORK/apk/assets/icudtl.dat" "$DASH_PREBUILT/icudtl.dat"
    echo "Installed engine prebuilt into $DASH_PREBUILT"
fi

for DASH_NEEDLE in "$DASH_FRAMEWORK_REVISION" "$DASH_DART_REVISION"; do
    LC_ALL=C grep -aq "$DASH_NEEDLE" "$DASH_PREBUILT/libflutter_engine.so" || {
        echo "libflutter_engine.so does not carry $DASH_NEEDLE" >&2
        echo "The engine is not the Flutter $DASH_FLUTTER_VERSION build" >&2
        exit 1
    }
done
echo "ENGINE_REVISION_PIN=PASS"

if [ "${DASH_SKIP_ABI_PIN_CHECK:-0}" != "1" ]; then
    verify "$DASH_HEADER" "$DASH_HEADER_SHA256" "$DASH_HEADER_BYTES" embedder.h || {
        echo "runtime/flutter-engine/include/embedder.h no longer matches engine" >&2
        echo "$DASH_ENGINE_REVISION, which libflutter_engine.so was built for." >&2
        echo "Rebuild runtime/host-rust against the new header and refresh the" >&2
        echo "pin at the top of $0." >&2
        exit 1
    }
    echo "ABI_PIN=PASS ($DASH_ENGINE_REVISION)"
fi

echo
echo "FLUTTER_ENGINE_FETCH=PASS"
echo "FLUTTER_VERSION=$DASH_FLUTTER_VERSION"
echo "FLUTTER_CHANNEL=$DASH_FLUTTER_CHANNEL"
echo "FLUTTER_FRAMEWORK_REVISION=$DASH_FRAMEWORK_REVISION"
echo "FLUTTER_ENGINE_REVISION=$DASH_ENGINE_REVISION"
echo "DART_REVISION=$DASH_DART_REVISION"
echo "PREBUILT_DIR=$DASH_PREBUILT"
echo "LIBFLUTTER_ENGINE_SHA256=$DASH_ENGINE_SHA256"
echo "ICUDTL_SHA256=$DASH_ICU_SHA256"
