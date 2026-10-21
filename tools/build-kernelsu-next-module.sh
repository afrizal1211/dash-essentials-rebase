#!/usr/bin/env bash
set -euo pipefail

DASH_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_MODULE_SOURCE="$DASH_PROJECT_ROOT/android/kernelsu-next"
DASH_SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
DASH_STAMP="$(date +%Y%m%d-%H%M%S)"
DASH_BUILD_ROOT="$DASH_PROJECT_ROOT/out/kernelsu-next/$DASH_STAMP"
DASH_STAGE="$DASH_BUILD_ROOT/module"
DASH_APP_BUILD="$DASH_BUILD_ROOT/application"

DASH_VERSION="$(sed -n 's/^version=//p' "$DASH_MODULE_SOURCE/module.prop")"

# An official build must be signed with the release certificate recorded in
# android/package/release-cert.sha256. A build without that key is still
# useful, so it is allowed explicitly and is marked: the archive carries a
# `-dev` suffix and is never written to the stable module path, and it never
# shares a signature with a published release.
DASH_UNOFFICIAL_SIGNING="${DASH_ALLOW_UNOFFICIAL_SIGNING:-0}"
DASH_ZIP_SUFFIX=""
if [ "$DASH_UNOFFICIAL_SIGNING" = "1" ]; then
    DASH_ZIP_SUFFIX="-dev"
elif [ "$DASH_UNOFFICIAL_SIGNING" != "0" ]; then
    echo "DASH_ALLOW_UNOFFICIAL_SIGNING must be 0 or 1" >&2
    exit 1
fi

DASH_ZIP_NAME="Dash-Essential-KernelSU-Next-Magisk-${DASH_VERSION}${DASH_ZIP_SUFFIX}.zip"
DASH_ZIP="$DASH_BUILD_ROOT/$DASH_ZIP_NAME"
DASH_STABLE_ZIP="$DASH_PROJECT_ROOT/out/Dash-Essential-KernelSU-Next-Magisk${DASH_ZIP_SUFFIX}.zip"

[ -n "${DASH_KEYSTORE:-}" ] || {
    echo "DASH_KEYSTORE is required for a distributable root module." >&2
    echo "Use one persistent signing key for every module update." >&2
    exit 1
}

DASH_NDK_VERSION="$(find "$DASH_SDK_ROOT/ndk" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)"
DASH_NDK="$DASH_SDK_ROOT/ndk/$DASH_NDK_VERSION"
DASH_TOOLCHAIN="$DASH_NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
DASH_STRIP="$DASH_TOOLCHAIN/llvm-strip"
DASH_BUILD_TOOLS="$(find "$DASH_SDK_ROOT/build-tools" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)"
DASH_AAPT2="$DASH_SDK_ROOT/build-tools/$DASH_BUILD_TOOLS/aapt2"
DASH_APKSIGNER="$DASH_SDK_ROOT/build-tools/$DASH_BUILD_TOOLS/apksigner"
DASH_ZIPALIGN="$DASH_SDK_ROOT/build-tools/$DASH_BUILD_TOOLS/zipalign"

for DASH_TOOL in "$DASH_STRIP" "$DASH_AAPT2" "$DASH_APKSIGNER" "$DASH_ZIPALIGN"; do
    [ -x "$DASH_TOOL" ] || {
        echo "Missing Android build tool: $DASH_TOOL" >&2
        exit 1
    }
done

"$DASH_PROJECT_ROOT/tools/test-kernelsu-watchdog-lock.sh"
"$DASH_PROJECT_ROOT/tools/test-root-module-contract.sh"
"$DASH_PROJECT_ROOT/tools/test-release-sync.sh"

mkdir -p "$DASH_STAGE/bin" "$DASH_STAGE/app"

echo "===== DASH ESSENTIAL — KERNELSU NEXT + MAGISK MODULE ====="
echo "version=$DASH_VERSION"
echo "ndk=$DASH_NDK_VERSION"
echo "output=$DASH_ZIP"

