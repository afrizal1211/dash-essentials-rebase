#!/system/bin/sh

ui_print "***************************************"
ui_print "           Dash Essential"
ui_print "      KernelSU Next / Magisk"
ui_print "***************************************"

if [ "${KSU:-false}" = "true" ]; then
    DASH_MANAGER="KernelSU Next"
    DASH_MANAGER_VERSION="${KSU_VER:-unknown}"
elif [ -n "${MAGISK_VER_CODE:-}" ]; then
    DASH_MANAGER="Magisk"
    DASH_MANAGER_VERSION="${MAGISK_VER:-unknown}"
else
    abort "! Install from KernelSU Next Manager or the Magisk app"
fi

if [ "$BOOTMODE" != "true" ]; then
    abort "! Install from the root manager while Android is running"
fi

if [ "$ARCH" != "arm64" ]; then
    abort "! Dash Essential requires an arm64 device"
fi

DASH_PRODUCT="$(getprop ro.product.device)"
DASH_VENDOR="$(getprop ro.product.vendor.device)"
case "$DASH_PRODUCT $DASH_VENDOR" in
    *dash*) ;;
    *) abort "! Unsupported device: $DASH_PRODUCT / $DASH_VENDOR" ;;
esac

[ -f "$MODPATH/bin/dash_daemon" ] || abort "! Missing Dash daemon"
[ -f "$MODPATH/bin/dash_ctl" ] || abort "! Missing Dash control client"
[ -f "$MODPATH/app/DashEssential.apk" ] || abort "! Missing Dash Essential APK"
[ -x /system/bin/pm ] || abort "! Android package manager is unavailable"

DASH_PACKAGE=io.github.neeschal.dashessential

dash_native_backend_present() {
    for DASH_NATIVE_BINARY in \
        /product/bin/dash_daemon \
        /system_ext/bin/dash_daemon \
        /system/bin/dash_daemon \
        /vendor/bin/dash_daemon \
        /odm/bin/dash_daemon; do
        [ -x "$DASH_NATIVE_BINARY" ] && return 0
    done

    for DASH_NATIVE_SERVICE in dash_daemon dash_essentiald; do
        [ -n "$(getprop "init.svc.$DASH_NATIVE_SERVICE" 2>/dev/null)" ] && return 0
    done

    DASH_PACKAGE_PATHS="$(/system/bin/pm path "$DASH_PACKAGE" 2>/dev/null)"
    case "$DASH_PACKAGE_PATHS" in
        *package:/product/*|*package:/system_ext/*|*package:/system/*|\
        *package:/vendor/*|*package:/odm/*) return 0 ;;
    esac
    return 1
}

DASH_NATIVE_MODE=0
if dash_native_backend_present; then
    DASH_NATIVE_MODE=1
    touch "$MODPATH/rom-native-mode"
else
    rm -f "$MODPATH/rom-native-mode"
fi

# The module changes no system partition files, so KernelSU does not need a
# mounting metamodule. The APK remains an ordinary application; only the
# separate hardware daemon runs in the root manager domain.
touch "$MODPATH/skip_mount"
set_perm_recursive "$MODPATH" 0 0 0755 0644
set_perm_recursive "$MODPATH/bin" 0 0 0755 0755
set_perm_recursive "$MODPATH/app" 0 0 0755 0644
set_perm "$MODPATH/customize.sh" 0 0 0755
set_perm "$MODPATH/service.sh" 0 0 0755
set_perm "$MODPATH/action.sh" 0 0 0755
set_perm "$MODPATH/uninstall.sh" 0 0 0755
[ "$DASH_NATIVE_MODE" -eq 0 ] || set_perm "$MODPATH/rom-native-mode" 0 0 0644

if [ "$DASH_NATIVE_MODE" -eq 1 ]; then
    ui_print "- Compatible ROM-native installation detected"
    ui_print "- Installing the signed application update without clearing data"
else
    ui_print "- Installing the unprivileged Android app"
fi
DASH_INSTALL_RESULT="$(/system/bin/pm install --user 0 -r "$MODPATH/app/DashEssential.apk" 2>&1)"
case "$DASH_INSTALL_RESULT" in
    *Success*) ;;
    *INSTALL_FAILED_UPDATE_INCOMPATIBLE*)
        if [ "$DASH_NATIVE_MODE" -eq 1 ]; then
            ui_print "$DASH_INSTALL_RESULT"
            ui_print "! The ROM-native APK uses a different signing certificate"
            ui_print "! Android cannot safely update it with the public module APK"
            ui_print "! Update that ROM build with its original signing key"
            abort "! Android correctly refused a cross-signature system-app update"
        else
            ui_print "- Legacy Dash Essential signing certificate detected"
            ui_print "- Performing the one-time application signing migration"
            ui_print "- Hardware profiles under /data/adb remain unchanged"

            DASH_REMOVE_RESULT="$(/system/bin/pm uninstall "$DASH_PACKAGE" 2>&1)"
            case "$DASH_REMOVE_RESULT" in
                *Success*) ;;
                *)
                    ui_print "$DASH_REMOVE_RESULT"
                    abort "! Could not remove the legacy application"
                    ;;
            esac

            DASH_INSTALL_RESULT="$(/system/bin/pm install --user 0 "$MODPATH/app/DashEssential.apk" 2>&1)"
            case "$DASH_INSTALL_RESULT" in
                *Success*)
                    ui_print "- Signing migration completed"
                    ui_print "- Application-local preferences were reset once"
                    ;;
                *)
                    ui_print "$DASH_INSTALL_RESULT"
                    abort "! APK installation failed after signing migration"
                    ;;
            esac
        fi
        ;;
    *)
        ui_print "$DASH_INSTALL_RESULT"
        abort "! APK installation failed"
        ;;
esac

/system/bin/pm path "$DASH_PACKAGE" >/dev/null 2>&1 || \
    abort "! Android did not register Dash Essential"

DASH_EXPECTED_VERSION_CODE="$(sed -n 's/^versionCode=//p' "$MODPATH/module.prop" | head -n 1)"
DASH_INSTALLED_VERSION_CODE="$(/system/bin/dumpsys package "$DASH_PACKAGE" 2>/dev/null \
    | sed -n 's/.*versionCode=\([0-9][0-9]*\).*/\1/p' | head -n 1)"
[ -n "$DASH_EXPECTED_VERSION_CODE" ] || abort "! Module versionCode is missing"
[ "$DASH_INSTALLED_VERSION_CODE" = "$DASH_EXPECTED_VERSION_CODE" ] || \
    abort "! App/module version mismatch: $DASH_INSTALLED_VERSION_CODE / $DASH_EXPECTED_VERSION_CODE"

ui_print "- Device: $DASH_PRODUCT"
ui_print "- Root manager: $DASH_MANAGER $DASH_MANAGER_VERSION"
if [ "$DASH_NATIVE_MODE" -eq 1 ]; then
    ui_print "- Mode: update layer over the ROM-native installation"
    ui_print "- App: signature-compatible update; existing data retained"
    ui_print "- Daemon: module service takes over after native init stops"
    ui_print "- State: shared with the ROM-native backend"
    ui_print "- Removal: restores the ROM APK and native init service"
else
    ui_print "- App: installed as a normal Android application"
    ui_print "- Daemon: isolated privileged module service"
fi
ui_print "- IPC: authenticated Unix and privileged-loopback transports"
ui_print "- System overlay: none; no KernelSU metamodule required"
ui_print "- Reboot once when you are ready to activate the daemon"
