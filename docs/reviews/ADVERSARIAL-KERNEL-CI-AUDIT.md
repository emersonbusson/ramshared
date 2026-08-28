# Adversarial Hang, Failure Mode & Kernel CI Defense Audit

> **Document purpose:** Deep architectural and adversarial failure-mode analysis
> for RamShared's kernel-level drivers, `ublk` block devices, PCIe DMA pathways,
> and continuous integration pipelines. Grounded in **Kahneman disciplines
> (#1–#18)** to systematically eliminate false-green tests, deadlocks, and silent
> data corruption.

---

## 1. Threat Model & Adversarial Philosophy

In low-level kernel development and hardware-adjacent systems, conventional
testing often suffers from **WYSIATI** (*What You See Is All There Is* — Kahneman #1).
A test suite that reports green (`exit 0`) frequently conceals catastrophic
latent failure modes: unexercised fallback branches, silent driver unloads,
unbounded page-fault stalls, or race conditions that only manifest under memory
pressure.

```text
┌──────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                ADVERSARIAL VERIFICATION CYCLE                                    │
├──────────────────────────────┬─────────────────────────────────┬─────────────────────────────────┤
│    1. Attack Formulation     │      2. Empirical Stress        │     3. Deterministic Defense    │
├──────────────────────────────┼─────────────────────────────────┼─────────────────────────────────┤
│ • Assume component failure   │ • Inject surprise GPU loss      │ • Write-through zero-loss sync  │
│ • Assume latency spikes      │ • 99% Host RAM overcommit       │ • Asynchronous timeout watchdogs│
│ • Assume race conditions     │ • Lock inversion stress drills  │ • LOCKDEP + lock order matrices │
│ • Assume false-green CI      │ • Mocked driver crashes         │ • Cryptographic SHA-256 checks  │
└──────────────────────────────┴─────────────────────────────────┴─────────────────────────────────┘
```

---

## 2. Adversarial Failure Modes & Concrete Defenses

### Failure Mode 1: The "False-Green" Driver Test (Kahneman #13)
* **The Attack:** A test executes `modprobe ramshared_ublk`, the module fails
  silently or fails to register `/dev/ublkb0`, but the test script ignores `stderr`
  or tests a mock fallback, reporting `PASS`.
* **Adversarial Risk:** Deploying broken drivers that panic user systems.
* **Deterministic Defense:**
  1. Every integration test must assert explicit device existence in `/dev/` and
     `/sys/class/block/`.
  2. Perform synchronous cryptographic I/O: write a 256 MiB deterministic
     pseudo-random block stream, flush caches (`echo 3 > /proc/sys/vm/drop_caches`),
     read back via `O_DIRECT`, and assert 100% SHA-256 match.

---

### Failure Mode 2: The Silent PCIe DMA Buffer Race (TOCTOU & UAF)
* **The Attack:** A userspace process requests asynchronous I/O over pinned host
  memory (`cuMemHostAlloc`). While the PCIe DMA controller is executing transfer,
  the process terminates, or memory is freed, leading to Use-After-Free (UAF) or
  DMA writing into unallocated physical memory.
* **Adversarial Risk:** Kernel memory corruption, privilege escalation, or host
  crash.
* **Deterministic Defense:**
  1. Strict reference-counted buffer lifetime: pages remain locked until the GPU
     completion event triggers the unpin callback.
  2. Complete fence synchronization barriers before freeing device context.
  3. Validated by `sparse` address-space checking and runtime `KASAN` tracking.

---

### Failure Mode 3: The Sudden GPU Revocation / WDDM Preemption Stall (Kahneman #16)
* **The Attack:** While RamShared is actively serving virtual memory swap pages
  from VRAM, the host OS (Windows/WDDM) abruptly revokes GPU memory to launch a
  3D application or display driver reset.
* **Adversarial Risk:** Indefinite I/O stall (1.18 s+ freeze) or kernel panic on
  missing swap pages.
* **Deterministic Defense:**
  1. **Write-Through Architecture (EVD-0038):** Every modified page is synchronously
     mirrored to authoritative primary SSD storage before acknowledgement.
  2. **Asynchronous Circuit Breaker:** If GPU transfers exceed 200 ms timeout,
     the memory broker instantly demotes the tier and redirects reads to SSD
     origin without generating I/O errors to calling processes.

---

### Failure Mode 4: Static Linter Blind Spots & Lock Order Inversions (ABBA Deadlock)
* **The Attack:** C code passes standard compiler checks and formatters, but
  contains nested locking where Function 1 acquires `Lock A -> Lock B` and
  Function 2 acquires `Lock B -> Lock A` during error cleanup.
* **Adversarial Risk:** Unrecoverable system hang under multi-threaded contention.
* **Deterministic Defense:**
  1. Mandatory `smatch` semantic analysis checking lock balances on all error
     return paths.
  2. Execution of QEMU smoke tests with `CONFIG_PROVE_LOCKING=y` and `CONFIG_LOCKDEP=y`
     enabled to detect potential circular dependencies before execution.

---

### Failure Mode 5: CI Flakiness & Uncalibrated Retries (Kahneman #15)
* **The Attack:** A test encounters a race condition or memory leak, and CI is
  configured with generic retries (`retry: 3`). The second run passes by chance,
  masking the bug.
* **Adversarial Risk:** Flaky releases and untraceable production regressions.
* **Deterministic Defense:**
  1. **Zero Retries for Logic Bugs:** Retries are strictly banned for unit,
     integration, and static analysis gates.
  2. Retries are restricted exclusively to proven transient external infrastructure
     signatures (e.g. GitHub Actions package mirror 503).
  3. Every failed run generates complete post-mortem diagnostics (`dmesg`, core dumps,
     memory map snapshots).

---

## 3. Quantitative Rollback & Defense Matrix

| Component | Quantitative Threshold | Breach Action |
| :--- | :--- | :--- |
| **Data Integrity** | `< 100% SHA-256 match` (even 1 bit flip) | Immediate hard rollback & build rejection |
| **Memory Latency** | `p99 4KB read > 500 µs` under idle | Automatic demotion of candidate |
| **Kernel Log Audit** | `> 0` occurrences of `BUG:`, `WARNING:`, `Oops:` | Immediate CI failure |
| **Slice Coverage** | `< 80.0%` line coverage on modified files | PR admission blocked by gate |

---

## 4. Permanent Codespace Knowledge & AI Directives

To maintain maximum technical rigor across all autonomous coding sessions:

1. **Verify Before Declaring Green:** Never rely on exit codes alone; inspect
   underlying state, byte hashes, and hardware counters.
2. **Assume Component Failure:** Every memory tier must have an independent,
   fail-safe fallback that operates without panic when hardware disappears.
3. **Strict Language & Clean State:** English across all commits, docs, and code;
   zero dirty working tree state left unmanaged.

---

## 5. Automated Adversarial Invariant Verification Gate

To ensure that every invariant is machine-checked on every commit, RamShared
executes [`scripts/ci/check-adversarial-invariants.sh`](../../scripts/ci/check-adversarial-invariants.sh):

```bash
./scripts/ci/check-adversarial-invariants.sh
```

### Verified Invariant Checklist

- [x] **Invariant 1 (Kahneman #15):** Zero uncalibrated generic retries in CI workflows.
- [x] **Invariant 2 (Buffer Security):** Zero banned unsafe string functions (`strcpy`/`sprintf`) in C drivers.
- [x] **Invariant 3 (Code Hygiene):** Zero trailing whitespace in source code.
- [x] **Invariant 4 (Language Standard):** 100% English comments across all git diffs.
- [x] **Invariant 5 (Kahneman #13):** Append-only validation log integrity verified against schema v2.
- [x] **Invariant 6 (Documentation Rigor):** Complete adversarial audit and defense documentation.
