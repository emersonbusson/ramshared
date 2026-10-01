#!/usr/bin/env bash
# Guest-side buddy-fragmentation drill for the ring-buffer upstream v2 series.
#
# Breaks high-order contiguity for real (KUnit fault injection is not this
# evidence) and then forces a fresh ring allocation. What that proves depends
# on the guest, and the verdict says which:
#
#   Ordinary x86_64 Hyper-V (this drill's home on a hosted runner):
#     vmbus_alloc_buffer() takes the vzalloc() path, because
#     vmbus_uses_shared_page_chunks() is only true for host-visible buffers in
#     an isolated VM or on ARM64. So the claim here is the historical one --
#     channel open survives buddy fragmentation -- and NOT that the CoCo
#     chunked order-N -> order-0 fallback ran. That path is unreachable on
#     this guest and is covered by KUnit fault injection instead; its runtime
#     proof stays with COCO-1..5.
#
#   Confidential guest (SEV-SNP / TDX / arm64 CCA):
#     the chunked path runs and this same drill is what would exercise the
#     order-7 -> order-0 degrade. That is exactly why COCO is still open.
#
# Runs INSIDE a disposable guest only. It is memory pressure, so it is
# forbidden on the daily WSL2 host and is guarded against it.
#
# Host-safety contract (vmbus-ring-buffer-upstream-v2/SPEC.md): never on the
# daily WSL2 environment. Swap is never activated here.
#
# usage: vmbus-fragmentation-drill.sh [hog_mib] [logfile]
#   hog_mib  ceiling in MiB; default MemTotal. The helper allocates 64 KiB
#            chunks until /proc/buddyinfo shows no free block of order 7 or
#            above, then unmaps one member of every physical buddy pair whose
#            buddy it also holds. The ceiling must be above what the guest can
#            allocate: if the loop stops at the ceiling the untouched
#            remainder still holds order-10 blocks and the drill correctly
#            reports INCONCLUSIVE.
#
# Depends on vmbus_drill_helper (static, see hyperv-drill-initramfs/) for the
# fragment-buddy primitive. The guest has no CPython and this is a Day-0
# dependency, not a shim.
set -euo pipefail

LOG="${2:-/var/tmp/vmbus-fragmentation-drill.log}"
: >"$LOG"

HELPER="${HELPER:-$(command -v vmbus_drill_helper || true)}"

# --- guards ------------------------------------------------------------------
guard() {
	if grep -qiE 'microsoft-standard-WSL2' /proc/version 2>/dev/null; then
		echo "REFUSE: daily WSL2 host. Fragmentation pressure is never run here." >&2
		exit 2
	fi
	if [ ! -d /sys/bus/vmbus ]; then
		echo "REFUSE: no VMBus; requires a Hyper-V guest." >&2
		exit 2
	fi
	# Refuse if RamShared cascade is present and active — SPEC forbids changing
	# its lifecycle state, and pressure would interact with it.
	if command -v ramshared >/dev/null 2>&1; then
		if ramshared status 2>/dev/null | grep -qiE 'phase *: *(On|ACTIVE)'; then
			echo "REFUSE: RamShared cascade is active. SPEC forbids host lifecycle change." >&2
			exit 2
		fi
	fi
	if [ -r /proc/swaps ] && [ "$(grep -c . /proc/swaps)" -gt 1 ]; then
		echo "REFUSE: swap is active. This drill never runs with swap." >&2
		exit 2
	fi
}
guard

say() { echo "$@" | tee -a "$LOG"; }

hog_mib() {
	local total
	# MemTotal, not MemAvailable. The helper treats this as a ceiling the
	# allocation loop must never reach: it stops when /proc/buddyinfo shows
	# order-7 depleted, and reports exhausted=1. Passing a share of
	# MemAvailable (the previous default) stopped the loop at the ceiling
	# with the untouched remainder still holding order-10 blocks, so
	# order-7 never failed and the drill reported INCONCLUSIVE every run.
	total="$(awk '/MemTotal/{print int($2/1024)}' /proc/meminfo)"
	echo "$total"
}

HOGB="${1:-$(hog_mib)}"
say "=== BEGIN vmbus-fragmentation-drill hog_mib=$HOGB ==="
say "kernel=$(uname -r)"
say "MemTotal=$(awk '/MemTotal/{print $2}' /proc/meminfo)kB MemAvailable=$(awk '/MemAvailable/{print $2}' /proc/meminfo)kB"

# --- buddy state before ------------------------------------------------------
buddy() { cat /proc/buddyinfo 2>/dev/null | tee -a "$LOG" || say "buddyinfo unavailable"; }

say "--- BUDDY BEFORE ---"
buddy

