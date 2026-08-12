#!/usr/bin/env bash
# Sealed NBD-only cascade activation. It plans unless a version-scoped approval
# explicitly requests execution; the preflight remains the authoritative
# read-only gate.
set -euo pipefail

PRODUCT_ROOT=/opt/ramshared
SELECTOR="$PRODUCT_ROOT/current"

refuse() {
  printf 'NBD_LIFECYCLE_STATE=REFUSED\n'
  printf 'NBD_LIFECYCLE_REASON=%s\n' "$1"
  exit 1
}

read_config_integer() {
  local key=$1 default=$2 value
  value=$(awk -F= -v key="$key" '
    $0 !~ /^[[:space:]]*#/ && $1 ~ "^[[:space:]]*" key "[[:space:]]*$" {
      candidate = $2
      sub(/^[[:space:]]*/, "", candidate)
      sub(/[[:space:]]*$/, "", candidate)
      print candidate
      exit
    }
  ' "$RELEASE/scripts/safety/cascade.conf.example")
  value=${value:-$default}
  [[ $value =~ ^[1-9][0-9]*$ ]] || refuse CONFIG_VALUE_INVALID
  printf '%s\n' "$value"
}

[[ -L $SELECTOR ]] || refuse RELEASE_SELECTOR_MISSING
RELEASE=$(readlink -f -- "$SELECTOR" 2>/dev/null || true)
[[ $RELEASE == "$PRODUCT_ROOT"/releases/* && -d $RELEASE ]] || refuse RELEASE_SELECTOR_INVALID
RELEASE_VERSION=${RELEASE##*/}
PREFLIGHT="$RELEASE/scripts/safety/nbd-product-preflight.sh"
CLI="$RELEASE/bin/ramshared"
DAEMON="$RELEASE/bin/ramsharedd"
[[ -x $PREFLIGHT && -x $CLI && -x $DAEMON ]] || refuse RELEASE_LAYOUT_INVALID

case $# in
  0)
    printf 'NBD_LIFECYCLE_STATE=PLAN\n'
    printf 'NBD_LIFECYCLE_ACTION=activate\n'
    printf 'NBD_LIFECYCLE_VERSION=%s\n' "$RELEASE_VERSION"
    printf 'NBD_LIFECYCLE_TRANSPORT=nbd\n'
    exit 0
    ;;
  1)
    [[ $1 == --execute ]] || refuse UNSUPPORTED_ARGUMENT
    ;;
  *) refuse UNSUPPORTED_ARGUMENT ;;
esac

[[ ${RAMSHARED_NBD_LIFECYCLE_APPROVAL:-} == "activate:$RELEASE_VERSION" ]] || refuse APPROVAL_MISSING
"$PREFLIGHT" --check

VRAM_MIB=$(read_config_integer VRAM_MIB 1024)
ZRAM_MIB=$(read_config_integer ZRAM_MIB 1024)
printf 'NBD_LIFECYCLE_STATE=EXECUTING\n'
printf 'NBD_LIFECYCLE_ACTION=activate\n'
printf 'NBD_LIFECYCLE_VERSION=%s\n' "$RELEASE_VERSION"
exec "$CLI" up --vram "$VRAM_MIB" --zram "$ZRAM_MIB" --daemon "$DAEMON" --transport nbd
