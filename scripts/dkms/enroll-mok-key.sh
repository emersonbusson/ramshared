#!/usr/bin/env bash
set -euo pipefail

# Require root
if [[ "${EUID}" -ne 0 ]]; then
    echo "Error: This script must be run as root." >&2
    exit 1
fi

# Check prerequisites
for cmd in openssl mokutil; do
    if ! command -v "${cmd}" >/dev/null 2>&1; then
        echo "Error: Required command '${cmd}' not found." >&2
        exit 1
    fi
done

MOK_DIR="/var/lib/dkms/mok"
MOK_KEY="${MOK_DIR}/MOK.priv"
MOK_DER="${MOK_DIR}/MOK.der"

mkdir -p "${MOK_DIR}"
chmod 0700 "${MOK_DIR}"

if [[ -f "${MOK_KEY}" ]] && [[ -f "${MOK_DER}" ]]; then
    echo "MOK certificate already exists in ${MOK_DIR}."
else
    echo "Generating 2048-bit RSA X.509 MOK certificate..."

    TMP_CONF=$(mktemp)
    # Ensure temporary file is securely cleaned up even on failure
    trap 'rm -f "${TMP_CONF}"' EXIT
    chmod 0600 "${TMP_CONF}"

    cat << 'INNER_EOF' > "${TMP_CONF}"
[ req ]
default_bits = 2048
distinguished_name = req_distinguished_name
prompt = no
string_mask = utf8only
x509_extensions = myexts

[ req_distinguished_name ]
O = RamShared
CN = RamShared MOK
emailAddress = security@ramshared.local

[ myexts ]
basicConstraints = critical,CA:FALSE
keyUsage = digitalSignature
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid
INNER_EOF

    openssl req -x509 -new -nodes -utf8 -sha256 -days 3650 \
        -batch -config "${TMP_CONF}" \
        -outform DER -out "${MOK_DER}" \
        -keyout "${MOK_KEY}" >/dev/null 2>&1

    chmod 0600 "${MOK_KEY}"
    chmod 0644 "${MOK_DER}"

    rm -f "${TMP_CONF}"
    trap - EXIT

    echo "Certificate generated successfully."
fi

# Check if already enrolled
# mokutil --test-key requires root
if mokutil --test-key "${MOK_DER}" >/dev/null 2>&1; then
    echo "MOK certificate is already enrolled."
else
    echo "Enrolling MOK certificate..."
    echo "You will be prompted to create a password for MOK enrollment."
    echo "Please remember this password! You will need to enter it during the next boot in the UEFI MOKManager interface."

    mokutil --import "${MOK_DER}"

    echo "Enrollment staged. Please reboot your system to complete the enrollment process in MOKManager."
fi
