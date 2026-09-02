#!/usr/bin/env bash
# Long-lived systemd controller. Only this process receives systemd stop
# signals; the backend remains alive until swapoff and detach are proven.
set -euo pipefail

product_root=/opt/ramshared
selector="$product_root/current"
recovery_root=/mnt/c/ProgramData/RamShared/lifecycle-recovery
distro=${RAMSHARED_WSL_DISTRO:-Ubuntu-24.04}
retry_seconds=5
stop_requested=0
mode=plan
managed_device=

refuse() {
  printf 'NBD_CONTROLLER_STATE=REFUSED\n'
  printf 'NBD_CONTROLLER_REASON=%s\n' "$1"
  exit "${2:-1}"
}

check_pre_cascade_state() {
  command -v modprobe >/dev/null || refuse MODPROBE_MISSING 69
  modprobe -n zram 2>/dev/null || refuse ZRAM_MODULE_UNAVAILABLE 69
  local zram_exists=0
  for f in /sys/block/zram*; do
    [[ -e $f ]] && zram_exists=1 && break
  done
  (( zram_exists == 1 )) || refuse ZRAM_DEVICE_MISSING 69
  [[ -r /proc/swaps && ! -L /proc/swaps ]] || refuse SWAPS_UNREADABLE 69
}

[[ $distro =~ ^[A-Za-z0-9._-]+$ ]] || refuse DISTRO_INVALID
case $# in
  0)
    printf 'NBD_CONTROLLER_STATE=PLAN\n'
    printf 'NBD_CONTROLLER_CONTRACT=swapoff-before-backend-stop\n'
    exit 0
    ;;
  1)
    case $1 in
      --execute) mode=execute ;;
      --recover) mode=recover ;;
      *) refuse UNSUPPORTED_ARGUMENT ;;
    esac
    ;;
  *) refuse UNSUPPORTED_ARGUMENT ;;
esac

marker="$recovery_root/$distro.pending"

read_marker_value() {
  local key=$1 value
  [[ -f $marker && ! -L $marker ]] || refuse RECOVERY_MARKER_INVALID
  [[ $(stat -c '%s' -- "$marker" 2>/dev/null || printf 99999) -le 8192 ]] \
    || refuse RECOVERY_MARKER_INVALID
  value=$(awk -F= -v wanted="$key" '$1 == wanted { if (++seen > 1) exit 3; print substr($0, index($0, "=") + 1) } END { if (seen != 1) exit 4 }' "$marker") \
    || refuse RECOVERY_MARKER_INVALID
  printf '%s\n' "$value"
}

validate_marker_layout() {
  local marker_boot marker_phase marker_version marker_device
  [[ $(wc -l <"$marker") -eq 6 ]] || refuse RECOVERY_MARKER_INVALID
  [[ $(read_marker_value schema_version) == 1 ]] || refuse RECOVERY_MARKER_SCHEMA_INVALID
  [[ $(read_marker_value distro) == "$distro" ]] || refuse RECOVERY_MARKER_DISTRO_MISMATCH
  marker_version=$(read_marker_value release_version)
  marker_boot=$(read_marker_value boot_id)
  marker_phase=$(read_marker_value phase)
  marker_device=$(read_marker_value managed_device)
  [[ $marker_version =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]] \
    || refuse RECOVERY_MARKER_VERSION_INVALID
  [[ $marker_boot =~ ^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$ ]] \
    || refuse RECOVERY_MARKER_BOOT_ID_INVALID
  [[ $marker_phase =~ ^(starting|active|stopping|startup_failed|startup_identity_missing)$ ]] \
    || refuse RECOVERY_MARKER_PHASE_INVALID
  [[ -z $marker_device || $marker_device =~ ^/dev/nbd[0-9]+$ ]] \
    || refuse RECOVERY_MARKER_DEVICE_INVALID
}

if [[ $mode == recover ]]; then
  [[ -f $marker && ! -L $marker ]] || refuse RECOVERY_MARKER_MISSING
  validate_marker_layout
  version=$(read_marker_value release_version)
  managed_device=$(read_marker_value managed_device)
  release="$product_root/releases/$version"
