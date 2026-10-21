#!/system/bin/sh

MODDIR=${0%/*}
DASH_ROOT=/data/adb/dash-essential
DASH_NATIVE_STATE_ROOT=/data/system/dash-essential
DASH_DAEMON="$MODDIR/bin/dash_daemon"
DASH_LOG="$DASH_ROOT/backend.log"
DASH_LOG_OLD="$DASH_ROOT/backend.log.1"
DASH_WATCHDOG_PID="$DASH_ROOT/watchdog.pid"
DASH_WATCHDOG_LOCK="$DASH_ROOT/watchdog.lock"
DASH_WATCHDOG_BOOT_ID="$DASH_WATCHDOG_LOCK/boot_id"
DASH_SERVICE_PATH="$MODDIR/service.sh"
DASH_CURRENT_BOOT_ID="$(cat /proc/sys/kernel/random/boot_id 2>/dev/null)"
DASH_PACKAGE=io.github.neeschal.dashessential

export DASH_REVERSE_IPC=1
export DASH_LOOPBACK_IPC=1

dash_native_backend_present() {
    [ -f "$MODDIR/rom-native-mode" ] && return 0

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
    return 1
}

DASH_NATIVE_MODE=0
if dash_native_backend_present; then
    DASH_NATIVE_MODE=1
    export DASH_STATE_DIR="$DASH_NATIVE_STATE_ROOT"
else
    export DASH_STATE_DIR="$DASH_ROOT"
fi

umask 077
mkdir -p "$DASH_ROOT"
chmod 0700 "$DASH_ROOT" 2>/dev/null
if [ "$DASH_NATIVE_MODE" -eq 1 ]; then
    mkdir -p "$DASH_NATIVE_STATE_ROOT"
    chmod 0700 "$DASH_NATIVE_STATE_ROOT" 2>/dev/null
    /system/bin/restorecon -RF "$DASH_NATIVE_STATE_ROOT" 2>/dev/null || \
        /system/bin/restorecon -R "$DASH_NATIVE_STATE_ROOT" 2>/dev/null || true
fi

DASH_LOG_BYTES=0
if [ -f "$DASH_LOG" ]; then
    DASH_LOG_BYTES="$(wc -c <"$DASH_LOG" 2>/dev/null)"
fi
case "$DASH_LOG_BYTES" in
    ''|*[!0-9]*) ;;
    *)
        if [ "$DASH_LOG_BYTES" -gt 1048576 ]; then
            mv -f "$DASH_LOG" "$DASH_LOG_OLD"
        fi
        ;;
esac

# mkdir is an atomic early-boot lock and prevents duplicate watchdog loops.
if ! mkdir "$DASH_WATCHDOG_LOCK" 2>/dev/null; then
    DASH_EXISTING_PID="$(cat "$DASH_WATCHDOG_LOCK/pid" 2>/dev/null)"
    DASH_EXISTING_BOOT_ID="$(cat "$DASH_WATCHDOG_BOOT_ID" 2>/dev/null)"
    DASH_EXISTING_CMDLINE=""
    if [ -n "$DASH_CURRENT_BOOT_ID" ] \
        && [ "$DASH_EXISTING_BOOT_ID" = "$DASH_CURRENT_BOOT_ID" ] \
        && [ "$DASH_EXISTING_PID" != "$$" ] \
        && kill -0 "$DASH_EXISTING_PID" 2>/dev/null; then
        DASH_EXISTING_CMDLINE="$(tr '\000' ' ' <"/proc/$DASH_EXISTING_PID/cmdline" 2>/dev/null)"
    fi
    case "$DASH_EXISTING_CMDLINE" in
        *"$DASH_SERVICE_PATH"*) exit 0 ;;
    esac

    rm -f "$DASH_WATCHDOG_LOCK/pid" "$DASH_WATCHDOG_BOOT_ID"
    rmdir "$DASH_WATCHDOG_LOCK" 2>/dev/null || exit 0
    mkdir "$DASH_WATCHDOG_LOCK" 2>/dev/null || exit 0
fi

printf '%s\n' "$$" >"$DASH_WATCHDOG_PID"
printf '%s\n' "$$" >"$DASH_WATCHDOG_LOCK/pid"
printf '%s\n' "$DASH_CURRENT_BOOT_ID" >"$DASH_WATCHDOG_BOOT_ID"

# Remove watchdogs left by older development packages of this same module.
for DASH_PROC in /proc/[0-9]*; do
    DASH_PID="${DASH_PROC##*/}"
    [ "$DASH_PID" = "$$" ] && continue
    DASH_CMDLINE="$(tr '\000' ' ' <"$DASH_PROC/cmdline" 2>/dev/null)"
    case "$DASH_CMDLINE" in
        *"$DASH_SERVICE_PATH"*)
            kill -9 "$DASH_PID" 2>/dev/null
            ;;
    esac
done

dash_cleanup() {
    DASH_OWNER="$(cat "$DASH_WATCHDOG_LOCK/pid" 2>/dev/null)"
    if [ "$DASH_OWNER" = "$$" ]; then
        rm -f "$DASH_WATCHDOG_PID" "$DASH_WATCHDOG_LOCK/pid" "$DASH_WATCHDOG_BOOT_ID"
        rmdir "$DASH_WATCHDOG_LOCK" 2>/dev/null
    fi
}

trap dash_cleanup EXIT
trap 'exit 0' HUP INT TERM

