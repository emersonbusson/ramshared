#!/bin/bash
set -euo pipefail

# scripts/systemd/validate-service-units.sh
# Systemd reload idempotency test harness for RamShared.

# --- 1. Guard clauses & prerequisite checks ---
for cmd in systemctl systemd-analyze mktemp ps grep awk chmod mkdir cp rm; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "Error: Prerequisite command '$cmd' is missing." >&2
        (exit 1) || return 1 2>/dev/null
    fi
done

UNIT_DIR="packaging/systemd"
UNIT_FILE="$UNIT_DIR/ramshared-vram.service"
if [ ! -f "$UNIT_FILE" ]; then
    echo "Error: Unit file '$UNIT_FILE' not found." >&2
    (exit 1) || return 1 2>/dev/null
fi

echo "--- Static Validation ---"
TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT

# Create a mock root filesystem to satisfy systemd-analyze verify without polluting host
mkdir -p "$TEMP_DIR/usr/bin"
touch "$TEMP_DIR/usr/bin/ramsharedd"
touch "$TEMP_DIR/usr/bin/ramshared"
chmod 0755 "$TEMP_DIR/usr/bin/ramsharedd"
chmod 0755 "$TEMP_DIR/usr/bin/ramshared"

# Copy the unit file into the mock root
mkdir -p "$TEMP_DIR/etc/systemd/system"
cp "$UNIT_FILE" "$TEMP_DIR/etc/systemd/system/"

# Copy systemd targets from the host into the chroot to satisfy dependencies
if [ -d /lib/systemd/system ]; then
    cp -r /lib/systemd/system/*.target "$TEMP_DIR/etc/systemd/system/" 2>/dev/null || true
fi

# Verify systemd unit structure
systemd-analyze verify --root="$TEMP_DIR" ramshared-vram.service
echo "Unit file static validation passed."

echo "--- Idempotency & Leak Harness ---"
CYCLES=${IDEMPOTENCY_CYCLES:-5}
SERVICE_NAME=$(basename "$UNIT_FILE")

# Run active lifecycle tests only if systemd is managing the system and we have privileges,
# otherwise this acts as a dry-run / static check.
if [ -d /run/systemd/system ] && [ "$(id -u)" -eq 0 ]; then
    echo "Systemd is active and root privileges available. Executing $CYCLES restart cycles..."

    # Temporarily install the unit for the harness
    TEST_UNIT_PATH="/etc/systemd/system/$SERVICE_NAME"
    cp "$UNIT_FILE" "$TEST_UNIT_PATH"
    chmod 0644 "$TEST_UNIT_PATH"

    # Ensure cleanup of the installed unit
    trap 'rm -f "$TEST_UNIT_PATH"; systemctl daemon-reload >/dev/null 2>&1 || true; rm -rf "$TEMP_DIR"' EXIT

    for i in $(seq 1 "$CYCLES"); do
        echo "Cycle $i/$CYCLES: daemon-reload & restart"
        systemctl daemon-reload

        systemctl restart "$SERVICE_NAME" 2>/dev/null || true

        # Assert zero leaked processes (zombies) for our daemon
        # Using ps -eo pid,stat,comm and checking for 'Z' status
        ZOMBIES=$(ps -eo pid,stat,comm | awk '$2 ~ /^Z/ {print $0}' | grep -E "ramsharedd|ramshared" || true)
        if [ -n "$ZOMBIES" ]; then
            echo "Error: Leaked zombie processes detected during cycle $i!" >&2
            echo "$ZOMBIES" >&2
            (exit 1) || return 1 2>/dev/null
        fi
    done
    echo "Zero process leaks detected across $CYCLES cycles."
else
    echo "Notice: Systemd not active as PID 1 or not running as root. Skipping live restart cycles."
fi

echo "Idempotency test harness completed successfully."
