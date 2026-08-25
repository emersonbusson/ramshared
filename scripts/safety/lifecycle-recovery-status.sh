#!/usr/bin/env bash
# Read-only proof consumed by the Windows recovery controller. It never starts,
# stops, formats, attaches, detaches, or signals a process.
set -euo pipefail

distro=${RAMSHARED_WSL_DISTRO:-Ubuntu-24.04}
test_root=${RAMSHARED_LIFECYCLE_TEST_ROOT:-}

refuse() {
  printf 'LIFECYCLE_RECOVERY_STATE=REFUSED\n'
  printf 'LIFECYCLE_RECOVERY_REASON=%s\n' "$1"
  exit 1
}

[[ $# -eq 0 ]] || refuse UNSUPPORTED_ARGUMENT
[[ $distro =~ ^[A-Za-z0-9._-]+$ ]] || refuse DISTRO_INVALID

if [[ -n $test_root ]]; then
  [[ $test_root == /* && -d $test_root && ! -L $test_root ]] || refuse TEST_ROOT_INVALID
  marker="$test_root/windows/lifecycle-recovery/$distro.pending"
  swaps="$test_root/proc/swaps"
  runtime="$test_root/run/ramshared"
  proc_root="$test_root/proc"
  sys_block="$test_root/sys/class/block"
else
  marker="/mnt/c/ProgramData/RamShared/lifecycle-recovery/$distro.pending"
  swaps=/proc/swaps
  runtime=/run/ramshared
  proc_root=/proc
  sys_block=/sys/class/block
fi

[[ -f $swaps && ! -L $swaps ]] || refuse SWAP_SNAPSHOT_UNAVAILABLE

marker_value() {
  local key=$1 value
  value=$(awk -F= -v wanted="$key" '$1 == wanted { if (++seen > 1) exit 3; print substr($0, index($0, "=") + 1) } END { if (seen != 1) exit 4 }' "$marker") \
    || refuse MARKER_INVALID
  printf '%s\n' "$value"
}

marker_present=0
marker_phase=absent
managed_device=
if [[ -e $marker ]]; then
  [[ -f $marker && ! -L $marker ]] || refuse MARKER_INVALID
  [[ $(stat -c '%s' -- "$marker" 2>/dev/null || printf 99999) -le 8192 ]] || refuse MARKER_INVALID
  [[ $(wc -l <"$marker") -eq 6 ]] || refuse MARKER_INVALID
  [[ $(marker_value schema_version) == 1 ]] || refuse MARKER_SCHEMA_INVALID
  [[ $(marker_value distro) == "$distro" ]] || refuse MARKER_DISTRO_MISMATCH
  version=$(marker_value release_version)
  boot_id=$(marker_value boot_id)
  marker_phase=$(marker_value phase)
  managed_device=$(marker_value managed_device)
  [[ $version =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]] || refuse MARKER_VERSION_INVALID
  [[ $boot_id =~ ^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$ ]] || refuse MARKER_BOOT_ID_INVALID
  [[ $marker_phase =~ ^(starting|active|stopping|startup_failed|startup_identity_missing)$ ]] \
    || refuse MARKER_PHASE_INVALID
  [[ -z $managed_device || $managed_device =~ ^/dev/nbd[0-9]+$ ]] || refuse MARKER_DEVICE_INVALID
  marker_present=1
fi

if [[ -z $managed_device && -f $runtime/swap-dev && ! -L $runtime/swap-dev ]]; then
  managed_device=$(tr -d '[:space:]' <"$runtime/swap-dev")
  [[ $managed_device =~ ^/dev/nbd[0-9]+$ ]] || refuse RUNTIME_DEVICE_INVALID
fi

managed_swap_count=$(awk 'NR > 1 && $1 ~ /^\/?(dev\/)?(nbd[0-9]+|ublkb[0-9]+|zram[0-9]+)([[:space:]]|$)/ { count++ } END { print count + 0 }' "$swaps")

daemon_running=0
if [[ -s $runtime/ramsharedd.pid && ! -L $runtime/ramsharedd.pid ]]; then
  daemon_pid=$(tr -d '[:space:]' <"$runtime/ramsharedd.pid")
  [[ $daemon_pid =~ ^[1-9][0-9]*$ ]] || refuse DAEMON_PID_INVALID
  [[ -f $proc_root/$daemon_pid/stat ]] && daemon_running=1
fi

device_attached=0
if [[ -n $managed_device ]]; then
  device_pid_file="$sys_block/${managed_device##*/}/pid"
  if [[ -e $device_pid_file ]]; then
    [[ -f $device_pid_file && ! -L $device_pid_file ]] || refuse DEVICE_PID_INVALID
    device_pid=$(tr -d '[:space:]' <"$device_pid_file")
    [[ -z $device_pid || $device_pid =~ ^[0-9]+$ ]] || refuse DEVICE_PID_INVALID
    [[ -n $device_pid && $device_pid != 0 ]] && device_attached=1
  fi
fi

printf 'LIFECYCLE_RECOVERY_MARKER=%s\n' "$marker_present"
printf 'LIFECYCLE_RECOVERY_PHASE=%s\n' "$marker_phase"
printf 'LIFECYCLE_RECOVERY_MANAGED_SWAP_COUNT=%s\n' "$managed_swap_count"
printf 'LIFECYCLE_RECOVERY_DAEMON_RUNNING=%s\n' "$daemon_running"
printf 'LIFECYCLE_RECOVERY_DEVICE_ATTACHED=%s\n' "$device_attached"

if (( marker_present == 0 && managed_swap_count == 0 && daemon_running == 0 && device_attached == 0 )) \
  && [[ ! -e $runtime/swap-dev ]]; then
  printf 'LIFECYCLE_RECOVERY_STATE=CLEAN\n'
  exit 0
fi

printf 'LIFECYCLE_RECOVERY_STATE=PENDING\n'
exit 2
