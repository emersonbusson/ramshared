#!/usr/bin/env bash
# Plan-first, transactional staging for the disabled WSL guest control plane.
set -euo pipefail

action=${1:-plan}
apply=${2:-}
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
source_dir="$root/scripts/safety/systemd"
[[ -d $source_dir ]] || source_dir="$root/systemd"
unit_dir=/etc/systemd/system
state_dir=/var/lib/ramshared/control-plane-install
meminfo=/proc/meminfo
backup_dir="$state_dir/backup"
manifest="$state_dir/manifest.tsv"
transaction="$state_dir/transaction.tsv"
manifest_magic=RAMSHARED_CONTROL_PLANE_MANIFEST_V1

units=(ramshared-control.slice ramshared-workloads.slice ramshared-workloads-docker.slice ramshared-workloads-cron.slice ramshared-host-gate.service ramshared-supervisor.service ramshared-cron-workload.service.in)
dropins=(docker.service.d/10-ramshared-control.conf containerd.service.d/10-ramshared-control.conf cron.service.d/10-ramshared-control.conf)
generated_workload_limits=ramshared-workloads.slice.d/10-ramshared-guest-memory.conf
managed_relatives=("${units[@]}" "${dropins[@]}" "$generated_workload_limits")

file_sha256() { sha256sum -- "$1" | awk '{print $1}'; }
file_metadata() { stat -c '%u:%g:%a' -- "$1"; }
valid_file_metadata() { [[ $1 =~ ^[0-9]+:[0-9]+:[0-7]{3,4}$ ]]; }
path_exists_or_link() { [[ -e $1 || -L $1 ]]; }

target_for_relative() {
  local relative=$1
  case $relative in
    ramshared-control.slice|ramshared-workloads.slice|ramshared-workloads-docker.slice|ramshared-workloads-cron.slice|ramshared-host-gate.service|ramshared-supervisor.service|ramshared-cron-workload.service.in|docker.service.d/10-ramshared-control.conf|containerd.service.d/10-ramshared-control.conf|cron.service.d/10-ramshared-control.conf|ramshared-workloads.slice.d/10-ramshared-guest-memory.conf) printf '%s/%s\n' "$unit_dir" "$relative" ;;
    *) return 1 ;;
  esac
}

relative_for_target() {
  local target=$1 relative
  for relative in "${managed_relatives[@]}"; do
    [[ $(target_for_relative "$relative") == "$target" ]] && { printf '%s\n' "$relative"; return 0; }
  done
  return 1
}

backup_path_for() { printf '%s/%s/%s\n' "$backup_dir" "$1" "${2//\//__}"; }
uninstall_backup_path_for() { printf '%s/%s/uninstall__%s\n' "$backup_dir" "$1" "${2//\//__}"; }

record_entry() {
  local receipt=$1 operation=$2 target=$3 backup=$4 backup_sha256=$5 backup_metadata=$6 installed_sha256=$7 installed_metadata=$8
  [[ $operation =~ ^(restore|remove)$ && $installed_sha256 =~ ^[[:xdigit:]]{64}$ ]] || return 1
  valid_file_metadata "$installed_metadata" || return 1
  if [[ $operation == restore ]]; then
    [[ $backup_sha256 =~ ^[[:xdigit:]]{64}$ ]] && valid_file_metadata "$backup_metadata" || return 1
  else
    [[ $backup == - && $backup_sha256 == - && $backup_metadata == - ]] || return 1
  fi
  printf 'entry\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$operation" "$target" "$backup" "$backup_sha256" "$backup_metadata" "${installed_sha256,,}" "$installed_metadata" >>"$receipt"
}

read_receipt_header() {
  local receipt=$1 header installation_id extra
  [[ -f $receipt && ! -L $receipt ]] || return 1
  IFS=$'\t' read -r header installation_id extra <"$receipt" || return 1
  [[ $header == "$manifest_magic" && $installation_id =~ ^[[:xdigit:]]{64}$ && -z ${extra:-} ]] || return 1
  printf '%s\n' "${installation_id,,}"
}

