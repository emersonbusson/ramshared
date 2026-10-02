# COCO-LAB-RUNBOOK — closing COCO-1..5 on rented confidential hardware

Companion to [`COCO-GAP.md`](COCO-GAP.md). That file is the gate; this is the
procedure. It changes no gate: the series stays a draft until the evidence
below exists on real hardware.

Nothing here has been executed. The local host is a Ryzen 5 3600 (Zen 2,
Matisse) and cannot run any of these platforms. Everything below is
preparation so that an authorized operator can close four of the five rows in
one session instead of discovering the shape of the work while the meter runs.

## What the rows actually need

| Row | Platform | Evidence |
| --- | --- | --- |
| COCO-1 | AMD SEV-SNP | Chunked allocate → per-chunk decrypt → `vmap()` → use → teardown re-encrypts or retains. Guest stays alive. |
| COCO-2 | Intel TDX **without paravisor** | Same as COCO-1. A paravisor result does **not** close this row. |
| COCO-3 | Arm CCA | Same as COCO-1. No cloud SKU exists at time of writing. |
| COCO-4 | All of the above | A failed or unknown page-state transition **retains** the buffer (`gpadl.leak`), observed at runtime, not only read from source. |
| COCO-5 | All of the above | An injected page-state failure leaves the guest alive and the buffer retained. No oops, no unbounded leak under a counted owner. |

COCO-1, COCO-2, COCO-4 and COCO-5 are obtainable from cloud guests. COCO-3 is
not, and stays open after this runbook is used.

## Why the chunked path runs only there

`vmbus_needs_shared_pages()` is `!confidential && (hv_isolation ||
cc_platform_has(CC_ATTR_GUEST_MEM_ENCRYPT))`. On an ordinary x86_64 guest that
predicate is false, so `vmbus_alloc_buffer_owned()` takes the single `vzalloc()`
path and the chunked order-N → order-0 degrade never runs. That is exactly why
the sixteen hosted guest-runs do not close COCO-1: they prove ring lifecycle
and fragmentation resilience on the `vzalloc()` path, not the confidential one.

On a SEV-SNP or TDX guest the predicate is true, the allocator walks
physically contiguous chunks, calls `set_memory_decrypted()` on each chunk's
direct-map address **before** `vmap()`, and the same drill code exercises the
path the gap is about. The static invariants INV-1..INV-6 already prove no
`set_memory_decrypted()` can be reached with a `vmap()` address; this run
proves the transitions actually work on silicon.

## Provisioning

Azure Confidential VMs expose the target platforms at an hourly rate. Rent,
do not buy.

| Platform | SKU family | Closes |
| --- | --- | --- |
| AMD SEV-SNP | `DCasv5` / `ECasv5` | COCO-1, COCO-4, COCO-5 |
| Intel TDX (no paravisor) | `DCesv5` / `ECesv5` | COCO-2, COCO-4, COCO-5 |
| Arm CCA | *none* | COCO-3 — needs partner or lab hardware |

Notes that matter before spending:

- Confirm in the guest that the confidential feature is actually on
  (`dmesg` must show SEV-SNP or TDX activation). A SKU name is not evidence.
- TDX must be verified as **no paravisor**. If a paravisor is present the run
  is still useful, but it does not close COCO-2 and must be labelled so.
- Use a disposable guest. The drills rebind the synthetic NIC and unload
  VMBus sub-drivers. They are not for a machine with state on it.
- Budget a few hours, not days. The hosted drills complete in minutes; the
  cost is provisioning and kernel boot, not test time.

## Getting the candidate into the guest

The bytes must be the exact nine-patch series, not a convenience build.

1. Build from the contribution fork at the pinned series commit against
   mainline base `93f51579e7df` ("Linux 7.3-rc4"). The `hyperv-drill-kernel`
   CI job is the reference build and already emits `bzImage`, `BOOTX64.EFI`
   and `kernel-version.txt`.
