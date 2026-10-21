#!/system/bin/sh

MODDIR=${0%/*}
DASH_CTL="$MODDIR/bin/dash_ctl"

echo "Dash Essential"
echo ""

if [ -f "$MODDIR/rom-native-mode" ]; then
    echo "Backend mode: ROM-native update layer"
    echo "Native init service: $(getprop init.svc.dash_daemon 2>/dev/null)"
else
    echo "Backend mode: standalone root module"
fi

if /system/bin/pm path io.github.neeschal.dashessential >/dev/null 2>&1; then
    echo "App: installed"
else
    echo "App: missing; flash the module again from your root manager"
fi

DASH_PIDS="$(pidof dash_daemon 2>/dev/null)"
if [ -z "$DASH_PIDS" ]; then
    echo "Daemon: waiting for activation"
    echo "Install the module and reboot when you are ready."
    exit 0
fi

DASH_PING="$($DASH_CTL PING 2>&1)"
case "$DASH_PING" in
    "OK PONG "*)
        echo "Daemon: healthy"
        echo "Daemon PID: $DASH_PIDS"
        echo "Protocol: ${DASH_PING#OK PONG }"
        DASH_PRIMARY_PID="${DASH_PIDS%% *}"
        DASH_DAEMON_CONTEXT="$(cat "/proc/$DASH_PRIMARY_PID/attr/current" 2>/dev/null)"
        [ -n "$DASH_DAEMON_CONTEXT" ] && echo "Daemon SELinux: $DASH_DAEMON_CONTEXT"
        ;;
    *)
        echo "Daemon: running but did not answer"
        echo "$DASH_PING"
        exit 1
        ;;
esac

DASH_SNAPSHOT="$($DASH_CTL GET snapshot 2>/dev/null)"
DASH_APP_CLIENT_SEEN="$(printf '%s\n' "$DASH_SNAPSHOT" | tr ';' '\n' \
    | sed -n 's/^app_client_seen=//p' | head -n 1)"
DASH_APP_CLIENT_TRANSPORT="$(printf '%s\n' "$DASH_SNAPSHOT" | tr ';' '\n' \
    | sed -n 's/^app_client_transport=//p' | head -n 1)"
if [ "$DASH_APP_CLIENT_SEEN" = "1" ]; then
    case "$DASH_APP_CLIENT_TRANSPORT" in
        3) echo "App IPC: verified (authenticated loopback)" ;;
        2) echo "App IPC: verified (daemon-initiated)" ;;
        1) echo "App IPC: verified (direct policy)" ;;
        *) echo "App IPC: verified" ;;
    esac
else
    echo "App IPC: not verified yet; open Dash Essential once"
fi

echo "$DASH_SNAPSHOT" | tr ';' '\n' | grep -E '^(io|touch|touch_ack|touch_resampler_ready|perf)=' 2>/dev/null