else
  [[ -L $selector ]] || refuse RELEASE_SELECTOR_MISSING
  release=$(readlink -f -- "$selector" 2>/dev/null || true)
  [[ $release == "$product_root"/releases/* && -d $release ]] || refuse RELEASE_SELECTOR_INVALID
  version=${release##*/}
fi
[[ $release == "$product_root"/releases/* && -d $release ]] || refuse RELEASE_SELECTOR_INVALID
up="$release/scripts/safety/cascade-up.sh"
down="$release/scripts/safety/cascade-down.sh"
[[ -x $up && -x $down ]] || refuse RELEASE_LAYOUT_INVALID
if [[ $mode == recover ]]; then
  [[ ${RAMSHARED_NBD_CONTROLLER_APPROVAL:-} == "recover:$version" ]] || refuse APPROVAL_MISSING
else
  [[ ${RAMSHARED_NBD_CONTROLLER_APPROVAL:-} == "lifecycle:$version" ]] || refuse APPROVAL_MISSING
fi

write_marker() {
  install -d -m 0700 -- "$recovery_root"
  temporary="$marker.$$.tmp"
  printf 'schema_version=1\ndistro=%s\nrelease_version=%s\nboot_id=%s\nphase=%s\nmanaged_device=%s\n' \
    "$distro" "$version" "$(tr -d '[:space:]' </proc/sys/kernel/random/boot_id)" "$1" "$managed_device" >"$temporary"
  chmod 0600 -- "$temporary"
  mv -f -- "$temporary" "$marker"
}

managed_device_detached() {
  local pid_file device_pid
  [[ -n $managed_device ]] || return 0
  [[ $managed_device =~ ^/dev/nbd[0-9]+$ ]] || return 1
  pid_file="/sys/class/block/${managed_device##*/}/pid"
  [[ -e $pid_file ]] || return 0
  device_pid=$(tr -d '[:space:]' <"$pid_file" 2>/dev/null || true)
  [[ -z $device_pid || $device_pid == 0 ]]
}

clean_shutdown_proven() {
  strict_managed_swaps_absent || return 1
  [[ ! -e /run/ramshared/swap-dev ]] || return 1
  managed_device_detached || return 1
  [[ ! -s /run/ramshared/ramsharedd.pid ]] \
    || ! kill -0 "$(< /run/ramshared/ramsharedd.pid)" 2>/dev/null
}

strict_managed_swaps_absent() {
  [[ -r /proc/swaps && ! -L /proc/swaps ]] || return 1
  awk '
    NR == 1 {
      if (NF != 5 || $1 != "Filename" || $2 != "Type" || $3 != "Size" ||
          $4 != "Used" || $5 != "Priority") exit 2
      header = 1
      next
    }
    {
      if (NF != 5 || $2 !~ /^(file|partition)$/ || $3 !~ /^[0-9]+$/ ||
          $4 !~ /^[0-9]+$/ || $5 !~ /^-?[0-9]+$/ || seen[$1]++) exit 2
      if ($1 ~ /^\/?(dev\/)?(nbd[0-9]+|ublkb[0-9]+|zram[0-9]+)$/) active = 1
    }
    END {
      if (!header || active) exit 1
    }
  ' /proc/swaps
}

finish_stop() {
  trap '' TERM INT
  write_marker stopping
  while ! RAMSHARED_NBD_LIFECYCLE_APPROVAL="deactivate:$version" "$down" --execute; do
    printf 'NBD_CONTROLLER_STATE=WAITING_FOR_SWAPOFF\n' >&2
    sleep "$retry_seconds"
  done
  while ! clean_shutdown_proven; do
    printf 'NBD_CONTROLLER_STATE=WAITING_FOR_CLEAN_PROOF\n' >&2
    sleep "$retry_seconds"
  done
  rm -f -- "$marker"
  printf 'NBD_CONTROLLER_STATE=STOPPED_CLEAN\n'
  exit 0
}

trap 'stop_requested=1' TERM INT

install -d -m 0755 -- /run/ramshared
exec 9>/run/ramshared/cascade-controller.lock
flock -n 9 || refuse CONTROLLER_ALREADY_RUNNING

if [[ $mode == recover ]]; then
  validate_marker_layout
  [[ $(read_marker_value release_version) == "$version" ]] || refuse RECOVERY_MARKER_VERSION_MISMATCH
  managed_device=$(read_marker_value managed_device)
  finish_stop
fi

# A controller crash or restart may leave a live origin behind. Never start a
# second lifecycle over that evidence: finish the old teardown first and leave
# reactivation to a later, explicit start.
if [[ -e $marker ]]; then
  [[ -f $marker && ! -L $marker ]] || refuse RECOVERY_MARKER_INVALID
  validate_marker_layout
  [[ $(read_marker_value release_version) == "$version" ]] || refuse RECOVERY_MARKER_VERSION_MISMATCH
  managed_device=$(read_marker_value managed_device)
  finish_stop
fi

if [[ $mode == execute ]]; then
  check_pre_cascade_state
fi

write_marker starting
if ! RAMSHARED_NBD_LIFECYCLE_APPROVAL="activate:$version" "$up" --execute; then
  write_marker startup_failed
  if RAMSHARED_NBD_LIFECYCLE_APPROVAL="deactivate:$version" "$down" --execute \
    && clean_shutdown_proven
  then
    rm -f -- "$marker"
  fi
  exit 1
fi
managed_device=$(tr -d '[:space:]' </run/ramshared/swap-dev 2>/dev/null || true)
[[ $managed_device =~ ^/dev/nbd[0-9]+$ ]] || {
  write_marker startup_identity_missing
  finish_stop
}
write_marker active

while (( stop_requested == 0 )); do
  sleep 30 &
  wait $! || true
done
finish_stop
