#!/usr/bin/env bash
set -euo pipefail

DASH_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_STAMP="$(date +%Y%m%d-%H%M%S)"
DASH_DESTINATION="${1:-$DASH_ROOT/dist/aosp/DashEssential-$DASH_STAMP}"
DASH_BUILD_DIR="$DASH_ROOT/out/aosp-export/$DASH_STAMP/build"
DASH_TEMPLATE="$DASH_ROOT/android/aosp"
DASH_SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Android/Sdk}}"
DASH_NDK_VERSION="$(find "$DASH_SDK_ROOT/ndk" -mindepth 1 -maxdepth 1 \
    -type d -printf '%f\n' | sort -V | tail -1)"
DASH_STRIP="$DASH_SDK_ROOT/ndk/$DASH_NDK_VERSION/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"

[ -n "${DASH_KEYSTORE:-}" ] || {
    echo "DASH_KEYSTORE is required for a reproducibly signed AOSP bundle." >&2
    echo "ROM maintainers must keep the same private key for every update." >&2
    exit 1
}
[ -f "$DASH_KEYSTORE" ] || {
    echo "DASH_KEYSTORE does not exist: $DASH_KEYSTORE" >&2
    exit 1
}

for DASH_TOOL in openssl readelf sha256sum unzip; do
    command -v "$DASH_TOOL" >/dev/null 2>&1 || {
        echo "Missing AOSP export tool: $DASH_TOOL" >&2
        exit 1
    }
done
[ -x "$DASH_STRIP" ] || {
    echo "Missing Android LLVM strip tool: $DASH_STRIP" >&2
    exit 1
}

"$DASH_ROOT/tools/test-aosp-integration.sh"
"$DASH_ROOT/tools/test-release-sync.sh"

if [ -e "$DASH_DESTINATION" ]; then
    echo "Destination already exists: $DASH_DESTINATION" >&2
    exit 1
fi

DASH_BUILD_ONLY=1 \
DASH_OUTPUT_DIR="$DASH_BUILD_DIR" \
    "$DASH_ROOT/build-and-install.sh"

DASH_APK="$DASH_BUILD_DIR/Dash-Essential.apk"
DASH_DAEMON="$DASH_BUILD_DIR/host-cargo/aarch64-linux-android/release/dash_daemon"
DASH_CTL="$DASH_BUILD_DIR/host-cargo/aarch64-linux-android/release/dash_ctl"
DASH_BUILD_TOOLS="$(find "$DASH_SDK_ROOT/build-tools" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort -V | tail -1)"
DASH_APKSIGNER="$DASH_SDK_ROOT/build-tools/$DASH_BUILD_TOOLS/apksigner"

[ -x "$DASH_APKSIGNER" ] || {
    echo "Missing Android APK signer: $DASH_APKSIGNER" >&2
    exit 1
}

for DASH_ARTIFACT in "$DASH_APK" "$DASH_DAEMON" "$DASH_CTL"; do
    [ -f "$DASH_ARTIFACT" ] || {
        echo "Missing AOSP artifact: $DASH_ARTIFACT" >&2
        exit 1
    }
done

mkdir -p "$DASH_DESTINATION/prebuilt" "$DASH_DESTINATION/docs"
cp -a "$DASH_TEMPLATE/Android.bp" "$DASH_DESTINATION/Android.bp"
cp -a "$DASH_TEMPLATE/dash-essential.mk" "$DASH_DESTINATION/dash-essential.mk"
cp -a "$DASH_TEMPLATE/BoardConfigDashEssential.mk" "$DASH_DESTINATION/BoardConfigDashEssential.mk"
cp -a "$DASH_TEMPLATE/dash_daemon.rc" "$DASH_DESTINATION/dash_daemon.rc"
cp -a "$DASH_TEMPLATE/sepolicy" "$DASH_DESTINATION/sepolicy"
cp -a "$DASH_TEMPLATE/README.md" "$DASH_DESTINATION/README.md"
cp -a "$DASH_ROOT/docs/ROM_INTEGRATION.md" "$DASH_DESTINATION/docs/ROM_INTEGRATION.md"
cp -a "$DASH_ROOT/docs/UNPACKED_ROM_INTEGRATION.md" "$DASH_DESTINATION/docs/UNPACKED_ROM_INTEGRATION.md"
cp -a "$DASH_ROOT/docs/UNPACKED_ROM_PORTER_GUIDE.md" "$DASH_DESTINATION/docs/UNPACKED_ROM_PORTER_GUIDE.md"
mkdir -p "$DASH_DESTINATION/tools"
cp -a "$DASH_ROOT/tools/prepare-aosp-touch-policy.py" "$DASH_DESTINATION/tools/prepare-aosp-touch-policy.py"
install -m 0644 "$DASH_APK" "$DASH_DESTINATION/prebuilt/DashEssential.apk"
install -m 0755 "$DASH_DAEMON" "$DASH_DESTINATION/prebuilt/dash_daemon"
install -m 0755 "$DASH_CTL" "$DASH_DESTINATION/prebuilt/dash_ctl"
"$DASH_STRIP" --strip-unneeded "$DASH_DESTINATION/prebuilt/dash_daemon"
"$DASH_STRIP" --strip-unneeded "$DASH_DESTINATION/prebuilt/dash_ctl"