dash_stop_native_services() {
    [ "$DASH_NATIVE_MODE" -eq 1 ] || return 0

    for DASH_NATIVE_SERVICE in dash_daemon dash_essentiald; do
        DASH_NATIVE_STATE="$(getprop "init.svc.$DASH_NATIVE_SERVICE" 2>/dev/null)"
        case "$DASH_NATIVE_STATE" in
            ''|stopped) ;;
            *) /system/bin/setprop ctl.stop "$DASH_NATIVE_SERVICE" 2>/dev/null || true ;;
        esac
    done

    DASH_STOP_ATTEMPT=0
    while [ "$DASH_STOP_ATTEMPT" -lt 10 ]; do
        DASH_NATIVE_RUNNING=0
        for DASH_NATIVE_SERVICE in dash_daemon dash_essentiald; do
            DASH_NATIVE_STATE="$(getprop "init.svc.$DASH_NATIVE_SERVICE" 2>/dev/null)"
            case "$DASH_NATIVE_STATE" in
                starting|running|restarting|stopping) DASH_NATIVE_RUNNING=1 ;;
            esac
        done
        [ "$DASH_NATIVE_RUNNING" -eq 1 ] || return 0
        DASH_STOP_ATTEMPT=$((DASH_STOP_ATTEMPT + 1))
        sleep 1
    done
    return 1
}

dash_kill_existing_daemons() {
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
}

dash_resolve_app_uid() {
    DASH_PACKAGE_ENTRY="$(/system/bin/pm list packages -U "$DASH_PACKAGE" 2>/dev/null \
        | sed -n "s/^package:$DASH_PACKAGE[[:space:]]*uid:\([0-9][0-9]*\).*$/\1/p" \
        | head -n 1)"
    case "$DASH_PACKAGE_ENTRY" in
        ''|*[!0-9]*) ;;
        *)
            if [ "$DASH_PACKAGE_ENTRY" -ge 10000 ] 2>/dev/null; then
                printf '%s\n' "$DASH_PACKAGE_ENTRY"
                return 0
            fi
            ;;
    esac

    DASH_PACKAGE_ENTRY="$(/system/bin/dumpsys package "$DASH_PACKAGE" 2>/dev/null \
        | sed -n 's/.*userId=\([0-9][0-9]*\).*/\1/p' \
        | head -n 1)"
    case "$DASH_PACKAGE_ENTRY" in
        ''|*[!0-9]*) return 1 ;;
        *)
            [ "$DASH_PACKAGE_ENTRY" -ge 10000 ] 2>/dev/null || return 1
            printf '%s\n' "$DASH_PACKAGE_ENTRY"
            ;;
    esac
}

# KernelSU/Magisk late-start scripts may run while framework/vendor services
# are still publishing their boot defaults. Start the backend only after the
# completed Android boot so every persisted Dash setting wins that race.
while [ "$(getprop sys.boot_completed 2>/dev/null)" != "1" ]; do
    sleep 1
done

if [ "$DASH_NATIVE_MODE" -eq 1 ]; then
    echo "DASH_MODULE_MODE rom_native_update state_dir=$DASH_STATE_DIR" >>"$DASH_LOG"
    while ! dash_stop_native_services; do
        echo "DASH_NATIVE_TAKEOVER_WAIT init_service_still_running" >>"$DASH_LOG"
        sleep 2
    done
    echo "DASH_NATIVE_TAKEOVER_READY init_service=stopped" >>"$DASH_LOG"
else
    echo "DASH_MODULE_MODE standalone state_dir=$DASH_STATE_DIR" >>"$DASH_LOG"
fi

# Start the daemon shipped by this module, never an old module or init-owned
# binary. Native init is stopped first so it cannot restart a competing daemon.
dash_kill_existing_daemons

# Pin the daemon to the UID Android assigned to this exact APK. The daemon
# validates Unix peers with SO_PEERCRED and privileged-loopback peers against
# the kernel TCP table, so no app-to-root SELinux relaxation is required and no
# other ordinary application can issue hardware commands.
DASH_UID_WAIT_LOGGED=0
while true; do
    DASH_APP_UID="$(dash_resolve_app_uid)"
    if [ -n "$DASH_APP_UID" ]; then
        break
    fi
    if [ "$DASH_UID_WAIT_LOGGED" -eq 0 ]; then
        echo "DASH_MODULE_APP_UID_WAIT package=$DASH_PACKAGE" >>"$DASH_LOG"
        DASH_UID_WAIT_LOGGED=1
    fi
    sleep 2
done
export DASH_APP_UID
echo "DASH_MODULE_APP_UID uid=$DASH_APP_UID" >>"$DASH_LOG"

while true; do
    if [ "$DASH_NATIVE_MODE" -eq 1 ] && ! dash_stop_native_services; then
        echo "DASH_NATIVE_TAKEOVER_WAIT init_service_restarted" >>"$DASH_LOG"
        sleep 2
        continue
    fi
    if [ -x "$DASH_DAEMON" ] && ! pidof dash_daemon >/dev/null 2>&1; then
        echo "DASH_MODULE_DAEMON_START path=$DASH_DAEMON" >>"$DASH_LOG"
        "$DASH_DAEMON" >>"$DASH_LOG" 2>&1
        DASH_EXIT=$?
        echo "DASH_MODULE_DAEMON_EXIT code=$DASH_EXIT" >>"$DASH_LOG"
    fi
    sleep 2
done
