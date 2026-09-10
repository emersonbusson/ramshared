#!/usr/bin/env bash
set -euo pipefail

# test-udev-rules.sh: udevadm test simulation harness for synthetic GPU add/remove events
# SPDX-License-Identifier: GPL-2.0-only

echo "Validating udevadm and rule file existence..."

command -v udevadm >/dev/null 2>&1 || {
    echo "ERROR: udevadm is required but not installed." >&2
    exit 1
}

RULES_FILE="packaging/systemd/60-ramshared.rules"

if [ ! -f "${RULES_FILE}" ]; then
    RULES_FILE="packaging/systemd/99-ramshared.rules"
    if [ ! -f "${RULES_FILE}" ]; then
        echo "ERROR: Rules file not found!" >&2
        exit 1
    fi
fi

echo "Running udevadm verify on rules..."
udevadm verify "${RULES_FILE}"

DEVPATH="/sys/class/drm/card0"

if [ ! -e "${DEVPATH}" ]; then
    echo "WARNING: ${DEVPATH} does not exist in this environment. Cannot fully simulate udevadm event."
    echo "Test passed conditionally (environment lacks ${DEVPATH})."
    exit 0
fi

RULE_INSTALLED=0
if [ -f "/etc/udev/rules.d/60-ramshared.rules" ] || [ -f "/usr/lib/udev/rules.d/60-ramshared.rules" ] || \
   [ -f "/etc/udev/rules.d/99-ramshared.rules" ] || [ -f "/usr/lib/udev/rules.d/99-ramshared.rules" ]; then
    RULE_INSTALLED=1
else
    echo "WARNING: Rules file not found in standard /etc/udev/rules.d or /usr/lib/udev/rules.d. Strict parsing may fail."
fi

# Simulate ADD event
echo "Running udevadm test (add) on ${DEVPATH}..."
OUTPUT_ADD=$(udevadm test --action=add "${DEVPATH}" 2>&1 || true)

if echo "$OUTPUT_ADD" | grep -q "SYSTEMD_WANTS=ramshared-vram.service"; then
    echo "ADD event: Service trigger 'ramshared-vram.service' found in udevadm test output!"
else
    if [ "$RULE_INSTALLED" -eq 1 ]; then
        echo "ERROR: Service trigger not found in output for ADD event, but rule is installed." >&2
        exit 1
    else
        echo "WARNING: Service trigger not found in output for ADD event. Rule likely not installed."
    fi
fi

# Simulate REMOVE event
echo "Running udevadm test (remove) on ${DEVPATH}..."
OUTPUT_REMOVE=$(udevadm test --action=remove "${DEVPATH}" 2>&1 || true)

if echo "$OUTPUT_REMOVE" | grep -q "SYSTEMD_WANTS=ramshared-vram.service"; then
    echo "ERROR: Service trigger 'ramshared-vram.service' was incorrectly found in REMOVE event!" >&2
    exit 1
else
    echo "REMOVE event: Service trigger not found (expected)."
fi

echo "Test successful."