"$DASH_APKSIGNER" verify --print-certs-pem "$DASH_APK" 2>/dev/null \
    | sed -n '/-----BEGIN CERTIFICATE-----/,/-----END CERTIFICATE-----/p' \
    >"$DASH_DESTINATION/prebuilt/DashEssential.x509.pem"
openssl x509 -in "$DASH_DESTINATION/prebuilt/DashEssential.x509.pem" \
    -noout -subject >/dev/null
DASH_APK_CERT_DIGEST="$("$DASH_APKSIGNER" verify --print-certs "$DASH_APK" \
    2>/dev/null | sed -n 's/^.*certificate SHA-256 digest: //p' | head -n 1 \
    | tr '[:upper:]' '[:lower:]')"
DASH_PEM_CERT_DIGEST="$(openssl x509 \
    -in "$DASH_DESTINATION/prebuilt/DashEssential.x509.pem" -outform DER \
    | sha256sum | awk '{print $1}')"
[[ "$DASH_APK_CERT_DIGEST" =~ ^[0-9a-f]{64}$ ]] || {
    echo "Unable to read the exported APK certificate" >&2
    exit 1
}
[ "$DASH_APK_CERT_DIGEST" = "$DASH_PEM_CERT_DIGEST" ] || {
    echo "Exported APK and SELinux public certificate do not match" >&2
    exit 1
}
openssl x509 -in "$DASH_DESTINATION/prebuilt/DashEssential.x509.pem" \
    -noout -subject -issuer -dates -fingerprint -sha256 \
    >"$DASH_DESTINATION/SIGNING-CERTIFICATE.txt"

ANDROID_SDK_ROOT="$DASH_SDK_ROOT" bash "$DASH_ROOT/tools/check-platform-dex.sh" "$DASH_DESTINATION/prebuilt/DashEssential.apk"

file "$DASH_DESTINATION/prebuilt/dash_daemon" | grep -q 'ARM aarch64'
file "$DASH_DESTINATION/prebuilt/dash_ctl" | grep -q 'ARM aarch64'

DASH_DAEMON_NEEDED="$(readelf -d "$DASH_DESTINATION/prebuilt/dash_daemon" \
    | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | LC_ALL=C sort)"
DASH_CTL_NEEDED="$(readelf -d "$DASH_DESTINATION/prebuilt/dash_ctl" \
    | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | LC_ALL=C sort)"
DASH_EXPECTED_DAEMON_NEEDED=$'libbinder_ndk.so\nlibc.so\nlibdl.so'
DASH_EXPECTED_CTL_NEEDED=$'libc.so\nlibdl.so'
[ "$DASH_DAEMON_NEEDED" = "$DASH_EXPECTED_DAEMON_NEEDED" ] || {
    echo "dash_daemon dependencies do not match Android.bp" >&2
    printf '%s\n' "$DASH_DAEMON_NEEDED" >&2
    exit 1
}
[ "$DASH_CTL_NEEDED" = "$DASH_EXPECTED_CTL_NEEDED" ] || {
    echo "dash_ctl dependencies do not match Android.bp" >&2
    printf '%s\n' "$DASH_CTL_NEEDED" >&2
    exit 1
}

(
    cd "$DASH_DESTINATION"
    find . -type f ! -name SHA256SUMS -print0 \
        | LC_ALL=C sort -z \
        | xargs -0 sha256sum \
        > SHA256SUMS
)

echo "AOSP_EXPORT=PASS"
echo "APK_CERT_SHA256=$DASH_APK_CERT_DIGEST"
echo "DIRECTORY=$DASH_DESTINATION"