# --- fragment: break high-order contiguity, leave order-0 available ---------
# A single large mlock hog only splits the buddy as far as it must and leaves
# the untouched remainder as order-10 blocks. fragment-buddy instead
# allocates until /proc/buddyinfo shows order-7 depleted -- the test
# condition, measured -- and then unmaps one member of every physical buddy
# pair (pfn even) whose buddy it also holds, so a freed page's buddy is
# always held, nothing above order-0 can coalesce, and the survivors still
# serve order-0.
say "=== FRAGMENT ${HOGB} MiB as 64 KiB chunks ==="
if [ -z "$HELPER" ]; then
	say "REFUSE: vmbus_drill_helper not found; cannot fragment without fragment-buddy"
	exit 2
fi
# 600s hold: the channel-open exercise and the buddy snapshots below have to
# finish while the pattern is still in place.
"$HELPER" fragment-buddy "$HOGB" 600 >>"$LOG" 2>&1 &
HOGPID=$!

FRAG_READY=no
for _ in $(seq 1 120); do
	if grep -q 'FRAGMENT_BUDDY ready=' "$LOG" 2>/dev/null; then
		FRAG_READY=yes
		break
	fi
	sleep 1
done
# The helper writes both the ready and the refusal lines to the log; echo the
# decisive one to the console so the uploaded artifact carries it.
# tail -6: the helper now emits min_free_kbytes, start, allocated and
# ready=1, and every one of them is evidence. tail -3 dropped the
# min_free read-back, which is exactly the line that shows whether the
# watermark lever was actually pulled.
grep 'FRAGMENT_BUDDY ' "$LOG" | tail -6 | tee -a "$LOG" || true
FRAG_LINE="$(grep 'FRAGMENT_BUDDY ready=1' "$LOG" | tail -1 || true)"
if [ "$FRAG_READY" != yes ]; then
	say "FRAGMENT did not reach a ready state"
elif [ -z "$FRAG_LINE" ]; then
	say "FRAGMENT refused to build the pattern (see FRAGMENT_BUDDY line above)"
else
	say "FRAGMENT pattern in place"
fi

say "--- BUDDY AFTER FRAGMENT ---"
buddy

