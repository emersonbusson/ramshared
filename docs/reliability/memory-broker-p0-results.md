# P0-RESULTS — RamShared Memory Broker (P1 Numeric Gate)

> SSDV3 STEP 3 / ITEM-1 of [`SPEC.md`](../specs/no-milestone/memory-broker/SPEC.md). **This file is the anti-halo gate (#11): no P1 item (ITEM ≥ 3) starts while any mandatory cell is empty or "estimated".** Discipline #3 (number, not adjective): each cell = number + unit + n of rounds + date + environment. Scripts: `scripts/p0/`.

## Gate Status

**P1 Gate: ✅ PASS on the calibrated RTX 2060 surface.** Closed:
**§1 PSI** (idle+load, WSL2+civm), **§2 R1 network** (port-forward
decision), **§3 NBD/TCP** (loopback + **cross-host p50 644 µs**),
**§4 aggregate external CUDA pressure with daemon DEMOTE and integrity
correlation**, and **§5 calibration** (proposes `delta_psi=10`). The later
multi-tenant `eviction` reconciliation flag remains a separate environment-bound
refinement and is not inferred from this gate.

## Environments

| Tag | Host/VM | Role | PSI (`/proc/pressure/memory`) | PAGE_SIZE | Date |
| --- | --- | --- | --- | --- | --- |
| WSL2 | dev-host / WSL2 (`6.6.123.2-microsoft-standard-WSL2+`) | tenant dev (brain) | **enabled** (CONFIG_PSI=y, readable) | 4096 B | 2026-06-13 |
| civm | `gha-ubuntu-2404` (Hyper-V, kernel `6.8.0-124-generic`) | tenant CI | **enabled** (`some`/`full` readable; some.avg10 0.5–7.8 according to CI load) | 4096 B | 2026-06-13 |
| host | dev-host (Windows + RTX 2060) | external GPU workload | N/A (Windows) | N/A | 2026-07-17 |

## 1. PSI by Environment (idle / load) — `measure-psi.sh`

Arbitration metric = `some` line, `avg10` (DT-15). Gate requires **≥3 runs** per cell (idle: 300 s; load: during `cargo build -j4` in WSL2 / real action on civm).

| Environment | Scenario | some.avg10 mean | some.avg10 max | full.avg10 max | n runs | Date |
| --- | --- | --- | --- | --- | --- | --- |
| WSL2 | idle | 0.011 (3 runs, 831 samples) | 0.55 | 0.00 | 3 × ~300 s | 2026-06-13 |
| WSL2 | load (mem., cgroup hog) | **14.25** | 22.54 | 18.26 | 40 (confined) | 2026-06-13 |
| civm | idle / CI (natural) | 1.237 (3 runs, 806 samples) | **19.44** (burst CI) | 7.75 | 3 × ~300 s | 2026-06-13 |
| civm | load | CI bursts up to ~19 (line above); confined hog **not** run on CI VM (restriction) | — | — | obs. | 2026-06-13 |

> **Load = `scripts/p0/measure-psi-load.sh`** (anonymous hog confined in cgroup v2, `memory.max` ceiling + `swap.max=0`; 0 OOM, cgroup cleaned post-test). The SPEC said "cargo build -j4", but P0 found that build is **CPU-bound** and does not generate memory PSI → replaced by confined hog (P0 methodology correction). **Caveat (#1 WYSIATI):** confined load is a **lower bound** of real PSI (only the cgroup starves) → real host pressure yields some.avg10 **≥ 14**. CSVs in `/tmp`.

## 2. VM↔WSL2 Network (reachability / RTT) — `measure-net.sh`

| Direction | Transport | RTT p50 (ms) | RTT p99 (ms) | Port (TCP:22 test) | n (ping) | Date |
| --- | --- | --- | --- | --- | --- | --- |
| WSL2 → civm | LAN (192.168.0.50) | 0.375 | 0.849 | open | 50 | 2026-06-13 |
| WSL2 → civm | Tailscale (100.123.103.106) | 1.02 | **430** | open | 50 | 2026-06-13 |
| civm → WSL2 | direct (NAT 172.31.230.209) | — | — | **100% loss (NAT)** | 5 | 2026-06-13 |
| civm → WSL2 | Tailscale | N/A | N/A | **WSL2 is not a Tailscale node** (no TS IP) | — | 2026-06-13 |

**Transport Decision (R1): port-forward on Windows host.** The critical direction (civm agent → WSL2 broker) is **blocked by NAT** — WSL2 at 172.31.x, `ping` from civm = **100% loss** (`ip route get` on civm routes 172.31.x to LAN gateway, which does not know the subnetwork) — and **WSL2 is not a Tailscale node** (no TS IP by any method). Tailscale-on-host has a **bad tail (p99 430 ms vs. LAN 0.85 ms)**, unviable for the swap data-plane (Phase B: 241–326 µs). → use `netsh portproxy` on `dev-host` (LAN:port → 172.31.230.209:port) for `--arbiter-listen` and `--listen-nbd`. ITEM-12 (runbook) and DT-25 (endpoints) follow this. WSL2 gw/host vNIC = 172.31.224.1.

## 3. Raw NBD/TCP in the Virt-Switch — `measure-nbd-tcp.sh`

Honest baseline (no custom code). Compare with Phase B: **p50 241 µs (ublk) / 326 µs (NBD-Unix)**.

| Path | Mode | p50 (µs) | p99 (µs) | IOPS | stddev | n runs | Date |
| --- | --- | --- | --- | --- | --- | --- | --- |
| NBD/TCP loopback | randread 4k | 174 | 285–578 | ~5200 | p50 spread 169–188 | 3 | 2026-06-13 |
| NBD/TCP loopback | randwrite 4k | 202 | 351–3228 | ~4200 | p50 spread 182–225 | 3 | 2026-06-13 |
| NBD/TCP WSL2↔civm | randread 4k | 644 | ~1250 | ~1450 | p50 611/676/644 | 3 | 2026-06-13 |
| NBD/TCP WSL2↔civm | randwrite 4k | 644 | ~1100 (r1 2278) | ~1400 | p50 742/644/644 | 3 | 2026-06-13 |

> **Loopback ≠ virt-switch.** Loopback p50 ~174 µs = floor (no network). **Cross-host MEASURED** (civm → `netsh portproxy` on host → WSL2 `nbdkit`, 3 runs): **p50 644 µs**, p99 ~1.0–1.5 ms. = floor + ~470 µs from virt-switch/portproxy (≈ 1 LAN RTT per NBD op; RTT p50 0.375 ms). **R4 confirmed:** 644 µs per 4k is way below swap on saturated disk (ms+) → civm profits using remote VRAM as swap (PRD Inference is now a number). Setup/teardown: nbdkit in WSL2 (userspace), portproxy+firewall on host (removed after), nbd-client+fio on civm (`-timeout 30`).

> Current measurement host: has `nbd-client` + `fio`, does **not** have `nbdkit`/`nbd-server` (`sudo apt install nbdkit` before running — preflight in script, F17).

## 4. External GPU Workload VRAM/RAM — `scripts/p0/measure-gpu-workload-vram.ps1` + `scripts/p0/Invoke-GpuWorkloadGate.ps1`

| Workload | GPU/VRAM | Max VRAM used (MiB) | Min RAM available (MiB) | Result (fit? spill?) | Date |
| --- | --- | --- | --- | --- | --- |
| generic CUDA VRAM workload, 1024 MiB hold 35 s | RTX 2060 / 6144 MiB | 2648 loaded peak (idle peak 1525; recovery peak 1540) | captured in artifact JSON | PASS: aggregate VRAM pressure observed and recovered near idle | 2026-07-17 |
| daemon DEMOTE correlation under external pressure | RTX 2060 / 6144 MiB | 5607 MiB used peak; 348 MiB free minimum | integrity worker verified all allocated chunks; 2 DEMOTEs | PASS: `GlobalGpuFreeFloor` observed under a generic 4096 MiB external workload, checksums matched, and teardown was clean | 2026-07-22 |

> **Sampler validated on dev-host** (RTX 2060): captures VRAM (nvidia-smi) + RAM free OK (e.g. VRAM 2015→1828 MiB / 6144, RAM free ~3670 MiB). **Bug caught in validation** (rule "run on host first", #13): `Get-Counter '\Memory\Available MBytes'` is **localized** and breaks on pt-BR Windows → swapped for CIM `Win32_OperatingSystem.FreePhysicalMemory` (locale-neutral). 2026-07-17 artifact: `C:\ramshared\artifacts\gpu-workload-gate-20260717-224420`; the workload is synthetic and app-agnostic by design, so it proves WDDM/CUDA VRAM pressure and recovery, not process attribution.

The daemon correlation row is closed by
`C:\ramshared\artifacts\shared-wsl-pressure-20260722-015303`: a 4096 MiB
external CUDA workload drove global free VRAM to 348 MiB, the daemon recorded
two `GlobalGpuFreeFloor` DEMOTEs, the integrity worker retained matching
checksums, and the terminal health snapshot had no daemon, NBD/VRAM swap, or
ghost entry. This does not claim per-process attribution.

## 5. Arbiter Defaults Calibration (ITEM-4)

Provisional defaults become final when these cells close (recalibration = update of SPEC + commit citing this file).

| Parameter | Provisional Default | Calibrated Value | Base (cell) |
| --- | --- | --- | --- |
| `delta_psi` | 15.0 | **propose 10.0** (validate in e2e P1) | idle Δ ~0–1 (WSL2 0.011, civm 1.237 → do not trigger); WSL2 load **14.25** vs. civm idle ~1.2 ⇒ Δ≈13 → with 15 **would not move** under clear pressure. delta_psi=10 + streak=5 captures sustained pressure (≥10) and ignores idle noise + transient CI bursts |
| `streak` | 5 ticks (10 s) | **keep 5** | filters transient CI bursts on civm (spikes up to 19.4 that do not last 10 s) without losing sustained load |
| `cooldown` | 60 s | 60 s (fixed, PRD §14) | — |
| `psi_floor` | 5.0 | **OK** | WSL2 idle ~0.01 and civm ~1.2 (both <5); load ≥14 (>5) → separates idle from real pressure |
| `cf_window`/`cf_factor`/`cf_cooldown` | 60 s / 2.0 / 300 s | fixed (PRD §14 trigger) | — |

## 6. Telemetry & Reconciliation (feature `broker-telemetry-reconciliation`)

Numbers from session 2026-06-16 (`docs/specs/no-milestone/broker-telemetry-reconciliation/`). Discipline #3 (number) + #1 (state).

| Item | Value | Unit | Environment | Date |
| --- | --- | --- | --- | --- |
| VRAM `total` / `free` / `used` | 6143 / 5040 / 1103 | MiB | RTX 2060, WSL2, desktop in use | 2026-06-16 |
| `vram_alloc_daemon` (test) | 64 | MiB | idem (alloc of the test) | 2026-06-16 |
| **`vram_outros`** (graphics by subtraction) | **1039** | MiB | idem | 2026-06-16 |
| `reconcile_delta` under normal swap | **≈ -1.0** (occupied ≈ 0 ≤ borrowed) | frac | QEMU broker RAM drill | 2026-06-16 |

- **Real gauge (RF-3):** `vram_gauge_outros_captures_real_graphics_usage` (`backend.rs`, `--ignored`) — real `mem_info` → `vram_outros=1039 MiB` captures desktop graphics usage (external consumer signal).
- **`tol_frac`/`streak` calibration (DT-7):** `tol_frac=0.10`, `streak=3` **provisional and safe** — `Unaccounted` only triggers if `occupied > borrowed · (1+tol)`; under normal operation `occupied ≤ borrowed` (`delta ≤ 0`; ~-1.0 in drill), so **no false positive**. Boundary unit-tested. Exact distribution under real load = refinement in civm e2e (does not block).
- **JSONL e2e:** QEMU broker drill with `--telemetry-jsonl` → `KTEST-TELEMETRY=ok` (live daemon writes the line inside isolated VM).
- **Pending (civm/GPU):** `eviction` flag under real WDDM load (canary triggering) — env-bound (GPU+daemon).

## Gate Closure Checklist

- [x] §1 PSI WSL2: idle (0.011) + **load (14.25)** ✓
- [x] §1 PSI civm: idle/CI (1.237, burst 19.4) + PSI enabled + PAGE_SIZE 4096 ✓
- [x] §2 RTT/ports in both directions + **transport decision (port-forward)**
- [x] §3 NBD/TCP: loopback (p50 174 µs) + **cross-host (p50 644 µs, 3 runs)** ✓
- [x] §4 aggregate external GPU workload VRAM/RAM (generic CUDA workload; idle/load/recovery PASS)
- [x] §5 calibration: **`delta_psi=10` proposed** (validate in e2e P1), `streak`=5, `psi_floor`=5 OK
- [x] **Gate P1 → CLOSED** (daemon DEMOTE correlation under external pressure,
  integrity, and clean terminal state captured on 2026-07-22)
