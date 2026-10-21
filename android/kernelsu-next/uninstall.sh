#!/system/bin/sh

MODDIR=${0%/*}
DASH_ROOT=/data/adb/dash-essential
DASH_WATCHDOG_LOCK="$DASH_ROOT/watchdog.lock"
DASH_WATCHDOG_PID="$(cat "$DASH_ROOT/watchdog.pid" 2>/dev/null)"
DASH_PACKAGE=io.github.neeschal.dashessential
DASH_NATIVE_MODE=0
[ -f "$MODDIR/rom-native-mode" ] && DASH_NATIVE_MODE=1

case "$DASH_WATCHDOG_PID" in
    ''|*[!0-9]*) ;;
    *) kill "$DASH_WATCHDOG_PID" 2>/dev/null ;;
esac

for DASH_PROCESS_NAME in dash_daemon dash_essentiald; do
    for DASH_PID in $(pidof "$DASH_PROCESS_NAME" 2>/dev/null); do
        kill "$DASH_PID" 2>/dev/null
    done
done
sleep 1
for DASH_PROCESS_NAME in dash_daemon dash_essentiald; do
    for DASH_PID in $(pidof "$DASH_PROCESS_NAME" 2>/dev/null); do
        kill -9 "$DASH_PID" 2>/dev/null
    done
done

rm -f "$DASH_ROOT/watchdog.pid" "$DASH_WATCHDOG_LOCK/pid" "$DASH_WATCHDOG_LOCK/boot_id"
rmdir "$DASH_WATCHDOG_LOCK" 2>/dev/null
rm -f "$DASH_ROOT/bin/dash_daemon" "$DASH_ROOT/bin/dash_ctl"
rmdir "$DASH_ROOT/bin" 2>/dev/null
rm -f "$DASH_ROOT/backend.log" "$DASH_ROOT/backend.log.1"

if [ "$DASH_NATIVE_MODE" -eq 1 ]; then
    # The module APK is an Android package-manager update over the matching ROM
    # system app. Revert only that update, keep package data, and return daemon
    # ownership to init. The native state directory is never removed.
    /system/bin/cmd package uninstall-system-updates "$DASH_PACKAGE" \
        >"$DASH_ROOT/native-app-rollback.log" 2>&1 || true
    /system/bin/pm enable --user 0 "$DASH_PACKAGE" >/dev/null 2>&1 || true
    for DASH_NATIVE_SERVICE in dash_daemon dash_essentiald; do
        if [ -n "$(getprop "init.svc.$DASH_NATIVE_SERVICE" 2>/dev/null)" ]; then
            /system/bin/setprop ctl.start "$DASH_NATIVE_SERVICE" 2>/dev/null || true
        fi
    done
fi

# Standalone installations keep state.conf and the ordinary Android app so
# removing the root module does not erase user data. The app can be uninstalled
# normally from Android.
exit 0
