#!/usr/bin/env bash
# RamShared Auto-Deploy & Service Bootstrap on WSL2 Boot
set -euo pipefail

REPO_DIR="${RAMSHARED_REPO_DIR:-}"
if [[ -z "$REPO_DIR" || ! -d "$REPO_DIR" ]]; then
    if [[ -f "/etc/ramshared/repo.conf" ]]; then
        # shellcheck disable=SC1091
        source "/etc/ramshared/repo.conf"
    fi
fi
if [[ -z "$REPO_DIR" || ! -d "$REPO_DIR" ]]; then
    REPO_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." 2>/dev/null && pwd -P || true)"
fi
LOG_FILE="/var/log/ramshared/auto-deploy.log"

mkdir -p /var/log/ramshared
echo "=== RamShared Auto-Deploy Boot: $(date) ===" >> "$LOG_FILE"

# 1. Install updated binaries if present
if [[ -f "$REPO_DIR/target/release/ramshared" ]]; then
    echo "[+] Deploying target/release/ramshared..." >> "$LOG_FILE"
    cp -f "$REPO_DIR/target/release/ramshared" /usr/local/bin/ramshared
fi

if [[ -f "$REPO_DIR/target/release/ramsharedd" ]]; then
    echo "[+] Deploying target/release/ramsharedd..." >> "$LOG_FILE"
    cp -f "$REPO_DIR/target/release/ramsharedd" /usr/local/bin/ramsharedd
fi

if [[ -f "$REPO_DIR/packaging/scripts/ramshared-vram-service.sh" ]]; then
    echo "[+] Deploying packaging/scripts/ramshared-vram-service.sh..." >> "$LOG_FILE"
    cp -f "$REPO_DIR/packaging/scripts/ramshared-vram-service.sh" /usr/local/bin/ramshared-vram-service.sh
fi

chmod +x /usr/local/bin/ramshared* 2>/dev/null || true

# 2. Start/Restart the protected VRAM tier service
echo "[+] Starting RamShared VRAM Tier Service..." >> "$LOG_FILE"
/usr/local/bin/ramshared-vram-service.sh restart >> "$LOG_FILE" 2>&1 || true

echo "[+] Auto-deploy complete at $(date)" >> "$LOG_FILE"