DASH_BUILD_ONLY=1 \
DASH_OUTPUT_DIR="$DASH_APP_BUILD" \
    "$DASH_PROJECT_ROOT/build-and-install.sh"

DASH_APK="$DASH_APP_BUILD/Dash-Essential.apk"
DASH_DAEMON="$DASH_APP_BUILD/host-cargo/aarch64-linux-android/release/dash_daemon"
DASH_CTL="$DASH_APP_BUILD/host-cargo/aarch64-linux-android/release/dash_ctl"

for DASH_ARTIFACT in "$DASH_APK" "$DASH_DAEMON" "$DASH_CTL"; do
    [ -f "$DASH_ARTIFACT" ] || {
        echo "Missing release artifact: $DASH_ARTIFACT" >&2
        exit 1
    }
done

install -m 0644 "$DASH_MODULE_SOURCE/module.prop" "$DASH_STAGE/module.prop"
install -m 0755 "$DASH_MODULE_SOURCE/customize.sh" "$DASH_STAGE/customize.sh"
install -m 0755 "$DASH_MODULE_SOURCE/service.sh" "$DASH_STAGE/service.sh"
install -m 0755 "$DASH_MODULE_SOURCE/action.sh" "$DASH_STAGE/action.sh"
install -m 0755 "$DASH_MODULE_SOURCE/uninstall.sh" "$DASH_STAGE/uninstall.sh"
install -m 0644 "$DASH_MODULE_SOURCE/skip_mount" "$DASH_STAGE/skip_mount"
install -m 0644 "$DASH_APK" "$DASH_STAGE/app/DashEssential.apk"
install -m 0755 "$DASH_DAEMON" "$DASH_STAGE/bin/dash_daemon"
install -m 0755 "$DASH_CTL" "$DASH_STAGE/bin/dash_ctl"

"$DASH_STRIP" --strip-unneeded "$DASH_STAGE/bin/dash_daemon"
"$DASH_STRIP" --strip-unneeded "$DASH_STAGE/bin/dash_ctl"

for DASH_SCRIPT in customize.sh service.sh action.sh uninstall.sh; do
    bash -n "$DASH_STAGE/$DASH_SCRIPT"
    if LC_ALL=C grep -q $'\r' "$DASH_STAGE/$DASH_SCRIPT"; then
        echo "CRLF line endings are not allowed: $DASH_SCRIPT" >&2
        exit 1
    fi
done

grep -Fxq 'export DASH_REVERSE_IPC=1' "$DASH_STAGE/service.sh" || {
    echo "Missing authenticated reverse IPC activation" >&2
    exit 1
}
grep -Fxq 'export DASH_LOOPBACK_IPC=1' "$DASH_STAGE/service.sh" || {
    echo "Missing authenticated loopback IPC activation" >&2
    exit 1
}
grep -Fq 'touch_resampler_ready' "$DASH_STAGE/action.sh" || {
    echo "Missing v1.18.0 touch-resampler diagnostics" >&2
    exit 1
}
grep -Fq 'App IPC: verified (daemon-initiated)' "$DASH_STAGE/action.sh" || {
    echo "Module Action does not verify the Android app transport" >&2
    exit 1
}
grep -Fq 'App IPC: verified (authenticated loopback)' "$DASH_STAGE/action.sh" || {
    echo "Module Action does not report the authenticated loopback transport" >&2
    exit 1
}
grep -Fq 'DASH_MODULE_MODE rom_native_update' "$DASH_STAGE/service.sh" || {
    echo "Missing ROM-native daemon takeover path" >&2
    exit 1
}
grep -Fq 'uninstall-system-updates "$DASH_PACKAGE"' "$DASH_STAGE/uninstall.sh" || {
    echo "Missing ROM-native application rollback path" >&2
    exit 1
}

