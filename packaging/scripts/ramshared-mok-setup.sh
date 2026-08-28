#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-2.0-only
# RamShared Automated Machine Owner Key (MOK) Enrollment for UEFI Secure Boot
set -euo pipefail

MOK_DIR="/var/lib/dkms"
MOK_KEY="${MOK_DIR}/mok.key"
MOK_DER="${MOK_DIR}/mok.der"
MOK_PUB="${MOK_DIR}/mok.pub"

mkdir -p "${MOK_DIR}"
chmod 0700 "${MOK_DIR}"

if [[ ! -f "${MOK_KEY}" || ! -f "${MOK_DER}" ]]; then
    echo "==> [RamShared Security] Generating local X.509 MOK keypair for kernel module signing..."
    openssl req -new -x509 -newkey rsa:2048 -nodes \
        -keyout "${MOK_KEY}" -out "${MOK_PUB}" \
        -days 36500 -subj "/CN=RamShared DKMS Module Signing Key/" >/dev/null 2>&1
    openssl x509 -in "${MOK_PUB}" -outform DER -out "${MOK_DER}" >/dev/null 2>&1
    chmod 0600 "${MOK_KEY}"
    chmod 0644 "${MOK_PUB}" "${MOK_DER}"
fi

# Check if Secure Boot is active
if command -v mokutil >/dev/null 2>&1 && mokutil --sb-state 2>&1 | grep -q "SecureBoot enabled"; then
    echo "==> [RamShared Security] Secure Boot is ENABLED."
    if ! mokutil --test-key "${MOK_DER}" 2>&1 | grep -q "is already enrolled"; then
        echo "==> [RamShared Security] Staging MOK certificate for enrollment..."
        mokutil --import "${MOK_DER}" || true
    fi
fi
