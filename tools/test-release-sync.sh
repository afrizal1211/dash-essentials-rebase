#!/usr/bin/env bash
set -euo pipefail

DASH_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_MODULE="$DASH_ROOT/android/kernelsu-next/module.prop"
DASH_MANIFEST="$DASH_ROOT/android/package/AndroidManifest.xml"
DASH_FLUTTER="$DASH_ROOT/ui/flutter/pubspec.yaml"
DASH_MAIN="$DASH_ROOT/ui/flutter/lib/main.dart"
DASH_DAEMON="$DASH_ROOT/runtime/daemon-rust/src/lib.rs"
DASH_HOST="$DASH_ROOT/runtime/host-rust/src/backend_bridge.rs"

DASH_CARGO_VERSION="$(sed -n 's/^version = "\([0-9][0-9.]*\)"$/\1/p' \
    "$DASH_ROOT/Cargo.toml" | head -n 1)"
DASH_MODULE_VERSION="$(sed -n 's/^version=v//p' "$DASH_MODULE")"
DASH_MODULE_CODE="$(sed -n 's/^versionCode=//p' "$DASH_MODULE")"
DASH_MANIFEST_VERSION="$(sed -n 's/.*android:versionName="\([^"]*\)".*/\1/p' \
    "$DASH_MANIFEST")"
DASH_MANIFEST_CODE="$(sed -n 's/.*android:versionCode="\([^"]*\)".*/\1/p' \
    "$DASH_MANIFEST")"
DASH_FLUTTER_VERSION="$(sed -n 's/^version: \([^+]*\)+.*/\1/p' "$DASH_FLUTTER")"
DASH_FLUTTER_CODE="$(sed -n 's/^version: [^+]*+\([0-9]*\).*/\1/p' "$DASH_FLUTTER")"
DASH_CHANGELOG_VERSION="$(sed -n 's/^## \([0-9][0-9.]*\)$/\1/p' \
    "$DASH_ROOT/CHANGELOG.md" | head -n 1)"

for DASH_VERSION in \
    "$DASH_MODULE_VERSION" "$DASH_MANIFEST_VERSION" \
    "$DASH_FLUTTER_VERSION" "$DASH_CHANGELOG_VERSION"; do
    [ "$DASH_VERSION" = "$DASH_CARGO_VERSION" ] || {
        echo "Release version mismatch: $DASH_VERSION / $DASH_CARGO_VERSION" >&2
        exit 1
    }
done

for DASH_CODE in "$DASH_MODULE_CODE" "$DASH_MANIFEST_CODE" "$DASH_FLUTTER_CODE"; do
    [ "$DASH_CODE" = "$DASH_MODULE_CODE" ] || {
        echo "Release versionCode mismatch: $DASH_CODE / $DASH_MODULE_CODE" >&2
        exit 1
    }
done

IFS=. read -r DASH_MAJOR DASH_MINOR DASH_PATCH <<EOF
$DASH_CARGO_VERSION
EOF
DASH_EXPECTED_CODE="$(printf '%d%02d%02d' \
    "$DASH_MAJOR" "$DASH_MINOR" "$DASH_PATCH")"
[ "$DASH_MODULE_CODE" = "$DASH_EXPECTED_CODE" ] || {
    echo "Release versionCode is not derived from the semantic version" >&2
    exit 1
}

grep -Fq "const String dashVersionName = '$DASH_CARGO_VERSION';" \
    "$DASH_ROOT/ui/flutter/lib/release_identity.dart" && \
grep -Fq "const int dashVersionCode = $DASH_MODULE_CODE;" \
    "$DASH_ROOT/ui/flutter/lib/release_identity.dart" && \
grep -Fq "version: 'v\$dashVersionName'" "$DASH_MAIN" || {
    echo "In-app release label is not synchronized" >&2
    exit 1
}

DASH_DAEMON_SOCKET="$(sed -n 's/^pub const SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$DASH_DAEMON")"
DASH_HOST_SOCKET="$(sed -n 's/^const SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$DASH_HOST")"
DASH_DAEMON_REVERSE="$(sed -n 's/^pub const REVERSE_SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$DASH_DAEMON")"
DASH_HOST_REVERSE="$(sed -n 's/^const REVERSE_SOCKET_NAME: &str = "\([^"]*\)";/\1/p' \
    "$DASH_HOST")"
DASH_PROTOCOL="$(sed -n 's/^pub const PROTOCOL_VERSION: &str = "\([^"]*\)";/\1/p' \
    "$DASH_DAEMON")"

[ -n "$DASH_DAEMON_SOCKET" ] && [ "$DASH_DAEMON_SOCKET" = "$DASH_HOST_SOCKET" ] || {
    echo "App/daemon primary socket mismatch" >&2
    exit 1
}
[ -n "$DASH_DAEMON_REVERSE" ] && [ "$DASH_DAEMON_REVERSE" = "$DASH_HOST_REVERSE" ] || {
    echo "App/daemon reverse socket mismatch" >&2
    exit 1
}
grep -Fq "Some(\"$DASH_PROTOCOL\")" "$DASH_HOST" || {
    echo "App/daemon protocol mismatch" >&2
    exit 1
}

for DASH_PACKAGER in \
    "$DASH_ROOT/tools/build-kernelsu-next-module.sh" \
    "$DASH_ROOT/tools/export-aosp-bundle.sh"; do
    grep -Fq 'build-and-install.sh' "$DASH_PACKAGER"
    grep -Fq 'Dash-Essential.apk' "$DASH_PACKAGER"
    grep -Fq 'host-cargo/aarch64-linux-android/release/dash_daemon' "$DASH_PACKAGER"
    grep -Fq -- '--strip-debug' "$DASH_PACKAGER"
    if grep -Fq -- '--strip-unneeded' "$DASH_PACKAGER"; then
        echo "the symbol table must be kept: $DASH_PACKAGER" >&2
        exit 1
    fi
done

echo 'RELEASE_SYNC_CONTRACT_TEST=PASS'
