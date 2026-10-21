#!/usr/bin/env bash
set -euo pipefail

DASH_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_MODULE="$DASH_PROJECT_ROOT/android/kernelsu-next"
DASH_SERVICE="$DASH_MODULE/service.sh"
DASH_INSTALLER="$DASH_MODULE/customize.sh"
DASH_ACTION="$DASH_MODULE/action.sh"

for DASH_SCRIPT in customize.sh service.sh action.sh uninstall.sh; do
    bash -n "$DASH_MODULE/$DASH_SCRIPT"
    if LC_ALL=C grep -q $'\r' "$DASH_MODULE/$DASH_SCRIPT"; then
        echo "CRLF line endings are not allowed: $DASH_SCRIPT" >&2
        exit 1
    fi
done

[ -f "$DASH_MODULE/skip_mount" ] || {
    echo "Root module must remain script-only and skip system mounting" >&2
    exit 1
}

if find "$DASH_MODULE" -maxdepth 1 -type f \
    \( -name 'sepolicy.rule' -o -name 'apply-sepolicy.sh' \) | grep -q .; then
    echo "Root module must not depend on app-to-root SELinux relaxation" >&2
    exit 1
fi

if grep -ERq 'magiskpolicy|ksud[[:space:]]+sepolicy|allow[[:space:]]+appdomain' \
    "$DASH_MODULE"; then
    echo "Root module contains an obsolete SELinux patch path" >&2
    exit 1
fi

grep -Fxq 'export DASH_REVERSE_IPC=1' "$DASH_SERVICE"
grep -Fxq 'export DASH_LOOPBACK_IPC=1' "$DASH_SERVICE"
grep -Fq 'export DASH_APP_UID' "$DASH_SERVICE"
grep -Fq 'pm list packages -U' "$DASH_SERVICE"
grep -Fq 'sys.boot_completed' "$DASH_SERVICE"
grep -Fq 'DASH_NATIVE_STATE_ROOT=/data/system/dash-essential' "$DASH_SERVICE"
grep -Fq 'DASH_MODULE_MODE rom_native_update' "$DASH_SERVICE"
grep -Fq 'ctl.stop' "$DASH_SERVICE"
grep -Fq 'rom-native-mode' "$DASH_INSTALLER"
grep -Fq 'update layer over the ROM-native installation' "$DASH_INSTALLER"
grep -Fq 'uninstall-system-updates "$DASH_PACKAGE"' "$DASH_MODULE/uninstall.sh"
grep -Fq 'ctl.start' "$DASH_MODULE/uninstall.sh"
grep -Fq 'App IPC: verified (daemon-initiated)' "$DASH_ACTION"
grep -Fq 'App IPC: verified (authenticated loopback)' "$DASH_ACTION"
grep -Fq 'INSTALL_FAILED_UPDATE_INCOMPATIBLE' "$DASH_INSTALLER"
grep -Fq 'App/module version mismatch' "$DASH_INSTALLER"
grep -Fq 'touch_resampler_ready' "$DASH_ACTION"
[ -s "$DASH_PROJECT_ROOT/runtime/daemon-rust/src/touch_resampler.rs" ] || {
    echo "The v1.18.0 touch resampler source is missing" >&2
    exit 1
}

grep -Fq 'DASH_INSTALL_RESULT' "$DASH_INSTALLER"
grep -Fq 'INSTALL_FAILED_UPDATE_INCOMPATIBLE' "$DASH_INSTALLER"
grep -Fq '/system/bin/pm uninstall "$DASH_PACKAGE"' "$DASH_INSTALLER"
grep -Fq 'if [ "$DASH_NATIVE_MODE" -eq 1 ]' "$DASH_INSTALLER"
grep -Fq 'Application-local preferences were reset once' "$DASH_INSTALLER"

if grep -Fq '/system/bin/pm uninstall -k' "$DASH_INSTALLER"; then
    echo "Signature migration must not retain the obsolete package signing record" >&2
    exit 1
fi

if grep -Fq 'ROM-native Dash Essential detected; do not install' "$DASH_INSTALLER"; then
    echo "Installer still rejects ROM-native updates" >&2
    exit 1
fi

DASH_MODULE_VERSION="$(sed -n 's/^version=v//p' "$DASH_MODULE/module.prop")"
DASH_MODULE_CODE="$(sed -n 's/^versionCode=//p' "$DASH_MODULE/module.prop")"
DASH_MANIFEST_VERSION="$(sed -n 's/.*android:versionName="\([^"]*\)".*/\1/p' \
    "$DASH_PROJECT_ROOT/android/package/AndroidManifest.xml")"
DASH_MANIFEST_CODE="$(sed -n 's/.*android:versionCode="\([^"]*\)".*/\1/p' \
    "$DASH_PROJECT_ROOT/android/package/AndroidManifest.xml")"
DASH_FLUTTER_VERSION="$(sed -n 's/^version: \([^+]*\)+.*/\1/p' \
    "$DASH_PROJECT_ROOT/ui/flutter/pubspec.yaml")"
DASH_FLUTTER_CODE="$(sed -n 's/^version: [^+]*+\([0-9]*\).*/\1/p' \
    "$DASH_PROJECT_ROOT/ui/flutter/pubspec.yaml")"

for DASH_VERSION in "$DASH_MODULE_VERSION" "$DASH_MANIFEST_VERSION" "$DASH_FLUTTER_VERSION"; do
    [ "$DASH_VERSION" = "$DASH_MODULE_VERSION" ] || {
        echo "Application/module version mismatch" >&2
        exit 1
    }
done
for DASH_CODE in "$DASH_MODULE_CODE" "$DASH_MANIFEST_CODE" "$DASH_FLUTTER_CODE"; do
    [ "$DASH_CODE" = "$DASH_MODULE_CODE" ] || {
        echo "Application/module versionCode mismatch" >&2
        exit 1
    }
done

echo 'ROOT_MODULE_CONTRACT_TEST=PASS'