# Count free blocks of order 7 and above across all zones. If any remain, the
# buddy can still satisfy vmbus_alloc_ring()'s order-7 request outright and the
# fallback path is not being exercised — say so here rather than inferring it
# later from a missing dmesg line.
#
# buddyinfo layout is `Node <n>, zone <name>` followed by one count per order,
# so order-7 is field 12 and higher orders follow.
HIGH_ORDER=$(awk '
	{
		for (i = 12; i <= NF; i++)
			if ($i + 0 > 0) sum += $i
	}
	END { print sum + 0 }
' /proc/buddyinfo 2>/dev/null)
say "high_order_7plus_blocks=${HIGH_ORDER:-unknown}"

# --- exercise channel open under fragmentation ------------------------------
say "=== CHANNEL OPEN UNDER FRAGMENTATION ==="

# The instance id is host-assigned and differs on every VM. Discover it from
# the hv_netvsc binding.
discover_nic() {
	local d n
	for d in /sys/bus/vmbus/drivers/hv_netvsc/*; do
		[ -e "$d" ] || continue
		n="$(basename "$d")"
		case "$n" in
		bind | unbind | uevent | module | new_id | remove_id) continue ;;
		esac
		[ -d "$d" ] || continue
		printf '%s\n' "$n"
		return 0
	done
	return 1
}

NIC="${NIC:-$(discover_nic || true)}"
if [ -z "${NIC:-}" ]; then
	say "REFUSE: no synthetic NIC bound to hv_netvsc; cannot force ring realloc"
	kill "$HOGPID" 2>/dev/null || true
	wait "$HOGPID" 2>/dev/null || true
	exit 2
fi
say "NIC=$NIC (discovered from hv_netvsc binding)"
DRIVER_DIR="/sys/bus/vmbus/drivers"

say "PRE-OPEN $(grep -c vmbus_alloc_buffer /proc/vmallocinfo 2>/dev/null || echo 0) maps"

# force a fresh ring allocation by rebinding the synthetic NIC
echo "$NIC" >"$DRIVER_DIR/hv_netvsc/unbind" 2>>"$LOG" || true
sleep 1
echo "$NIC" >"$DRIVER_DIR/hv_netvsc/bind" 2>>"$LOG" || say "rebind_fail"
sleep 2

say "POST-OPEN $(grep -c vmbus_alloc_buffer /proc/vmallocinfo 2>/dev/null || echo 0) maps"
NIC_DRIVER="$(readlink -f "/sys/bus/vmbus/devices/$NIC/driver" 2>/dev/null || echo none)"
say "nic_driver=$NIC_DRIVER"

# --- verdict ----------------------------------------------------------------
# What this drill can and cannot claim on an ordinary x86_64 Hyper-V guest:
#
#   vmbus_alloc_buffer() takes its chunked order-N -> order-0 path only when
#   vmbus_uses_shared_page_chunks() is true, i.e. host-visible buffer AND
#   (Hyper-V isolation OR CONFIG_ARM64). A hosted runner is neither, so every
#   ring here is allocated with vzalloc() and the CoCo chunked fallback is
#   unreachable. It also cannot be forced: that path uses __GFP_NOWARN, so an
#   order-7 failure would not print 'order:7' even where the path ran.
#
#   What IS exercisable here is the historical failure mode: channel open
#   dying under buddy fragmentation (the accept4 110 incidents). The candidate
#   answers that with vzalloc(). So the claim is "fragmentation does not break
#   ring allocation", not "the CoCo order-0 fallback executed".
#
#   The chunked fallback itself is covered by the KUnit fault-injection test
#   in 0005-order-zero-fallback-injection.patch and remains runtime-open on
#   CoCo hardware (COCO-1..5).
say "--- DMESG SCAN ---"
DM="$(dmesg 2>/dev/null | tail -500)"
ORDER7="$(echo "$DM" | grep -c 'order:7' || true)"
ACCEPT="$(echo "$DM" | grep -c 'accept4 failed' || true)"
OOPS="$(echo "$DM" | grep -cE 'BUG:|Oops:|WARNING:|hung task' || true)"

REBOUND=no
case "$NIC_DRIVER" in
*hv_netvsc*) REBOUND=yes ;;
esac

# exhausted=1 is what separates "the buddy was deprived of order-7" from
# "we stopped early and the untouched remainder still has it". The previous
# runs looked like the second while the log said the first, because the cap
# was MemAvailable and the loop stopped at the ceiling before order-7 was
# gone. stop= names why the loop ended.
EXHAUSTED="$(printf '%s\n' "$FRAG_LINE" | sed -n 's/.*exhausted=\([0-9]*\).*/\1/p')"
PAGEMAP="$(printf '%s\n' "$FRAG_LINE" | sed -n 's/.*pagemap=\([0-9]*\).*/\1/p')"
STOPREASON="$(printf '%s\n' "$FRAG_LINE" | sed -n 's/.*stop=\([a-z0-9-]*\).*/\1/p')"
say "RESULT high_order_7plus_blocks=${HIGH_ORDER:-unknown} exhausted=${EXHAUSTED:-unknown} pagemap=${PAGEMAP:-unknown} stop=${STOPREASON:-unknown} order7_dmesg=$ORDER7 accept4_failures=$ACCEPT oops=$OOPS rebind=$REBOUND"

# Exit codes for init:
#   0  PASS            pressure achieved and ring allocation survived it
#   3  INCONCLUSIVE    measurement ran but the test condition was not met
#   1  FAIL            candidate damage, or the channel would not reopen
#   2  REFUSE          guard tripped (see top of file)
RC=0
if [ -z "$FRAG_LINE" ]; then
	say "VERDICT=INCONCLUSIVE_NO_PATTERN (fragment-buddy never reported ready=1)"
	RC=3
elif [ "${EXHAUSTED:-0}" != 1 ]; then
	say "VERDICT=INCONCLUSIVE_CAP_REACHED (exhausted=${EXHAUSTED:-unknown} stop=${STOPREASON:-unknown}; ceiling stopped the loop before order-7 was depleted, untouched remainder still holds high orders)"
	RC=3
elif [ "${PAGEMAP:-0}" != 1 ]; then
	say "VERDICT=INCONCLUSIVE_NO_PAGEMAP (holes were virtual, not physical buddy pairs)"
	RC=3
elif [ "${HIGH_ORDER:-1}" -gt 0 ]; then
	say "VERDICT=INCONCLUSIVE_ORDER7_STILL_AVAILABLE (high_order_7plus_blocks=$HIGH_ORDER; pattern did not break contiguity)"
	RC=3
elif [ "$REBOUND" = yes ] && [ "$ACCEPT" -eq 0 ] && [ "$OOPS" -eq 0 ]; then
	say "VERDICT=PASS_RING_ALLOCATION_UNDER_FRAGMENTATION"
	say "VERDICT_SCOPE ordinary-x86_64-vzalloc-path; co-co-chunked-fallback-not-exercised"
	RC=0
elif [ "$ACCEPT" -gt 0 ] || [ "$OOPS" -gt 0 ]; then
	say "VERDICT=FAIL accept4=$ACCEPT oops=$OOPS"
	RC=1
else
	say "VERDICT=FAIL channel did not rebind after fragmentation"
	RC=1
fi

say "--- BUDDY FINAL ---"
buddy

# --- teardown hog -----------------------------------------------------------
kill "$HOGPID" 2>/dev/null || true
wait "$HOGPID" 2>/dev/null || true
say "=== END vmbus-fragmentation-drill ==="
say "log=$LOG"
exit "$RC"
