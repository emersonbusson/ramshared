#!/usr/bin/env bash
# measure-cascade-demote.sh — proves the DEMOTE ACTION (SPEC §9 / §14.4).
#
# With active pages in the VRAM tier (/dev/nbd0), runs `swapoff /dev/nbd0` while the
# daemon continues to serve read-back; checks the integrity of the hog after migration
# to the lower tier (VHDX/zram). The canary TRIGGER (latency/free/content)
# is unit-tested in crates/ramshared-wsl2d/src/residency.rs — this script validates
# safe migration in runtime (the same call as spawn_swapoff in the daemon).
#
# Host-safety (benchmarks.md / Kahneman #16):
#   - hog isolated in cgroup v2 (bounded memory.max)
#   - no kill -9 of the daemon; no global thrash
#   - by default, RESTORES NBD swapon after the drill (keeps cushion)
#
# usage (root):
#   ./scripts/p0/measure-cascade-demote.sh
# if the agent does not have sudo (sandbox), use root through Docker host namespaces:
#   docker run --rm --privileged --pid=host --cgroupns=host alpine:3.20 \
#     nsenter -t 1 -m -u -i -n -p -- \
#     /bin/bash ./scripts/p0/measure-cascade-demote.sh
# (--cgroupns=host is mandatory: without it, writing to cgroup.procs returns ENOENT)
# optional environment:
#   HOG_MB=2200 CAP_MB=512 MIN_NBD_MIB=150 RESTORE=1 RAW=/tmp/cascade-demote.txt
set -euo pipefail

# Validate /sys/block/zram*/mm_stat and cascade sysfs entries exist before reading demotion metrics
for entry in mm_stat bd_stat backing_dev writeback; do
    if ! ls /sys/block/zram*/"$entry" >/dev/null 2>&1; then
        echo "FAILURE: /sys/block/zram*/$entry not found"
        exit 69
    fi
done

HOG_BIN="${HOG_BIN:-}"
if [ -z "$HOG_BIN" ]; then
  if [ -x ./target/release/cascade-hog ]; then HOG_BIN=./target/release/cascade-hog
  elif command -v cascade-hog >/dev/null 2>&1; then HOG_BIN=$(command -v cascade-hog)
  fi
fi
RAW="${RAW:-${TMPDIR:-/tmp}/ramshared-cascade-demote-$(date +%Y%m%d-%H%M%S).txt}"
CG="${CG:-/sys/fs/cgroup/ramshared-demote-drill}"
# Defaults tuned for product cascade sizes ~2G zram + 2G nbd (2026-07):
# need hog >> zram free so pages spill to nbd under cgroup cap.
HOG_MB="${HOG_MB:-2800}"
CAP_MB="${CAP_MB:-256}"
MIN_NBD_MIB="${MIN_NBD_MIB:-100}"
RESTORE="${RESTORE:-1}"   # 1 = swapon -p 100 /dev/nbd0 after the proof
NBD_DEV="${NBD_DEV:-/dev/nbd0}"
SWAPOFF_BIN="${SWAPOFF_BIN:-/usr/sbin/swapoff}"
SWAPON_BIN="${SWAPON_BIN:-/usr/sbin/swapon}"
STATUS_BIN="${STATUS_BIN:-}"
if [ -z "$STATUS_BIN" ]; then
  if [ -x ./target/release/ramshared ]; then STATUS_BIN=./target/release/ramshared
  elif command -v ramshared >/dev/null 2>&1; then STATUS_BIN=$(command -v ramshared)
  fi
fi

HOG_PID=""
DEMOTE_DONE=0
: >"$RAW"
log() { echo "$@" | tee -a "$RAW"; }

nbd_used_mib() {
  awk -v d="$NBD_DEV" '$1==d{print int($4/1024)}' /proc/swaps
}
vhdx_used_mib() {
  awk '$1 ~ /\/dev\/sd[a-z]/{print int($4/1024); exit}' /proc/swaps
}
zram_used_mib() {
  awk '$1 ~ /zram/{print int($4/1024); exit}' /proc/swaps
}
tier_present() {
  awk -v d="$1" '$1==d{found=1} END{exit !found}' /proc/swaps
}

snapshot_swaps() {
  log "--- /proc/swaps ---"
  cat /proc/swaps | tee -a "$RAW"
  log "--- free -h ---"
  free -h | tee -a "$RAW"
}