2. Verify the nine patches against `series/SHA256SUMS` in the guest before
   building or booting. The hosted runs do this as `series-byte-pin.log` and
   report `OK` per patch; reproduce that, do not assume it.
3. Boot the guest with `console=ttyS0,115200` so the drill output is capturable
   the same way the hosted runs capture it.

## Running

Both drills refuse to run on the daily WSL2 host: `guard()` matches
`microsoft-standard-WSL2` in `/proc/version` and exits 2. It also requires
`/sys/bus/vmbus` and visible VMBus devices. An Azure Confidential VM that
booted the candidate kernel satisfies all three checks and proceeds.

```sh
# 1. Lifecycle: 100 rebind cycles, UIO mmap incl. hold-in-mmap
./scripts/kernel/vmbus-lifecycle-drill.sh 100 /var/tmp/coco-lifecycle.log

# 2. Fragmentation: buddy drill to order-7 exhaustion, channel open must survive
./scripts/kernel/vmbus-fragmentation-drill.sh /var/tmp/coco-fragment.log
```

Safety contract is unchanged and applies in the rented guest the same way: no
swap activation, no memory pressure, no RamShared lifecycle state change. The
fragmentation drill is the only pressure, and it lives inside the disposable
guest.

## What to capture per row

**COCO-1 / COCO-2 (chunked path works).**
- `dmesg` line proving the platform (SEV-SNP or TDX, and for TDX no paravisor).
- The allocator took the chunked path, not `vzalloc()`. The chunk count and
  the order-descent behaviour must be visible in the drill output.
- `PHASE1-BEFORE` / `PHASE1-AFTER` map accounting returns to the exact
  baseline across 100 rebind cycles.
- Teardown re-encrypts cleanly: no `permanent_leak`, no `encryption_unknown`
  without an accompanying injected failure.
- Zero `#GP`, `#VC`, oops or guest panic across the whole run.

**COCO-4 (unknown state retains).**
- A failed or indeterminate page-state transition sets `gpadl.leak` /
  `owner->encryption_unknown` and the buffer is **retained**, not freed and
  not re-encrypted.
- Observed at runtime: the map is still present after teardown when the
  transition failed. Source agreement is not enough.

**COCO-5 (injected failure is survivable).**
- Force a page-state failure (the KUnit fault-injection hooks are the model;
  the runtime equivalent must fail the transition, not the allocation).
- Guest stays alive. Buffer retained. `permanent_leak` may be set; an
  unbounded leak under a counted owner may not.

## Recording the result

- Append the run identity (SKU, platform proof line, kernel version, series
  commit, `SHA256SUMS` verification) to `validation.md` — **check the 1 MiB
  append-only cap first**; the log was at 38 bytes of headroom and a rotation
  decision is owed before any new entry.
- Mark the closed rows in [`COCO-GAP.md`](COCO-GAP.md) and in the vmbus row of
  [`GAP-REGISTER.md`](../../../reliability/GAP-REGISTER.md). Do not close
  COCO-3.
- Per [`benchmarks.md`](../../../../.claude/rules/benchmarks.md), any latency or
  throughput number cited afterwards is a registered benchmark: ≥3 runs,
  median + p99 + deviation, same load snapshot.

## If only one platform is affordable

COCO-1 on `DCasv5` closes the largest share: it exercises the chunked path on
the platform Kelley's objection named first, and it can carry COCO-4 and
COCO-5 in the same session. COCO-2 is the row Kelley named explicitly
(no-paravisor TDX), so it is the second priority. COCO-3 remains open either
way and must be stated as open in the cover letter.

## Rollback trigger

If a run shows `#GP`, `#VC`, guest panic, or a buffer freed while its page
state was unproven, the series is **not** ready and the send gate stays
closed. Record the failure as evidence rather than retrying around it; that
failure mode is what the `gpadl.leak` design exists to prevent, so it is a
finding about the design and not only about the run.
