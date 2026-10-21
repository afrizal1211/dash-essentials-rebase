#!/usr/bin/env bash
set -euo pipefail

DASH_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_AOSP="$DASH_PROJECT_ROOT/android/aosp"
DASH_PRODUCT_PRIVATE="$DASH_AOSP/sepolicy/product/private"
DASH_VENDOR_POLICY="$DASH_AOSP/sepolicy/vendor/dash_daemon.te"
DASH_GENFS="$DASH_AOSP/sepolicy/vendor/genfs_contexts"

for DASH_REQUIRED in \
    Android.bp BoardConfigDashEssential.mk dash-essential.mk dash_daemon.rc \
    sepolicy/product/public/dash_daemon.te \
    sepolicy/product/private/file_contexts \
    sepolicy/product/private/keys.conf \
    sepolicy/product/private/mac_permissions.xml \
    sepolicy/product/private/dash_app.te \
    sepolicy/product/private/dash_daemon.te \
    sepolicy/product/private/seapp_contexts \
    sepolicy/vendor/genfs_contexts \
    sepolicy/vendor/dash_daemon.te; do
    [ -s "$DASH_AOSP/$DASH_REQUIRED" ] || {
        echo "Missing AOSP integration file: $DASH_REQUIRED" >&2
        exit 1
    }
done

grep -Fq 'presigned: true' "$DASH_AOSP/Android.bp"
grep -Fq 'preprocessed: true' "$DASH_AOSP/Android.bp"
grep -Fq 'dex_preopt:' "$DASH_AOSP/Android.bp"
grep -Fq '"libbinder_ndk"' "$DASH_AOSP/Android.bp"
if grep -Eq 'certificate:[[:space:]]*"platform"|check_elf_files:[[:space:]]*false' \
    "$DASH_AOSP/Android.bp"; then
    echo "AOSP prebuilts must retain their signing identity and ELF checks" >&2
    exit 1
fi

grep -Fq 'PRODUCT_PACKAGES_DEBUG +=' "$DASH_AOSP/dash-essential.mk"
if sed -n '/^PRODUCT_PACKAGES +=/,/^$/p' "$DASH_AOSP/dash-essential.mk" \
    | grep -Fq 'dash_ctl'; then
    echo "dash_ctl must not be installed in production user builds" >&2
    exit 1
fi

grep -Fxq '[@DASH_ESSENTIAL]' "$DASH_PRODUCT_PRIVATE/keys.conf"
grep -Fq 'vendor/dash-essential/prebuilt/DashEssential.x509.pem' \
    "$DASH_PRODUCT_PRIVATE/keys.conf"
grep -Fq '<signer signature="@DASH_ESSENTIAL">' \
    "$DASH_PRODUCT_PRIVATE/mac_permissions.xml"
grep -Fq '<package name="io.github.neeschal.dashessential">' \
    "$DASH_PRODUCT_PRIVATE/mac_permissions.xml"
grep -Fq '<seinfo value="dash_essential"' \
    "$DASH_PRODUCT_PRIVATE/mac_permissions.xml"
python3 - "$DASH_PRODUCT_PRIVATE/mac_permissions.xml" <<'PY'
import sys
import xml.etree.ElementTree as ET

root = ET.parse(sys.argv[1]).getroot()
if root.tag != "policy":
    raise SystemExit("mac_permissions.xml root must be <policy>")
PY

grep -Fxq \
    'user=_app seinfo=dash_essential name=io.github.neeschal.dashessential domain=dash_app type=app_data_file levelFrom=all' \
    "$DASH_PRODUCT_PRIVATE/seapp_contexts"
grep -Fq 'untrusted_app_domain(dash_app)' "$DASH_PRODUCT_PRIVATE/dash_app.te"
grep -Fq 'net_domain(dash_app)' "$DASH_PRODUCT_PRIVATE/dash_app.te"
grep -Fq 'typeattribute dash_app coredomain;' "$DASH_PRODUCT_PRIVATE/dash_app.te"
grep -Fq 'allow dash_app dash_daemon:unix_stream_socket connectto;' \
    "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq 'typeattribute dash_daemon mlstrustedsubject;' \
    "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq 'settings_service:service_manager find;' \
    "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq 'window_service:service_manager find;' \
    "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq 'activity_service:service_manager find;' \
    "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq 'overlay_service:service_manager find;' \
    "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq 'allow system_server dash_daemon:fd use;' \
    "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq 'allow system_server dash_daemon:fifo_file write;' \
    "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq 'toolbox_exec:file' "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq 'execute_no_trans' "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
if grep -Fq 'shell_exec:file' "$DASH_PRODUCT_PRIVATE/dash_daemon.te"; then
    echo "AOSP daemon must call cmd directly instead of executing shell wrappers" >&2
    exit 1
fi
if grep -Eq 'allow[[:space:]]+(appdomain|untrusted_app)[[:space:]]+dash_daemon' \
    "$DASH_PRODUCT_PRIVATE/dash_daemon.te"; then
    echo "AOSP socket access must remain restricted to dash_app" >&2
    exit 1
fi

