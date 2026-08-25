#!/usr/bin/env bash
# Legacy installer retained only as an explicit disabled-staging boundary.
# It must not write system paths, load modules, reload a manager, or arrange
# boot/service activation.  A future attended transaction needs its own sealed
# implementation and approval; it is deliberately not provided here.
set -euo pipefail

printf 'RAMSHARED_LEGACY_INSTALL=STAGING_REFUSED\n'
printf 'RAMSHARED_LEGACY_INSTALL_REASON=direct_install_disabled_requires_future_sealed_attended_transaction\n'
printf '%s\n' 'No files, units, modules, or runtime state were changed.'
exit 2