validate_receipt_structure() {
  local receipt=$1 require_complete=${2:-complete} installation_id line_kind operation target backup backup_sha256 backup_metadata installed_sha256 installed_metadata extra relative expected_backup seen_list='|' entry_count=0
  installation_id=$(read_receipt_header "$receipt") || return 1
  while IFS=$'\t' read -r line_kind operation target backup backup_sha256 backup_metadata installed_sha256 installed_metadata extra; do
    [[ $line_kind == "$manifest_magic" ]] && continue
    [[ $line_kind == entry && -n $operation && -n $target && -n $backup && -n $backup_sha256 && -n $backup_metadata && -n $installed_sha256 && -n $installed_metadata && -z ${extra:-} ]] || return 1
    relative=$(relative_for_target "$target") || return 1
    seen_list+="$relative|"
    ((entry_count += 1))
    case $operation in
      restore)
        [[ $installed_sha256 =~ ^[[:xdigit:]]{64}$ ]] && valid_file_metadata "$installed_metadata" || return 1
        expected_backup=$(backup_path_for "$installation_id" "$relative")
        [[ $backup == "$expected_backup" && $backup_sha256 =~ ^[[:xdigit:]]{64}$ ]] && valid_file_metadata "$backup_metadata" || return 1
        [[ -f $backup && ! -L $backup && $(file_sha256 "$backup") == "${backup_sha256,,}" && $(file_metadata "$backup") == "$backup_metadata" ]] || return 1
        ;;
      remove)
        [[ $installed_sha256 =~ ^[[:xdigit:]]{64}$ ]] && valid_file_metadata "$installed_metadata" || return 1
        [[ $backup == - && $backup_sha256 == - && $backup_metadata == - ]] || return 1
        ;;
      reinstall)
        expected_backup=$(uninstall_backup_path_for "$installation_id" "$relative")
        [[ $backup == "$expected_backup" && $backup_sha256 =~ ^[[:xdigit:]]{64}$ ]] && valid_file_metadata "$backup_metadata" || return 1
        [[ -f $backup && ! -L $backup && $(file_sha256 "$backup") == "${backup_sha256,,}" && $(file_metadata "$backup") == "$backup_metadata" ]] || return 1
        if [[ $installed_sha256 == - ]]; then
          [[ $installed_metadata == - ]] || return 1
        else
          [[ $installed_sha256 =~ ^[[:xdigit:]]{64}$ ]] && valid_file_metadata "$installed_metadata" || return 1
        fi
        ;;
      *) return 1 ;;
    esac
  done <"$receipt"
  [[ $require_complete == complete || $require_complete == partial ]] || return 1
  if [[ $require_complete == complete ]]; then
    ((entry_count == ${#managed_relatives[@]})) || return 1
    for relative in "${managed_relatives[@]}"; do [[ $seen_list == *"|$relative|"* ]] || return 1; done
  fi
}

target_matches() {
  [[ -f $1 && ! -L $1 && $(file_sha256 "$1") == "${2,,}" && $(file_metadata "$1") == "$3" ]]
}

validate_active_manifest() {
  local line_kind operation target backup backup_sha256 backup_metadata installed_sha256 installed_metadata extra
  validate_receipt_structure "$manifest" || return 1
  while IFS=$'\t' read -r line_kind operation target backup backup_sha256 backup_metadata installed_sha256 installed_metadata extra; do
    [[ $line_kind == entry ]] && target_matches "$target" "$installed_sha256" "$installed_metadata" || [[ $line_kind != entry ]] || return 1
  done <"$manifest"
}

compute_workload_limits() {
  local mem_total_kib quarter_kib reserve_kib max_kib margin_kib high_kib
  [[ -f $meminfo && ! -L $meminfo ]] || return 2
  mem_total_kib=$(awk '$1 == "MemTotal:" && $2 ~ /^[0-9]+$/ && $3 == "kB" { count++; value = $2 } END { if (count == 1) print value; else exit 1 }' "$meminfo") || return 2
  [[ $mem_total_kib =~ ^[0-9]+$ && $mem_total_kib -le 70368744177663 ]] || return 2
  quarter_kib=$(((mem_total_kib + 3) / 4)); reserve_kib=$quarter_kib
  (( reserve_kib < 4194304 )) && reserve_kib=4194304
  (( mem_total_kib > reserve_kib )) || return 1
  max_kib=$((mem_total_kib - reserve_kib)); margin_kib=$(((mem_total_kib + 9) / 10))
  (( margin_kib < 1048576 )) && margin_kib=1048576
  (( max_kib > margin_kib )) || return 1
  high_kib=$((max_kib - margin_kib))
  CONTROL_RESERVE_BYTES=$((reserve_kib * 1024)); WORKLOAD_MEMORY_MAX_BYTES=$((max_kib * 1024)); WORKLOAD_MEMORY_HIGH_BYTES=$((high_kib * 1024)); MEMTOTAL_KIB=$mem_total_kib
}

plan() {
  printf 'RAMSHARED_CONTROL_PLANE=PLAN\nACTION=%s\n' "$action"
  if compute_workload_limits; then
    printf 'MEMTOTAL_KIB=%s\nCONTROL_RESERVE_BYTES=%s\nWORKLOAD_MEMORY_HIGH_BYTES=%s\nWORKLOAD_MEMORY_MAX_BYTES=%s\n' "$MEMTOTAL_KIB" "$CONTROL_RESERVE_BYTES" "$WORKLOAD_MEMORY_HIGH_BYTES" "$WORKLOAD_MEMORY_MAX_BYTES"
  else
    printf 'WORKLOAD_LIMITS=UNAVAILABLE\n'
  fi
  printf 'ACTIVATION=disabled\nAMBIENT_SERVICE_BEHAVIOR=unchanged\nDOCKER_RESTART=not_performed\n'
}

write_workload_limits() { printf '[Slice]\nMemoryHigh=%s\nMemoryMax=%s\n' "$WORKLOAD_MEMORY_HIGH_BYTES" "$WORKLOAD_MEMORY_MAX_BYTES" >"$1"; }
new_installation_id() { od -An -N32 -tx1 /dev/urandom | tr -d '[:space:]'; }

remove_backup_set() {
  local installation_id=$1 relative backup
  for relative in "${managed_relatives[@]}"; do backup=$(backup_path_for "$installation_id" "$relative"); [[ -f $backup && ! -L $backup ]] && rm -f -- "$backup"; done
  rmdir -- "$backup_dir/$installation_id" 2>/dev/null || true
}

remove_uninstall_backup_set() {
  local installation_id=$1 relative backup
  for relative in "${managed_relatives[@]}"; do backup=$(uninstall_backup_path_for "$installation_id" "$relative"); [[ -f $backup && ! -L $backup ]] && rm -f -- "$backup"; done
  rmdir -- "$backup_dir/$installation_id" 2>/dev/null || true
}

stage_file() {
  local receipt=$1 installation_id=$2 relative=$3 source=$4 installed_metadata=$5 target backup prior_sha256 prior_metadata installed_sha256 temporary uid gid mode
  [[ -f $source && ! -L $source ]] && valid_file_metadata "$installed_metadata" || return 1
  target=$(target_for_relative "$relative") || return 1
  install -d -m 0755 "$(dirname -- "$target")"; installed_sha256=$(file_sha256 "$source")
  if path_exists_or_link "$target"; then
    [[ -f $target && ! -L $target ]] || return 1
    prior_sha256=$(file_sha256 "$target"); prior_metadata=$(file_metadata "$target"); backup=$(backup_path_for "$installation_id" "$relative")
    cp --preserve=mode,ownership -- "$target" "$backup"
    [[ $(file_sha256 "$backup") == "$prior_sha256" && $(file_metadata "$backup") == "$prior_metadata" ]] || return 1
    record_entry "$receipt" restore "$target" "$backup" "$prior_sha256" "$prior_metadata" "$installed_sha256" "$installed_metadata"
  else
    record_entry "$receipt" remove "$target" - - - "$installed_sha256" "$installed_metadata"
  fi
  temporary=$(mktemp "$(dirname -- "$target")/.ramshared-control.${installation_id}.XXXXXX")
  IFS=: read -r uid gid mode <<<"$installed_metadata"; install -m "$mode" -- "$source" "$temporary"; chown "$uid:$gid" -- "$temporary"
  [[ $(file_sha256 "$temporary") == "$installed_sha256" && $(file_metadata "$temporary") == "$installed_metadata" ]] || { rm -f -- "$temporary"; return 1; }
  mv -f -- "$temporary" "$target"
}

preflight_recovery() {
  local receipt=$1 line_kind operation target backup backup_sha256 backup_metadata installed_sha256 installed_metadata extra
  validate_receipt_structure "$receipt" partial || return 1
  while IFS=$'\t' read -r line_kind operation target backup backup_sha256 backup_metadata installed_sha256 installed_metadata extra; do
    [[ $line_kind == entry ]] || continue
    case $operation in
      restore) target_matches "$target" "$installed_sha256" "$installed_metadata" || target_matches "$target" "$backup_sha256" "$backup_metadata" || return 1 ;;
      remove) target_matches "$target" "$installed_sha256" "$installed_metadata" || ! path_exists_or_link "$target" || return 1 ;;
      reinstall)
        if [[ $installed_sha256 == - ]]; then
          target_matches "$target" "$backup_sha256" "$backup_metadata" || ! path_exists_or_link "$target" || return 1
        else
          target_matches "$target" "$backup_sha256" "$backup_metadata" || target_matches "$target" "$installed_sha256" "$installed_metadata" || return 1
        fi
        ;;
    esac
  done <"$receipt"
}