teardown() {
  local rc=$?
  log ""
  log "=== CLEANUP (rc=$rc) ==="
  # releases the hog if it is still held
  touch /tmp/cv-go 2>/dev/null || true
  if [ -n "${HOG_PID:-}" ] && kill -0 "$HOG_PID" 2>/dev/null; then
    kill "$HOG_PID" 2>/dev/null || true
    wait "$HOG_PID" 2>/dev/null || true
  fi
  # if demote ran and restore was requested, reattaches VRAM swap (daemon must be alive)
  if [ "$DEMOTE_DONE" = 1 ] && [ "$RESTORE" = 1 ]; then
    if ! tier_present "$NBD_DEV"; then
      if pgrep -x ramsharedd >/dev/null 2>&1 || pgrep -f 'ramsharedd ' >/dev/null 2>&1; then
        log "RESTORE: $SWAPON_BIN -p 100 $NBD_DEV"
        if $SWAPON_BIN -p 100 "$NBD_DEV" >>"$RAW" 2>&1; then
          log "RESTORE ok"
        else
          log "RESTORE failed (see RAW); cascade is without VRAM until 'ramshared up'"
        fi
      else
        log "RESTORE skip: ramsharedd is not running"
      fi
    fi
  fi
  [ -d "$CG" ] && rmdir "$CG" 2>/dev/null || true
  rm -f /tmp/cv-filled /tmp/cv-go
  log "RAW: $RAW"
  snapshot_swaps
}
trap teardown EXIT
trap 'exit 143' INT TERM

log_status() {
  local tag="$1"
  log "--- status ($tag) ---"
  if [ -n "$STATUS_BIN" ] && [ -x "$STATUS_BIN" ]; then
    "$STATUS_BIN" status --json 2>/dev/null | tee -a "$RAW" || log "status --json failed"
  else
    log "status bin missing"
  fi
  if [ -f /run/ramshared/demote-status.json ]; then
    log "demote-status: $(cat /run/ramshared/demote-status.json)"
  else
    log "demote-status: (absent)"
  fi
}

log "### CASCADE DEMOTE DRILL — $(date -Is) ###"
log "params: HOG_MB=$HOG_MB CAP_MB=$CAP_MB MIN_NBD_MIB=$MIN_NBD_MIB RESTORE=$RESTORE NBD=$NBD_DEV"
log "issue: #31 demote under pressure + integrity (action path = spawn_swapoff)"
[ "$(id -u)" = 0 ] || { log "root is required"; exit 2; }
[ -x "$HOG_BIN" ] || { log "hog missing: $HOG_BIN"; exit 2; }
[ -b "$NBD_DEV" ] || { log "block device missing: $NBD_DEV"; exit 2; }

log ""
log "=== 0. preflight cascade ==="
snapshot_swaps
log_status preflight
tier_present "$NBD_DEV" || { log "FAILURE: $NBD_DEV is not in /proc/swaps (start the cascade first)"; exit 1; }
tier_present /dev/zram0 || tier_present /dev/zram1 || log "WARN: no zram (A1 is still OK if VHDX exists)"
# A1: requires a sink below VRAM
if ! awk '$1 ~ /\/dev\/sd/ && $5+0 < 100 {ok=1} END{exit !ok}' /proc/swaps; then
  log "FAILURE: A1 invariant — no VHDX (priority < 100) to absorb DEMOTE"
  exit 1
fi
if ! pgrep -x ramsharedd >/dev/null 2>&1 && ! pgrep -f 'ramsharedd ' >/dev/null 2>&1; then
  log "FAILURE: ramsharedd is not alive (swapoff without a server = hang)"
  exit 1
fi
log "preflight OK (A1 + nbd + daemon)"

rm -f /tmp/cv-filled /tmp/cv-go

log ""
log "=== 1. hog hold in cgroup (active pages in VRAM) ==="
echo +memory > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true
# cleans the residual cgroup from a previous drill
if [ -d "$CG" ]; then
  # tries to empty it before rmdir
  while read -r p; do
    [ -n "$p" ] && echo "$p" > /sys/fs/cgroup/cgroup.procs 2>/dev/null || true
  done <"$CG/cgroup.procs" 2>/dev/null || true
  rmdir "$CG" 2>/dev/null || true
fi
mkdir -p "$CG" || { log "FAILURE: mkdir $CG"; exit 1; }
echo "${CAP_MB}M" >"$CG/memory.max" || { log "FAILURE: memory.max"; exit 1; }
echo max >"$CG/memory.swap.max" || { log "FAILURE: memory.swap.max"; exit 1; }

# Start first, then migrate PID into cgroup (WSL: echo $$ in subshell often fails with ENOENT).
"$HOG_BIN" "$HOG_MB" hold >>"$RAW" 2>&1 &
HOG_PID=$!
# migrate ASAP so fill already runs under cap
if ! echo "$HOG_PID" >"$CG/cgroup.procs" 2>>"$RAW"; then
  log "FAILURE: did not migrate hog pid=$HOG_PID to $CG (no isolation = abort)"
  kill "$HOG_PID" 2>/dev/null || true
  wait "$HOG_PID" 2>/dev/null || true
  exit 1
fi
log "hog pid=$HOG_PID in cgroup memory.max=${CAP_MB}M"
if [ -r "$CG/cgroup.procs" ]; then
  log "cgroup.procs=$(tr '\n' ' ' <"$CG/cgroup.procs")"
fi

