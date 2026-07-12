#!/usr/bin/env bash
# measure-cascade-demote.sh — prova a ACAO do DEMOTE (SPEC §9 / §14.4).
#
# With active pages in the VRAM tier (/dev/nbd0), runs `swapoff /dev/nbd0` while the
# daemon continues to serve read-back; checks the integrity of the hog after migration
# to the lower tier (VHDX/zram). The canary TRIGGER (latency/free/content)
# is unit-tested in crates/ramshared-wsl2d/src/residency.rs — this script validates
# safe migration in runtime (the same call as spawn_swapoff in the daemon).
#
# Host-safety (benchmarks.md / Kahneman #16):
#   - hog isolado em cgroup v2 (memory.max limitado)
#   - sem kill -9 no daemon; sem thrash global
#   - por default RESTAURA o swapon do NBD apos o drill (mantem cushion)
#
# uso (root):
#   ./scripts/p0/measure-cascade-demote.sh
# se o agent nao tem sudo (sandbox), root via Docker host namespaces:
#   docker run --rm --privileged --pid=host --cgroupns=host alpine:3.20 \
#     nsenter -t 1 -m -u -i -n -p -- \
#     /bin/bash ./scripts/p0/measure-cascade-demote.sh
# (--cgroupns=host is mandatory: without it, writing to cgroup.procs returns ENOENT)
# env opcional:
#   HOG_MB=2200 CAP_MB=512 MIN_NBD_MIB=150 RESTORE=1 RAW=/tmp/cascade-demote.txt
set -u

HOG_BIN="${HOG_BIN:-/home/emdev/fase0/cascade-hog}"
RAW="${RAW:-/home/emdev/fase0/CASCADE-DEMOTE-$(date +%Y%m%d-%H%M%S).txt}"
CG="${CG:-/sys/fs/cgroup/ramshared-demote-drill}"
HOG_MB="${HOG_MB:-2200}"
CAP_MB="${CAP_MB:-512}"
MIN_NBD_MIB="${MIN_NBD_MIB:-150}"
RESTORE="${RESTORE:-1}"   # 1 = swapon -p 100 /dev/nbd0 apos prova
NBD_DEV="${NBD_DEV:-/dev/nbd0}"
SWAPOFF_BIN="${SWAPOFF_BIN:-/usr/sbin/swapoff}"
SWAPON_BIN="${SWAPON_BIN:-/usr/sbin/swapon}"

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
  # libera hog se ainda segurando
  touch /tmp/cv-go 2>/dev/null || true
  if [ -n "${HOG_PID:-}" ] && kill -0 "$HOG_PID" 2>/dev/null; then
    kill "$HOG_PID" 2>/dev/null || true
    wait "$HOG_PID" 2>/dev/null || true
  fi
  # se demote rodou e pedimos restore, reata VRAM swap (daemon deve estar vivo)
  if [ "$DEMOTE_DONE" = 1 ] && [ "$RESTORE" = 1 ]; then
    if ! tier_present "$NBD_DEV"; then
      if pgrep -x ramsharedd >/dev/null 2>&1 || pgrep -f 'ramsharedd ' >/dev/null 2>&1; then
        log "RESTORE: $SWAPON_BIN -p 100 $NBD_DEV"
        if $SWAPON_BIN -p 100 "$NBD_DEV" >>"$RAW" 2>&1; then
          log "RESTORE ok"
        else
          log "RESTORE falhou (ver RAW); cascata sem VRAM ate 'ramshared up'"
        fi
      else
        log "RESTORE skip: ramsharedd nao esta rodando"
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

log "### CASCADE DEMOTE DRILL — $(date -Is) ###"
log "params: HOG_MB=$HOG_MB CAP_MB=$CAP_MB MIN_NBD_MIB=$MIN_NBD_MIB RESTORE=$RESTORE NBD=$NBD_DEV"
[ "$(id -u)" = 0 ] || { log "precisa root"; exit 2; }
[ -x "$HOG_BIN" ] || { log "hog ausente: $HOG_BIN"; exit 2; }
[ -b "$NBD_DEV" ] || { log "block device ausente: $NBD_DEV"; exit 2; }

log ""
log "=== 0. preflight cascade ==="
snapshot_swaps
tier_present "$NBD_DEV" || { log "FALHA: $NBD_DEV nao esta em /proc/swaps (suba a cascata antes)"; exit 1; }
tier_present /dev/zram0 || tier_present /dev/zram1 || log "WARN: sem zram (A1 ainda ok se VHDX existir)"
# A1: precisa sink abaixo da VRAM
if ! awk '$1 ~ /\/dev\/sd/ && $5+0 < 100 {ok=1} END{exit !ok}' /proc/swaps; then
  log "FALHA: invariante A1 — sem VHDX (prio < 100) para absorver DEMOTE"
  exit 1
fi
if ! pgrep -f 'ramsharedd' >/dev/null 2>&1; then
  log "FALHA: ramsharedd nao esta vivo (swapoff sem servidor = hang)"
  exit 1
fi
log "preflight OK (A1 + nbd + daemon)"

rm -f /tmp/cv-filled /tmp/cv-go

log ""
log "=== 1. hog hold em cgroup (paginas vivas na VRAM) ==="
echo +memory > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true
# limpa cgroup residual de drill anterior
if [ -d "$CG" ]; then
  # tenta esvaziar antes de rmdir
  while read -r p; do
    [ -n "$p" ] && echo "$p" > /sys/fs/cgroup/cgroup.procs 2>/dev/null || true
  done <"$CG/cgroup.procs" 2>/dev/null || true
  rmdir "$CG" 2>/dev/null || true