restore_exact_file() {
  local backup=$1 backup_sha256=$2 backup_metadata=$3 target=$4 temporary
  target_matches "$target" "$backup_sha256" "$backup_metadata" && return 0
  temporary=$(mktemp "$(dirname -- "$target")/.ramshared-control.rollback.XXXXXX")
  cp --preserve=mode,ownership -- "$backup" "$temporary"
  [[ $(file_sha256 "$temporary") == "${backup_sha256,,}" && $(file_metadata "$temporary") == "$backup_metadata" ]] || { rm -f -- "$temporary"; return 1; }
  mv -f -- "$temporary" "$target"; target_matches "$target" "$backup_sha256" "$backup_metadata"
}

recover_receipt() {
  local receipt=$1 line_kind operation target backup backup_sha256 backup_metadata installed_sha256 installed_metadata extra
  preflight_recovery "$receipt" || return 1
  while IFS=$'\t' read -r line_kind operation target backup backup_sha256 backup_metadata installed_sha256 installed_metadata extra; do
    [[ $line_kind == entry ]] || continue
    case $operation in
      restore)
        if target_matches "$target" "$installed_sha256" "$installed_metadata"; then
          restore_exact_file "$backup" "$backup_sha256" "$backup_metadata" "$target"
        fi
        ;;
      remove)
        if target_matches "$target" "$installed_sha256" "$installed_metadata"; then
          rm -f -- "$target"
        fi
        ;;
      reinstall)
        if [[ $installed_sha256 == - ]]; then
          if ! path_exists_or_link "$target"; then restore_exact_file "$backup" "$backup_sha256" "$backup_metadata" "$target"; fi
        elif target_matches "$target" "$installed_sha256" "$installed_metadata"; then
          restore_exact_file "$backup" "$backup_sha256" "$backup_metadata" "$target"
        fi
        ;;
    esac
  done <"$receipt"
  return 0
}