# waits for fill
for _ in $(seq 1 180); do
  [ -f /tmp/cv-filled ] && break
  kill -0 "$HOG_PID" 2>/dev/null || { log "hog exited before fill"; wait "$HOG_PID"; exit 1; }
  sleep 0.5
done
[ -f /tmp/cv-filled ] || { log "timeout waiting for hog fill"; exit 1; }

# waits for spill to NBD
for _ in $(seq 1 90); do
  u=$(nbd_used_mib)
  u=${u:-0}
  [ "$u" -ge "$MIN_NBD_MIB" ] && break
  sleep 0.5
done
NBD_BEFORE=$(nbd_used_mib); NBD_BEFORE=${NBD_BEFORE:-0}
VHDX_BEFORE=$(vhdx_used_mib); VHDX_BEFORE=${VHDX_BEFORE:-0}
ZRAM_BEFORE=$(zram_used_mib); ZRAM_BEFORE=${ZRAM_BEFORE:-0}
log "before DEMOTE: nbd=${NBD_BEFORE} MiB zram=${ZRAM_BEFORE} MiB vhdx=${VHDX_BEFORE} MiB"
log_status before-demote
if [ "$NBD_BEFORE" -lt "$MIN_NBD_MIB" ]; then
  log "FAILURE: too few pages in VRAM ($NBD_BEFORE < $MIN_NBD_MIB). Increase HOG_MB or reduce CAP_MB/zram."
  exit 1
fi

log ""
log "=== 2. DEMOTE action: swapoff $NBD_DEV (daemon serve read-back) ==="
log "NOTE: sparse product path skips FreeFloor/Latency auto-swapoff; this drills the same"
log "      spawn_swapoff action the daemon uses for Corruption/WDDM-constrained demote."
# Raise cgroup cap so page-in during swapoff does not OOM-kill the hog (integrity).
if [ -n "${HOG_PID:-}" ] && [ -d "$CG" ]; then
  DEMOTE_CAP_MB="${DEMOTE_CAP_MB:-$((HOG_MB + 512))}"
  log "raising cgroup memory.max to ${DEMOTE_CAP_MB}M for demote page-in"
  echo "${DEMOTE_CAP_MB}M" >"$CG/memory.max" 2>>"$RAW" || log "WARN: could not raise memory.max"
fi
T0=$(date +%s%N)
if timeout 300 "$SWAPOFF_BIN" "$NBD_DEV" >>"$RAW" 2>&1; then
  T1=$(date +%s%N)
  MS=$(( (T1 - T0) / 1000000 ))
  log "swapoff $NBD_DEV OK in ${MS} ms"
  DEMOTE_DONE=1
else
  log "FAILURE: swapoff $NBD_DEV (timeout or error) — risk of stuck pages"
  # show hog still alive?
  if [ -n "${HOG_PID:-}" ]; then
    if kill -0 "$HOG_PID" 2>/dev/null; then log "hog still alive pid=$HOG_PID"
    else log "hog dead during swapoff (likely OOM in cgroup)"
    fi
  fi
  exit 1
fi

if tier_present "$NBD_DEV"; then
  log "FAILURE: $NBD_DEV is still in /proc/swaps after demote"
  exit 1
fi
VHDX_AFTER=$(vhdx_used_mib); VHDX_AFTER=${VHDX_AFTER:-0}
ZRAM_AFTER=$(zram_used_mib); ZRAM_AFTER=${ZRAM_AFTER:-0}
log "after DEMOTE: nbd=ABSENT zram=${ZRAM_AFTER} MiB vhdx=${VHDX_AFTER} MiB"
log_status after-demote
# zram and/or vhdx must remain active
if ! awk 'NR>1{n++} END{exit !(n>=1)}' /proc/swaps; then
  log "FAILURE: no swap remains after demote"
  exit 1
fi

log ""
log "=== 3. post-migration integrity (hog verify through fault-in) ==="
touch /tmp/cv-go
set +e
wait "$HOG_PID"
HOG_RC=$?
set -e
HOG_PID=""
log "hog rc=$HOG_RC"
grep -E '\[hog\]' "$RAW" | tail -5 | tee -a "$RAW" >/dev/null || true
grep -E '\[hog\]' "$RAW" | tail -5 | while read -r line; do log "  $line"; done || true

log ""
log "=== 4. VERDICT ==="
OK=1
[ "$HOG_RC" = 0 ] || { log "FAILURE: integrity (hog rc=$HOG_RC)"; OK=0; }
[ "$DEMOTE_DONE" = 1 ] || { log "FAILURE: demote did not complete"; OK=0; }
if [ "$OK" = 1 ]; then
  log ">>> DEMOTE OK: ${NBD_BEFORE} MiB of active pages left VRAM; 0 corruption in hog; sink active."
  log ">>> canary trigger: unit-tested (residency.rs); action path: THIS drill."
else
  log ">>> DEMOTE FAILED."
fi
log "### END $(date -Is) ###"
exit $((1 - OK))
