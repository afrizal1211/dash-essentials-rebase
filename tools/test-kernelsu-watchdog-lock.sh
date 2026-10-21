#!/usr/bin/env bash
set -euo pipefail

DASH_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DASH_SERVICE_SOURCE="$DASH_PROJECT_ROOT/android/kernelsu-next/service.sh"
DASH_TEST_ROOT="$(mktemp -d)"
DASH_TEST_RUNNER=""

dash_test_cleanup() {
    if [ -n "$DASH_TEST_RUNNER" ]; then
        kill "$DASH_TEST_RUNNER" 2>/dev/null || true
        wait "$DASH_TEST_RUNNER" 2>/dev/null || true
    fi
    rm -rf -- "$DASH_TEST_ROOT"
}
trap dash_test_cleanup EXIT

DASH_TEST_MODULE="$DASH_TEST_ROOT/nees_dash_essential_backend"
DASH_TEST_STATE="$DASH_TEST_ROOT/state"
DASH_TEST_BOOT_ID="$DASH_TEST_ROOT/boot_id"
mkdir -p "$DASH_TEST_MODULE" "$DASH_TEST_STATE/watchdog.lock"

sed \
    -e "s|DASH_ROOT=/data/adb/dash-essential|DASH_ROOT=$DASH_TEST_STATE|" \
    -e "s|/proc/sys/kernel/random/boot_id|$DASH_TEST_BOOT_ID|" \
    "$DASH_SERVICE_SOURCE" >"$DASH_TEST_MODULE/service.sh"
chmod 0755 "$DASH_TEST_MODULE/service.sh"
printf '%s\n' 'current-test-boot' >"$DASH_TEST_BOOT_ID"

# Reproduce the field failure: a stale lock from the previous boot contains
# the same numeric PID assigned to the newly launched service process.
(
    printf '%s\n' "$BASHPID" >"$DASH_TEST_STATE/watchdog.lock/pid"
    printf '%s\n' 'previous-test-boot' >"$DASH_TEST_STATE/watchdog.lock/boot_id"
    exec bash "$DASH_TEST_MODULE/service.sh"
) &
DASH_TEST_RUNNER=$!

DASH_TEST_ATTEMPT=0
while [ "$DASH_TEST_ATTEMPT" -lt 50 ]; do
    DASH_TEST_ATTEMPT=$((DASH_TEST_ATTEMPT + 1))
    DASH_LOCK_PID="$(cat "$DASH_TEST_STATE/watchdog.lock/pid" 2>/dev/null || true)"
    DASH_LOCK_BOOT_ID="$(cat "$DASH_TEST_STATE/watchdog.lock/boot_id" 2>/dev/null || true)"
    if [ "$DASH_LOCK_PID" = "$DASH_TEST_RUNNER" ] \
        && [ "$DASH_LOCK_BOOT_ID" = 'current-test-boot' ]; then
        echo "WATCHDOG_PID_REUSE_TEST=PASS"
        exit 0
    fi
    sleep 0.02
done

echo "Watchdog did not reclaim a same-PID lock from the previous boot" >&2
exit 1
