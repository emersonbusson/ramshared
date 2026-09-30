#!/usr/bin/env bash
# Read-only post-reboot identity and health checks for the custom WSL kernel.
set -euo pipefail
echo "=== uname ==="
uname -a
echo "=== build receipt ==="
cat /mnt/c/wsl/kernel-ramshared-v6.receipt 2>/dev/null || echo "receipt missing"
echo "=== expected sha256 ==="
echo "24ac89168096b1dfbd4fda18f19b2aebeeca9742d19ce808dfba4ea0b3be8b2a  /mnt/c/wsl/kernel-ramshared-v6"
sha256sum /mnt/c/wsl/kernel-ramshared-v6 2>/dev/null || true
echo "=== modules present ==="
ls /lib/modules/$(uname -r)/kernel/drivers/uio/ 2>/dev/null || true
ls /lib/modules/$(uname -r)/kernel/drivers/block/nbd.ko 2>/dev/null || true
echo "=== ramshared identity ==="
ramshared --version 2>/dev/null || true
ramshared status 2>/dev/null | head -12 || true
echo "=== swap ==="
cat /proc/swaps
echo "=== kernel log (boot) ==="
dmesg 2>/dev/null | grep -iE 'Linux version|Command line|uio_hv|dxg|vmbus' | head -20 || true
echo "=== done ==="
