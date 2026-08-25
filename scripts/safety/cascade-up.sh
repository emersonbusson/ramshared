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

VRAM_OVERRIDE=""
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
  3)
    [[ $1 == --execute && $2 == --vram-mib ]] || refuse UNSUPPORTED_ARGUMENT
    [[ $3 =~ ^(1024|2048|4096)$ ]] || refuse BENCHMARK_SIZE_INVALID
    VRAM_OVERRIDE=$3
    ;;
  *) refuse UNSUPPORTED_ARGUMENT ;;
esac

VRAM_MIB=$(read_config_integer VRAM_MIB 4096)
ZRAM_MIB=$(read_config_integer ZRAM_MIB 1024)
EXPECTED_APPROVAL="activate:$RELEASE_VERSION"
if [[ -n $VRAM_OVERRIDE ]]; then
  VRAM_MIB=$VRAM_OVERRIDE
  [[ $ZRAM_MIB == 1024 ]] || refuse BENCHMARK_ZRAM_SIZE_INVALID
  EXPECTED_APPROVAL="activate:$RELEASE_VERSION:vram=$VRAM_MIB:zram=$ZRAM_MIB"
fi
[[ ${RAMSHARED_NBD_LIFECYCLE_APPROVAL:-} == "$EXPECTED_APPROVAL" ]] || refuse APPROVAL_MISSING
RAMSHARED_NBD_VRAM_MIB=$VRAM_MIB "$PREFLIGHT" --check

printf 'NBD_LIFECYCLE_STATE=EXECUTING\n'
printf 'NBD_LIFECYCLE_ACTION=activate\n'
printf 'NBD_LIFECYCLE_VERSION=%s\n' "$RELEASE_VERSION"
printf 'NBD_LIFECYCLE_VRAM_MIB=%s\n' "$VRAM_MIB"
printf 'NBD_LIFECYCLE_ZRAM_MIB=%s\n' "$ZRAM_MIB"
exec "$CLI" up --vram "$VRAM_MIB" --zram "$ZRAM_MIB" --daemon "$DAEMON" --transport nbd
