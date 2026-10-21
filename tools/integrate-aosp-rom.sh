#!/usr/bin/env bash
set -euo pipefail

DASH_PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

usage() {
    cat <<'EOF'
Usage:
  tools/integrate-aosp-rom.sh <aosp-root>
  tools/integrate-aosp-rom.sh <aosp-root> <product-makefile> <boardconfig>

Builds and verifies Dash Essential, then stages a self-contained integration
at vendor/dash-essential in an existing AOSP ROM source tree.

With the optional device-tree file arguments, the script also adds both exact
include lines idempotently. Paths may be absolute or relative to <aosp-root>.
With only <aosp-root>, it stages the bundle and prints the two lines.
EOF
}

if [ "$#" -ne 1 ] && [ "$#" -ne 3 ]; then
    usage >&2
    exit 2
fi

DASH_AOSP_ROOT="$(readlink -f "$1")"
DASH_DESTINATION="$DASH_AOSP_ROOT/vendor/dash-essential"

[ -f "$DASH_AOSP_ROOT/build/make/core/envsetup.mk" ] || {
    echo "Not an AOSP source root: $DASH_AOSP_ROOT" >&2
    exit 1
}

if [ -e "$DASH_DESTINATION" ]; then
    echo "Destination already exists: $DASH_DESTINATION" >&2
    echo "Remove or archive it explicitly before staging a replacement." >&2
    exit 1
fi

DASH_PRODUCT_INCLUDE='$(call inherit-product, vendor/dash-essential/dash-essential.mk)'
DASH_BOARD_INCLUDE='include vendor/dash-essential/BoardConfigDashEssential.mk'
DASH_WIRED=0

resolve_tree_file() {
    local DASH_REQUESTED="$1"
    local DASH_RESOLVED
    case "$DASH_REQUESTED" in
        /*) DASH_RESOLVED="$(readlink -f "$DASH_REQUESTED")" ;;
        *) DASH_RESOLVED="$(readlink -f "$DASH_AOSP_ROOT/$DASH_REQUESTED")" ;;
    esac
    case "$DASH_RESOLVED" in
        "$DASH_AOSP_ROOT"/*) ;;
        *)
            echo "Device-tree file is outside the AOSP root: $DASH_REQUESTED" >&2
            exit 1
            ;;
    esac
    [ -f "$DASH_RESOLVED" ] || {
        echo "Device-tree file does not exist: $DASH_RESOLVED" >&2
        exit 1
    }
    printf '%s\n' "$DASH_RESOLVED"
}

append_include_once() {
    local DASH_FILE="$1"
    local DASH_LINE="$2"
    if grep -Fxq "$DASH_LINE" "$DASH_FILE"; then
        return 0
    fi
    printf '\n# Dash Essential\n%s\n' "$DASH_LINE" >>"$DASH_FILE"
}

if [ "$#" -eq 3 ]; then
    DASH_PRODUCT_FILE="$(resolve_tree_file "$2")"
    DASH_BOARD_FILE="$(resolve_tree_file "$3")"
    for DASH_DEVICE_TREE_FILE in "$DASH_PRODUCT_FILE" "$DASH_BOARD_FILE"; do
        [ -w "$DASH_DEVICE_TREE_FILE" ] || {
            echo "Device-tree file is not writable: $DASH_DEVICE_TREE_FILE" >&2
            exit 1
        }
    done
fi

"$DASH_PROJECT_ROOT/tools/export-aosp-bundle.sh" "$DASH_DESTINATION"

if [ "$#" -eq 3 ]; then
    append_include_once "$DASH_PRODUCT_FILE" "$DASH_PRODUCT_INCLUDE"
    append_include_once "$DASH_BOARD_FILE" "$DASH_BOARD_INCLUDE"
    DASH_WIRED=1
fi

cat <<EOF

AOSP_INTEGRATION=PASS
STAGED_AT=$DASH_DESTINATION
DEVICE_TREE_WIRED=$DASH_WIRED
EOF

if [ "$DASH_WIRED" -eq 0 ]; then
    cat <<EOF
Add this line to the Dash product makefile:

  $DASH_PRODUCT_INCLUDE

Add this line to device/xiaomi/dash/BoardConfig.mk:

  $DASH_BOARD_INCLUDE
EOF
fi

cat <<EOF
Before building, review the native touch platform exception:

  python3 vendor/dash-essential/tools/prepare-aosp-touch-policy.py "$DASH_AOSP_ROOT"

This is a preview, not an automatic policy modification. Follow the staged
docs/ROM_INTEGRATION.md and apply only after maintainer review.
Then run the normal ROM build. For a focused validation first:

  m DashEssential dash_daemon dash_ctl selinux_policy
EOF