if grep -Eq '(^|[[:space:]])permissive[[:space:]]+dash_daemon|allow[[:space:]]+dash_daemon[[:space:]]+sysfs:file|ctl_(start|stop)_prop' \
    "$DASH_AOSP"/sepolicy/{product/private,product/public,vendor}/*; then
    echo "AOSP policy contains an unsafe or obsolete grant" >&2
    exit 1
fi

# Traversal is required on the reported vendor base; broader parent access is not.
grep -Fxq 'allow dash_daemon sysfs_batteryinfo:dir search;' "$DASH_VENDOR_POLICY"
if grep '^[[:space:]]*allow .*sysfs_batteryinfo' "$DASH_VENDOR_POLICY" \
    | grep -Fvx 'allow dash_daemon sysfs_batteryinfo:dir search;'; then
    echo "Battery parent access must remain directory-search only" >&2
    exit 1
fi
grep -Fxq 'allow dash_daemon sysfs_battery_supply:dir search;' "$DASH_VENDOR_POLICY"
grep -Fxq 'allow dash_daemon sysfs_usb_supply:dir search;' "$DASH_VENDOR_POLICY"

# Installed-app discovery now needs lookup, not package mutation privileges.
grep -Fxq 'allow dash_daemon package_service:service_manager find;' "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fxq 'read_logd(dash_daemon)' "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
if grep 'package_service' "$DASH_PRODUCT_PRIVATE/dash_daemon.te" | grep -Fvx 'allow dash_daemon package_service:service_manager find;'; then
    echo "Package discovery must use only the scoped service lookup grant" >&2
    exit 1
fi

grep -Fq 'type sysfs_dash_mali_devfreq, fs_type, sysfs_type;' "$DASH_VENDOR_POLICY"
grep -Fq 'type sysfs_dash_touch, fs_type, sysfs_type;' "$DASH_VENDOR_POLICY"
grep -Fq 'allow dash_daemon sysfs_battery_supply:file rw_file_perms;' "$DASH_VENDOR_POLICY"
grep -Fq 'netlink_kobject_uevent_socket' "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq 'allow dash_daemon vendor_sysfs_displayfeature:file r_file_perms;' "$DASH_VENDOR_POLICY"
grep -Fq 'allow dash_daemon input_device:chr_file rw_file_perms;' "$DASH_VENDOR_POLICY"
grep -Fq 'binder_call(dash_daemon, hal_touchfeature_xiaomi_default)' "$DASH_VENDOR_POLICY"
grep -Fq 'binder_call(dash_daemon, vendor_hal_displayfeature_xiaomi_default)' \
    "$DASH_VENDOR_POLICY"
grep -Fq 'allow dash_daemon hal_touchfeature_xiaomi_service:service_manager find;' \
    "$DASH_VENDOR_POLICY"
grep -Fq 'allow dash_daemon hal_touchfeature_xiaomi_default:process ptrace;' "$DASH_VENDOR_POLICY"
grep -Fq 'allow dash_daemon hal_touchfeature_xiaomi_default:dir search;' "$DASH_VENDOR_POLICY"
grep -Fq 'typeattribute dash_daemon dash_native_touch_access;' "$DASH_PRODUCT_PRIVATE/dash_daemon.te"
grep -Fq '/devices/platform/soc/48000000.mali/devfreq/48000000.mali' "$DASH_GENFS"
grep -Fq '/devices/platform/goodix_ts.0' "$DASH_GENFS"

grep -Fq 'user root' "$DASH_AOSP/dash_daemon.rc"
grep -Fq 'setenv DASH_STATE_DIR /data/system/dash-essential' "$DASH_AOSP/dash_daemon.rc"
grep -Fq 'setenv DASH_TOUCH_DIRECT_INPUT 1' "$DASH_AOSP/dash_daemon.rc"
grep -Eq '^[[:space:]]*group.*input' "$DASH_AOSP/dash_daemon.rc"
grep -Fq 'capabilities DAC_OVERRIDE SYS_ADMIN SYS_NICE SYS_PTRACE' \
    "$DASH_AOSP/dash_daemon.rc"
if grep -Eq 'DAC_READ_SEARCH|readproc' \
    "$DASH_AOSP/dash_daemon.rc" "$DASH_PRODUCT_PRIVATE/dash_daemon.te"; then
    echo "AOSP daemon requests an unreviewed capability or process group" >&2
    exit 1
fi
grep -Fq 'mod touch_resampler;' "$DASH_PROJECT_ROOT/runtime/daemon-rust/src/lib.rs"
grep -Fq 'DASH_TOUCH_DIRECT_INPUT' \
    "$DASH_PROJECT_ROOT/runtime/daemon-rust/src/touch_resampler.rs"
grep -Fq 'on property:sys.boot_completed=1' "$DASH_AOSP/dash_daemon.rc"

DASH_PLAT_CIL="${DASH_PLAT_SEPOLICY_CIL:-}"
if [ -n "$DASH_PLAT_CIL" ]; then
    [ -f "$DASH_PLAT_CIL" ] || {
        echo "DASH_PLAT_SEPOLICY_CIL does not exist: $DASH_PLAT_CIL" >&2
        exit 1
    }
    command -v secilc >/dev/null 2>&1 || {
        echo "secilc is required for the requested platform neverallow check" >&2
        exit 1
    }
    DASH_CONTRACT_CIL="$DASH_PROJECT_ROOT/tools/aosp-policy-contract.cil"
    DASH_POLICY_TMP="$(mktemp -d)"
    trap 'rm -rf "$DASH_POLICY_TMP"' EXIT

    # An incomplete or failing baseline is not a successful policy check.
    # Never disable neverallows or excuse unknown diagnostics as OEM noise.
    secilc -m -M true -G -c 30 "$DASH_PLAT_CIL" \
        -o "$DASH_POLICY_TMP/baseline-policy" -f /dev/null
    secilc -m -M true -G -c 30 \
        "$DASH_PLAT_CIL" "$DASH_CONTRACT_CIL" \
        -o "$DASH_POLICY_TMP/combined-policy" -f /dev/null
    echo 'AOSP_PLATFORM_NEVERALLOW_TEST=PASS (synthetic probe only)'
else
    echo 'AOSP_PLATFORM_NEVERALLOW_TEST=SKIP (set DASH_PLAT_SEPOLICY_CIL)'
fi

echo 'AOSP_INTEGRATION_CONTRACT_TEST=PASS'