install_id=
transaction_active=0
transaction_kind=
rollback_install_failure() {
  local status=$? rollback_reloaded=0
  trap - EXIT
  if (( transaction_active )); then
    if recover_receipt "$transaction"; then
      if systemctl daemon-reload; then
        rollback_reloaded=1
        rm -f -- "$transaction"
        if [[ $transaction_kind == install ]]; then remove_backup_set "$install_id"; else remove_uninstall_backup_set "$install_id"; fi
      else
        echo 'control-plane staging recovery restored files but daemon reload remains pending' >&2
      fi
    else
      echo 'control-plane staging recovery refused; preserve operator changes and recover explicitly' >&2
    fi
  fi
  exit "$status"
}

stage_uninstall_recovery() {
  local active_manifest=$1 receipt=$2 installation_id=$3 line_kind operation target backup backup_sha256 backup_metadata installed_sha256 installed_metadata extra relative snapshot
  while IFS=$'\t' read -r line_kind operation target backup backup_sha256 backup_metadata installed_sha256 installed_metadata extra; do
    [[ $line_kind == entry ]] || continue
    relative=$(relative_for_target "$target") || return 1
    target_matches "$target" "$installed_sha256" "$installed_metadata" || return 1
    snapshot=$(uninstall_backup_path_for "$installation_id" "$relative")
    cp --preserve=mode,ownership -- "$target" "$snapshot"
    [[ $(file_sha256 "$snapshot") == "${installed_sha256,,}" && $(file_metadata "$snapshot") == "$installed_metadata" ]] || return 1
    if [[ $operation == restore ]]; then
      printf 'entry\treinstall\t%s\t%s\t%s\t%s\t%s\t%s\n' "$target" "$snapshot" "$installed_sha256" "$installed_metadata" "$backup_sha256" "$backup_metadata" >>"$receipt"
    else
      printf 'entry\treinstall\t%s\t%s\t%s\t%s\t-\t-\n' "$target" "$snapshot" "$installed_sha256" "$installed_metadata" >>"$receipt"
    fi
  done <"$active_manifest"
  validate_receipt_structure "$receipt"
}