DASH_MODID="$(sed -n 's/^id=//p' "$DASH_STAGE/module.prop")"
DASH_VERSION_CODE="$(sed -n 's/^versionCode=//p' "$DASH_STAGE/module.prop")"
[[ "$DASH_MODID" =~ ^[a-zA-Z][a-zA-Z0-9._-]+$ ]] || {
    echo "Invalid KernelSU module id: $DASH_MODID" >&2
    exit 1
}
[[ "$DASH_VERSION_CODE" =~ ^[0-9]+$ ]] || {
    echo "Invalid KernelSU versionCode: $DASH_VERSION_CODE" >&2
    exit 1
}

DASH_APK_BADGING="$("$DASH_AAPT2" dump badging "$DASH_STAGE/app/DashEssential.apk")"
DASH_APK_HEADER="$(printf '%s\n' "$DASH_APK_BADGING" | sed -n '1p')"
DASH_APK_PACKAGE="$(printf '%s\n' "$DASH_APK_HEADER" | sed -n "s/^package: name='\([^']*\)'.*/\1/p")"
DASH_APK_VERSION_CODE="$(printf '%s\n' "$DASH_APK_HEADER" | sed -n "s/^package: name='[^']*' versionCode='\([^']*\)'.*/\1/p")"
DASH_APK_VERSION_NAME="$(printf '%s\n' "$DASH_APK_HEADER" | sed -n "s/^package: name='[^']*' versionCode='[^']*' versionName='\([^']*\)'.*/\1/p")"

[ "$DASH_APK_PACKAGE" = "io.github.neeschal.dashessential" ] || {
    echo "Unexpected APK package: $DASH_APK_PACKAGE" >&2
    exit 1
}
[ "$DASH_APK_VERSION_CODE" = "$DASH_VERSION_CODE" ] || {
    echo "APK/module versionCode mismatch: $DASH_APK_VERSION_CODE / $DASH_VERSION_CODE" >&2
    exit 1
}
[ "$DASH_APK_VERSION_NAME" = "${DASH_VERSION#v}" ] || {
    echo "APK/module version mismatch: $DASH_APK_VERSION_NAME / $DASH_VERSION" >&2
    exit 1
}

"$DASH_APKSIGNER" verify --verbose "$DASH_STAGE/app/DashEssential.apk" >/dev/null
DASH_CERT_DIGEST="$("$DASH_APKSIGNER" verify --print-certs \
    "$DASH_STAGE/app/DashEssential.apk" \
    | sed -n 's/^.*certificate SHA-256 digest: //p' | head -n 1 \
    | tr '[:upper:]' '[:lower:]')"
DASH_EXPECTED_CERT_DIGEST="$(tr -d '[:space:]:' \
    <"$DASH_PROJECT_ROOT/android/package/release-cert.sha256" \
    | tr '[:upper:]' '[:lower:]')"
[[ "$DASH_CERT_DIGEST" =~ ^[0-9a-f]{64}$ ]] || {
    echo "Unable to read APK signing certificate" >&2
    exit 1
}
if [ "$DASH_UNOFFICIAL_SIGNING" != "1" ]; then
    [ "$DASH_CERT_DIGEST" = "$DASH_EXPECTED_CERT_DIGEST" ] || {
        echo "Official release certificate mismatch" >&2
        echo "expected=$DASH_EXPECTED_CERT_DIGEST" >&2
        echo "actual=$DASH_CERT_DIGEST" >&2
        echo "Set DASH_ALLOW_UNOFFICIAL_SIGNING=1 to build a development-signed" >&2
        echo "archive that is marked as such and never published as a release." >&2
        exit 1
    }
else
    echo "Development build: the APK certificate is not the release certificate."
    echo "expected=$DASH_EXPECTED_CERT_DIGEST" >&2
    echo "actual=$DASH_CERT_DIGEST" >&2
fi
"$DASH_ZIPALIGN" -c -P 16 -v 4 "$DASH_STAGE/app/DashEssential.apk" >/dev/null
ANDROID_SDK_ROOT="$DASH_SDK_ROOT" bash "$DASH_PROJECT_ROOT/tools/check-platform-dex.sh" "$DASH_STAGE/app/DashEssential.apk"

