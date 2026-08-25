# Architecture

RamShared's current source candidate models **idle GPU memory** as a clean,
revocable cache for an SSD-authoritative cold tier. It also models WSL2
control-plane containment independently from managed workload pressure.

## Current boundary — inactive candidate / disabled staging

No architecture in this document is an installed product path. The candidate
is inert: it authorizes no quickstart, package or boot installation, WSL
configuration/application, VM lifecycle, storage/VHDX/GPU/device action, or
pressure campaign. Historical measurements describe past builds only. The
legacy full-VRAM NBD source composition is removed; future activation still
requires fresh qualification and separate attended approval.

Two candidate tracks share CUDA/block pieces. Neither is currently active.

| Track | Status | Where we test |
| --- | --- | --- |
| Linux / WSL2 cascade | Source candidate · disabled | Static tests and isolated-lab evidence only |
| Windows StorPort | Supervised beta candidate · disabled | Historical VM evidence; physical revalidation open |

---

## Track 1 — Linux / WSL2 candidate topology

```text
Pressure  →  zram                 priority 200   hot
          →  RamShared NBD        priority 100   cold logical tier
                ├─ SSD origin                    authoritative
                └─ VRAM cache                    clean + revocable
          →  WSL swap VHDX        priority  -2   last resort
```

**Why cold?** Under WDDM reclaim, a 4 KB read measured ~**1.18 s**. Behind
zram, only cooler pages reach the RamShared device. Acknowledged writes reach
the origin and `sync_data` before cache mutation, so VRAM allocation failure or
revocation changes performance, not block correctness.

**Candidate give-back policy:** the controller would drop clean 128 MiB cache
chunks after three restricted samples. It would grow by at most one chunk every
two seconds after three healthy samples. Missing WDDM/GPU measurement sets the
physical target to zero. Origin detach retains swapoff-first ordering.

**Lifecycle observability:** schema v4 derives topology plus independent
`control_state`, `origin_state`, `cache_state`, `guardian_state`, and
`overall_state`. A usable cache cannot mask pressure, a stale guardian, or an
origin failure.

**Invariant A1:** demotion is valid only if something lower can absorb pages
(disk swap or enough free RAM). The candidate checks this before any lifecycle
transition; this document supplies no transition command.

On WSL2, Windows WDDM/VidMm remains the memory authority. The physical target
is the minimum of logical capacity, the sealed cache cap, and the measured
budget after external use and `max(2 GiB, 20% total VRAM)` headroom. More than
one dxg adapter is rejected until CUDA↔LUID identity is proven.

### Control-plane containment

The candidate reserves all admitted workloads from one
`ramshared-workloads.slice` budget. For a 16 GiB guest, its design ceiling is
12 GiB with `MemoryHigh` approximately 10.4 GiB; simultaneous scopes cannot
reuse one `MemAvailable` snapshot. Docker/containerd/BuildKit and cron would
be child slices under that ceiling. The candidate control units would occupy
`ramshared-control.slice` with protected memory and high CPU/I/O weights.

The one-second supervisor's candidate policy closes admission in `GUARDED`,
shrinks cache and freezes the lowest-priority discardable scope in `CRITICAL`,
and uses a bounded scope TERM/KILL sequence in `EMERGENCY`. A future guardian
would bind only a sealed, explicitly named distro after a stale heartbeat, two
failed guest probes, a failed independent WSL/HCS probe, and closed host
evidence. It writes safe mode first and never reboots Windows.

### Main pieces

| Piece | Job |
| --- | --- |
| `ramshared` CLI | candidate lifecycle, schema v4 status/monitor, managed run/session, recovery |
| `ramsharedd` | candidate SSD-origin NBD server and revocable-cache manager |
| `ramshared-tier` | priority order + safety net |
| `ramshared-cuda` | load NVIDIA driver at runtime |
| `ramshared-dxg` | query the host-authoritative WDDM budget |
| `ramshared-supervisor.service` | candidate one-second preventive pressure policy |
| Windows guardian | candidate independent liveness proof, evidence, targeted recovery, safe mode |
| `ramshared-cascade.service` | disabled definition; no boot installation or activation route is documented here |

### Anti-hang rules (learned the hard way)

1. Never detach the origin daemon while NBD/ublk remains in the swap table.
2. Cache chunks are clean and independently revocable; origin detach is
   swapoff-first.
3. Refuse a lifecycle transition on ghost `(deleted)` swap or an
   unsealed/non-block PARTUUID.
4. Missing guardian, GPU measurement, or supervisor status is never green.
5. Any future boot design is host-gated and safe-mode fail-closed; its staging
   definition remains disabled.

Related design record: [docs/specs/no-milestone/wsl2-cascade-boot/](docs/specs/no-milestone/wsl2-cascade-boot/)

---

## Track 2 — Windows supervised beta

The candidate native path is a StorPort virtual disk whose I/O is completed by
`RamSharedWinSvc`. A separate least-privilege `RamSharedBroker` SCM service
owns only logical lease arbitration. The consumer depends on the broker and
uses an authenticated local named-pipe boundary; no TCP listener is part of the
candidate. Both services and their immutable configs are selected by one hashed
product manifest and remain disabled pending qualification.

**Hard rule:** never tear the disk down under a **hot** pagefile (BugCheck **0x7A** proven). Ordered teardown (DT-9) refuses that.

Physical execution remains prohibited for this candidate. Any future physical
qualification would need explicit approval, watchdog supervision, a
production-trusted signature, and normal-Windows proof with Test Mode disabled.

---

## Process

Structural work uses **SSDV3** (PRD → SPEC → IMPL).  
We write failure modes in [docs/reliability/DEGRADATION-MATRIX.md](docs/reliability/DEGRADATION-MATRIX.md).  
We don’t thrash the live WSL you work in for “fun” benchmarks.