install_control_plane() {
  local relative limits_temporary
  systemctl is-active docker.socket >/dev/null 2>&1 || { echo 'docker.socket is not active' >&2; return 69; }
  systemctl is-enabled docker.socket >/dev/null 2>&1 || { echo 'docker.socket is not enabled' >&2; return 69; }
  compute_workload_limits || { echo 'guest memory cannot safely reserve the control plane' >&2; return 1; }
  [[ ! -e $manifest && ! -L $manifest && ! -e $transaction && ! -L $transaction ]] || { echo 'control plane installation state already exists; recover it explicitly' >&2; return 1; }
  install -d -m 0700 "$state_dir" "$backup_dir"; [[ -d $state_dir && ! -L $state_dir && -d $backup_dir && ! -L $backup_dir ]] || return 1
  install_id=$(new_installation_id); [[ $install_id =~ ^[[:xdigit:]]{64}$ ]] || return 1
  install -d -m 0700 "$backup_dir/$install_id"; printf '%s\t%s\n' "$manifest_magic" "$install_id" >"$transaction"
  transaction_active=1; transaction_kind=install; trap rollback_install_failure EXIT
  for relative in "${units[@]}" "${dropins[@]}"; do stage_file "$transaction" "$install_id" "$relative" "$source_dir/$relative" '0:0:644'; done
  limits_temporary=$(mktemp "$state_dir/.ramshared-workload-limits.XXXXXX"); write_workload_limits "$limits_temporary"; stage_file "$transaction" "$install_id" "$generated_workload_limits" "$limits_temporary" '0:0:644'; rm -f -- "$limits_temporary"
  validate_receipt_structure "$transaction" || return 1
  systemctl daemon-reload
  mv -f -- "$transaction" "$manifest"; transaction_active=0; transaction_kind=; trap - EXIT
  printf 'RAMSHARED_CONTROL_PLANE=INSTALLED_DISABLED\nNEXT=coordinate Docker restart and enable units in an approved window\n'
}

uninstall_control_plane() {
  local installation_id
  systemctl is-active docker.socket >/dev/null 2>&1 || { echo 'docker.socket is not active' >&2; return 69; }
  systemctl is-enabled docker.socket >/dev/null 2>&1 || { echo 'docker.socket is not enabled' >&2; return 69; }
  [[ -f $manifest && ! -L $manifest ]] || { echo 'owned install receipt is missing' >&2; return 1; }
  validate_active_manifest || { echo 'control-plane rollback refused; preserve operator changes and resolve them explicitly' >&2; return 1; }
  installation_id=$(read_receipt_header "$manifest") || return 1
  install_id=$(new_installation_id); [[ $install_id =~ ^[[:xdigit:]]{64}$ ]] || return 1
  install -d -m 0700 "$backup_dir/$install_id"; printf '%s\t%s\n' "$manifest_magic" "$install_id" >"$transaction"
  transaction_active=1; transaction_kind=uninstall; trap rollback_install_failure EXIT
  stage_uninstall_recovery "$manifest" "$transaction" "$install_id"
  recover_receipt "$manifest" || { echo 'control-plane rollback refused; preserve operator changes and resolve them explicitly' >&2; return 1; }
  systemctl daemon-reload
  rm -f -- "$transaction"; remove_uninstall_backup_set "$install_id"
  rm -f -- "$manifest"; remove_backup_set "$installation_id"
  transaction_active=0; transaction_kind=; trap - EXIT
  printf 'RAMSHARED_CONTROL_PLANE=ROLLED_BACK\n'
}

[[ $action =~ ^(plan|install|status|uninstall)$ ]] || { echo 'usage: manage-control-plane.sh plan|install|status|uninstall [--apply]' >&2; exit 2; }
if [[ $action == plan || ( $action != status && $apply != --apply ) ]]; then plan; exit 0; fi
if [[ $action == status ]]; then
  if path_exists_or_link "$transaction"; then echo 'control-plane staging recovery is required' >&2; exit 1; fi
  if ! path_exists_or_link "$manifest"; then printf 'RAMSHARED_CONTROL_PLANE=NOT_INSTALLED\n'; exit 0; fi
  validate_active_manifest || { echo 'control-plane install receipt is stale, foreign, or malformed' >&2; exit 1; }
  printf 'RAMSHARED_CONTROL_PLANE=INSTALLED_DISABLED\n'; cat -- "$manifest"; exit 0
fi
(( EUID == 0 )) || { echo 'control-plane mutation requires root' >&2; exit 1; }
if [[ $action == install ]]; then install_control_plane; else uninstall_control_plane; fi