file "$DASH_STAGE/bin/dash_daemon" | grep -q 'ARM aarch64'
file "$DASH_STAGE/bin/dash_ctl" | grep -q 'ARM aarch64'
readelf -lW "$DASH_STAGE/bin/dash_daemon" | grep -q '/system/bin/linker64'
readelf -lW "$DASH_STAGE/bin/dash_ctl" | grep -q '/system/bin/linker64'

for DASH_BINARY in "$DASH_STAGE/bin/dash_daemon" "$DASH_STAGE/bin/dash_ctl"; do
    DASH_LOAD_COUNT=0
    while read -r DASH_ALIGNMENT; do
        [ -n "$DASH_ALIGNMENT" ] || continue
        DASH_LOAD_COUNT=$((DASH_LOAD_COUNT + 1))
        if [ "$((DASH_ALIGNMENT))" -lt 16384 ]; then
            echo "ELF alignment below 16 KB: $DASH_BINARY ($DASH_ALIGNMENT)" >&2
            exit 1
        fi
    done < <(readelf -lW "$DASH_BINARY" | awk '$1 == "LOAD" { print $NF }')

    [ "$DASH_LOAD_COUNT" -gt 0 ] || {
        echo "No ELF LOAD segments found: $DASH_BINARY" >&2
        exit 1
    }
done

(
    cd "$DASH_STAGE"
    zip -X -9 "$DASH_ZIP" \
        module.prop customize.sh service.sh action.sh uninstall.sh skip_mount \
        app/DashEssential.apk bin/dash_daemon bin/dash_ctl >/dev/null
)

unzip -tq "$DASH_ZIP"
DASH_ENTRIES="$(unzip -Z1 "$DASH_ZIP")"
for DASH_REQUIRED in \
    module.prop customize.sh service.sh action.sh uninstall.sh skip_mount \
    app/DashEssential.apk bin/dash_daemon bin/dash_ctl; do
    echo "$DASH_ENTRIES" | grep -Fxq "$DASH_REQUIRED" || {
        echo "Missing module entry: $DASH_REQUIRED" >&2
        exit 1
    }
done

if echo "$DASH_ENTRIES" | grep -Eq '(^|/)system/'; then
    echo "Unexpected system overlay in the root module" >&2
    exit 1
fi
if echo "$DASH_ENTRIES" | grep -Eq '(^|/)(sepolicy\.rule|apply-sepolicy\.sh)$'; then
    echo "Unexpected SELinux patch payload in the root module" >&2
    exit 1
fi

DASH_STAGE_APK_SHA="$(sha256sum "$DASH_STAGE/app/DashEssential.apk" | awk '{print $1}')"
DASH_ZIP_APK_SHA="$(unzip -p "$DASH_ZIP" app/DashEssential.apk | sha256sum | awk '{print $1}')"
[ "$DASH_STAGE_APK_SHA" = "$DASH_ZIP_APK_SHA" ] || {
    echo "APK payload checksum mismatch" >&2
    exit 1
}

cp -f "$DASH_ZIP" "$DASH_STABLE_ZIP"
DASH_ZIP_SHA256="$(sha256sum "$DASH_ZIP" | awk '{print $1}')"
printf '%s  %s\n' "$DASH_ZIP_SHA256" "${DASH_ZIP##*/}" \
    | tee "$DASH_ZIP.sha256"

echo "ROOT_MODULE=PASS"
echo "MANAGERS=KernelSU Next, Magisk"
echo "APK_PACKAGE=$DASH_APK_PACKAGE"
echo "APK_VERSION=$DASH_APK_VERSION_NAME ($DASH_APK_VERSION_CODE)"
echo "APK_CERT_SHA256=$DASH_CERT_DIGEST"
echo "ZIP=$DASH_ZIP"
echo "STABLE_ZIP=$DASH_STABLE_ZIP"