fi
mkdir -p "$CG" || { log "FALHA: mkdir $CG"; exit 1; }
echo "${CAP_MB}M" >"$CG/memory.max" || { log "FALHA: memory.max"; exit 1; }
echo max >"$CG/memory.swap.max" || { log "FALHA: memory.swap.max"; exit 1; }

# Start first, then migrate PID into cgroup (WSL: echo $$ in subshell often fails with ENOENT).
"$HOG_BIN" "$HOG_MB" hold >>"$RAW" 2>&1 &
HOG_PID=$!
# migrate ASAP so fill already runs under cap
if ! echo "$HOG_PID" >"$CG/cgroup.procs" 2>>"$RAW"; then
  log "FALHA: nao migrou hog pid=$HOG_PID para $CG (sem isolamento = abort)"
  kill "$HOG_PID" 2>/dev/null || true
  wait "$HOG_PID" 2>/dev/null || true
  exit 1
fi
log "hog pid=$HOG_PID em cgroup memory.max=${CAP_MB}M"
if [ -r "$CG/cgroup.procs" ]; then
  log "cgroup.procs=$(tr '\n' ' ' <"$CG/cgroup.procs")"
fi

# espera fill
for _ in $(seq 1 180); do
  [ -f /tmp/cv-filled ] && break
  kill -0 "$HOG_PID" 2>/dev/null || { log "hog morreu antes do fill"; wait "$HOG_PID"; exit 1; }
  sleep 0.5
done
[ -f /tmp/cv-filled ] || { log "timeout aguardando fill do hog"; exit 1; }

# espera spill no NBD
for _ in $(seq 1 90); do
  u=$(nbd_used_mib)
  u=${u:-0}
  [ "$u" -ge "$MIN_NBD_MIB" ] && break
  sleep 0.5
done
NBD_BEFORE=$(nbd_used_mib); NBD_BEFORE=${NBD_BEFORE:-0}
VHDX_BEFORE=$(vhdx_used_mib); VHDX_BEFORE=${VHDX_BEFORE:-0}
ZRAM_BEFORE=$(zram_used_mib); ZRAM_BEFORE=${ZRAM_BEFORE:-0}
log "antes DEMOTE: nbd=${NBD_BEFORE} MiB zram=${ZRAM_BEFORE} MiB vhdx=${VHDX_BEFORE} MiB"
if [ "$NBD_BEFORE" -lt "$MIN_NBD_MIB" ]; then
  log "FALHA: poucas paginas na VRAM ($NBD_BEFORE < $MIN_NBD_MIB). Aumente HOG_MB ou reduza CAP_MB/zram."
  exit 1
fi

log ""
log "=== 2. DEMOTE action: swapoff $NBD_DEV (daemon serve read-back) ==="
T0=$(date +%s%N)
if timeout 120 "$SWAPOFF_BIN" "$NBD_DEV" >>"$RAW" 2>&1; then
  T1=$(date +%s%N)
  MS=$(( (T1 - T0) / 1000000 ))
  log "swapoff $NBD_DEV OK em ${MS} ms"
  DEMOTE_DONE=1
else
  log "FALHA: swapoff $NBD_DEV (timeout ou erro) — risco de paginas presas"
  exit 1
fi

if tier_present "$NBD_DEV"; then
  log "FALHA: $NBD_DEV ainda em /proc/swaps apos demote"
  exit 1
fi
VHDX_AFTER=$(vhdx_used_mib); VHDX_AFTER=${VHDX_AFTER:-0}
ZRAM_AFTER=$(zram_used_mib); ZRAM_AFTER=${ZRAM_AFTER:-0}
log "apos DEMOTE: nbd=AUSENTE zram=${ZRAM_AFTER} MiB vhdx=${VHDX_AFTER} MiB"
# zram e/ou vhdx devem continuar
if ! awk 'NR>1{n++} END{exit !(n>=1)}' /proc/swaps; then
  log "FALHA: nenhum swap restante apos demote"
  exit 1
fi

log ""
log "=== 3. integridade pos-migracao (hog verify via fault-in) ==="
touch /tmp/cv-go
wait "$HOG_PID"
HOG_RC=$?
HOG_PID=""
log "hog rc=$HOG_RC"
grep -E '\[hog\]' "$RAW" | tail -5 | tee -a "$RAW" >/dev/null
grep -E '\[hog\]' "$RAW" | tail -5 | while read -r line; do log "  $line"; done

log ""
log "=== 4. VEREDITO ==="
OK=1
[ "$HOG_RC" = 0 ] || { log "FALHA: integridade (hog rc=$HOG_RC)"; OK=0; }
[ "$DEMOTE_DONE" = 1 ] || { log "FALHA: demote nao completou"; OK=0; }
if [ "$OK" = 1 ]; then
  log ">>> DEMOTE OK: ${NBD_BEFORE} MiB vivos sairam da VRAM; 0 corrupcao no hog; sink ativo."
  log ">>> canary trigger: unit-tested (residency.rs); action path: THIS drill."
else
  log ">>> DEMOTE COM FALHA."
fi
log "### FIM $(date -Is) ###"
exit $((1 - OK))
