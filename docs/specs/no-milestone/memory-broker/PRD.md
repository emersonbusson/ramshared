---
slug: memory-broker
title: RamShared Memory Broker (Unified Final)
milestone: —
issues: []
---

# PRD — RamShared Memory Broker (Unified Final)

> SSDV3 STEP 1 **consolidated**. Slug: `memory-broker`. This is the **single PRD** from which the SPEC will be generated. It absorbs and replaces the following as sources: `docs/vram-arbiter/PRD.md`, `docs/dcc-out-of-core/PRD.md`, and `docs/memory-broker/VISION.md`. Critical evaluation of source documents is in Annex A.
> Disciplines: SSDV3 (fact vs. inference per item) + Kahneman (counterfactuals and rollback triggers in §14; anti-halo gates between phases in §10).

## 1. Summary

**A memory tiering platform for a single physical host with a GPU**: a **broker** (arbiter) + **agents** per environment + a **revocable VRAM lease** primitive, serving N consumers using each consumer's native mechanism — the Linux kernel consumes VRAM as **swap** (ublk/NBD, Phase B, ready); the DCC app on Windows consumes **RAM as backing for VRAM** (out-of-core); the arbitration moves capacity to **whoever needs it most** (PSI/pressure), on **any GPU with VRAM** (backend trait: CUDA ready, Vulkan next). **Installable** product: Windows service (`.exe`/winget) + Linux binary (`.deb`/systemd) + Blender addon.

## 2. Technical Context

- **Confirmed in the codebase (Phase B validated in hardware):** VRAM served as a block device via **ublk** (p50 241µs, ~26% faster than NBD 326µs); multi-connection **NBD** (currently Unix socket); **DEMOTE** engine ready (canary §9 latency, probe §9.4 content/free, `ResidencySampler` with hysteresis, `spawn_swapoff`); CUDA via **dlopen** (binary has no libcuda dependency); `BlockBackend` is the bridge — the swap tier has nothing CUDA-specific.
- **Confirmed in docs (civm):** Hyper-V VM `gha-ubuntu-2404` (GitHub Actions runner, label `civm`) on the **same physical host** (`dev-host`) as WSL2 + RTX 2060; reachable via SSH/Tailscale; no GPU.
- **Confirmed in docs (CUDA/Blender):** UVM oversubscription via page faults is **Linux-only** (`cudaMallocManaged` WDDM does not have demand paging); Cycles/CUDA has host memory fallback for scenes that do not fit (exact coverage by backend/version — **to be measured in P0**).
- **Confirmed (tester report):** Alex (3D artist, Windows, no WSL2) loses **days** optimizing scenes manually to fit in VRAM, while RAM is idle; available to test with real scenes.
- **Two personas** *(Confirmed in conversations)*:
  - **Dev/CI (Developer, host dev-host):** WSL2 (compiles) + civm (Actions) compete for memory; VRAM is the extra tier. The brain lives in WSL2 (the stack lives there).
  - **Artist (Alex, Windows desktop):** Blender competes for VRAM with the rest of the system; RAM is the extra tier. **No WSL2** — the brain must run as a native Windows service.
- **Inference (to be validated in P0):** VM → WSL2 connectivity (WSL2 NAT may require Tailscale in WSL2 or port-forwarding); NBD/TCP latency in the virt-switch; out-of-core coverage of OptiX/HIP.

## 3. Recommended Option

**Protocol-first platform; one brain per host; native mechanisms per consumer; GPU via trait.**

- **A single protocol** (agent ↔ broker): register tenant, report pressure, receive commands (swapon/swapoff of slice, lease release/grant, demote-all). The brain becomes a deployment detail: WSL2 on the dev host; Windows service on the artist's host.
- **Revocable VRAM lease** as a universal primitive: all VRAM usage by the swap tier is borrowed; revocation is the already built DEMOTE engine. This is what joins the two worlds: the Blender addon **requests** the VRAM that the swap tier **returns**.
- **Mechanisms per consumer (irreducible, OS/physics constraint):** Linux = block device (local ublk, remote NBD — both ready); DCC = native out-of-core configuration (MVP) and, gated, interposer (v2); Windows-as-swap-consumer = out of scope (would require a Windows disk driver).
- **GPU via trait** (`VramProvider`: alloc/free/read_at/write_at/budget): CUDA (ready) → Vulkan (any card on native Windows/Linux; the tier does not use shaders, only alloc+copy) → D3D12/dxg (research; only plausible path for non-NVIDIA inside WSL2).

**Rejected options:** identical single binary everywhere (mechanisms diverge by OS); UVM-only (does not cover Windows); GPU-P/passthrough on civm (poor consumer GPU support); broker on Windows host for the dev persona (stack is Linux/Day-0; WSL2 is the place on the dev host); static partitioning (does not address "whoever needs it most").

## 4. Functional Requirements

**Broker Core**
- **RF-B1** Agent ↔ broker protocol: `Register(tenant, transport)`, `PsiReport`, `SwapOn/Off(slice)`, `LeaseRequest/Release`, `DemoteAll`, `Status`. Format in the SPEC (JSON-lines vs. length-prefixed).
- **RF-B2** Arbiter: compares pressure between tenants; rebalances with **hysteresis + cooldown** (reusing the standard `ResidencySampler`); never leaves a tenant under pressure with zero slices.
- **RF-B3** **Revocable Lease**: revocation = demote per-slice (reusing `spawn_swapoff` + canary); priority: explicit VRAM request (DCC) > swap tier.
- **RF-B4** Observability: each decision logged with pressure from both sides; `Status` shows slices/tenant, PSI, last rebalance ("each knows who needs it most").

**Linux Tenants (WSL2 + civm)**
- **RF-L1** Slices: `--slices K --slice-mb N`; K independent devices (disjoint offsets in the same `DeviceMem`); dynamic slice → tenant mapping. *(Check in the SPEC if `VramBackend` supports offset/len view — Hard Rule #1.)*
- **RF-L2** **NBD/TCP** listener (`--listen-nbd tcp://IP:PORT`), coexisting with Unix socket; bind only to private interface/Tailscale.
- **RF-L3** Linux Agent: reads `/proc/pressure/memory` + `/proc/swaps`, executes swapon/swapoff.
- **RF-L4** Copiable provisioning runbook for civm (nbd-client + agent + systemd), respecting civm policy (peer copies template; zero host automation).

**Windows / DCC (Phase C)**
- **RF-W1** Windows Agent (native Rust): OS memory pressure + NVML/GPU budget.
- **RF-W2** Blender Addon MVP: fits/does-not-fit prediction (footprint vs. free VRAM), automatic configuration of native out-of-core (backend/flags), optional **non-destructive** proxies/mipmaps, spill/time monitor during render.
- **RF-W3** Addon ↔ broker bridge: "about to render → release VRAM" (LeaseRequest) and return post-render.
- **RF-W4** *(v2, gated — see §10)* Residence interposer (Driver API hook; prefetch/pinning; optional compression in RAM).

**Cross-vendor**
- **RF-G1** `VramProvider` trait extracted from the current layer (CUDA becomes one backend).
- **RF-G2** **Vulkan** backend (`DEVICE_LOCAL` + `VK_EXT_memory_budget` + transfer queue) — unlocks "any card" on native Windows/Linux.
- **RF-G3** *(research)* D3D12/`/dev/dxg` for non-NVIDIA inside WSL2.

**Product**
- **RF-P1** Installables: `ramshared-setup.exe`/winget (Windows service + CLI) and `.deb` + systemd (Linux/WSL2/civm). Native Rust binaries; GPU APIs via dlopen/driver (zero extra dependencies).
- **RF-P2** Transport with fallback: ublk where the kernel supports it (`CONFIG_BLK_DEV_UBLK`); **NBD as universal fallback** (measured: ~26% slower — acceptable).
- **RF-P3** Single configuration file (TOML) per host: tenants, slices, binds, arbiter policy.

## 5. Non-Functional Requirements

- **RNF-1 Anti-D-state (risk #1, which has already bitten us):** remote tenant with swap on a dead broker device = D-state. Mandatory mitigations: remote slices with **lower swap priority** than local swap; **watchdog in the agent** (broker disappeared → immediate best-effort swapoff); removal runbook; validated orderly teardown (QEMU harness — already PASS in Phase B/F2).
- **RNF-2 Security:** NBD/TCP and broker protocol **without native auth** → bind only to private network/Tailscale, never `0.0.0.0`; local-only addon; **zero external telemetry**.
- **RNF-3 Anti-flapping:** hysteresis + cooldown; rare and cheap rebalancing (small slice, bounded swapoff, outside the hot path).
- **RNF-4 Zero regression:** Phase B (single-tenant ublk, NBD Unix) continues passing smoke tests.
- **RNF-5** `unsafe` restricted to FFI crates (`ramshared-uring`, `ramshared-cuda`, future `ramshared-vulkan`); daemon library has `#![forbid(unsafe_code)]`.
- **RNF-6 Day-0:** no shims; each phase delivers the final form of its interface.

## 6. Workflows

1. **CI vs. build (dev persona):** Actions on civm (PSI rises) + `cargo build` on WSL2 → arbiter sees `psi_civm ≫ psi_wsl2` over N samples → swapoff slice on WSL2 → swapon on civm via NBD → invert when pressure inverts.
2. **Artist render:** addon detects scene > free VRAM → `LeaseRequest` to broker → broker demotes swap slices (revokes lease) → render with full VRAM + out-of-core to RAM → end of render → `LeaseRelease` → broker re-leases to swap tier.
3. **Broker dies:** agents detect (watchdog) → best-effort swapoff of remote slices → tenants continue with local swap (RNF-1).
4. **Orderly shutdown:** demote-all → agents confirm swapoff → STOP/DEL (ublk) + close NBD → zero VRAM (reusing validated teardown).

## 7. Data Model

`Tenant{id, transport: Ublk|NbdTcp|DccAgent, psi, slices}` · `Slice{id, offset, len, tenant?, state: Active|Draining|Free}` · `Lease{holder, bytes, revocable}` · `PsiSample{avg10, avg60, stall_us}`. Protocol: format in the SPEC.

## 8. API / Interfaces

- `ramsharedd`: `--slices K --slice-mb N --listen-nbd tcp://IP:PORT --arbiter-listen IP:PORT --transport {ublk,nbd} --backend {cuda,vulkan}` (+ current flags).
- `ramshared-agent`: `--broker IP:PORT --tenant NAME [--swap-prio P]`.
- Blender Addon (Python): talks to local agent/broker.
- Trait `VramProvider { alloc, free, read_at, write_at, budget }` — `BlockBackend` remains the swap tier interface (Confirmed: already agnostic).
- **No new kernel uAPIs** (existing ublk/NBD).

## 9. Dependencies and Risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | VM↔WSL2 connectivity (NAT) *(Inference)* | P0 measures; Tailscale in WSL2 or port-forwarding |
| R2 | **D-state in remote tenant** (dead broker) | RNF-1 (priority, watchdog, runbook) |
| R3 | Arbiter flapping | RNF-3 (hysteresis+cooldown) + counterfactual §14 |
| R4 | NBD/TCP latency in virt-switch | P0 measures; compared to swap in saturated VHDX, civm still profits *(Inference)* |
| R5 | Vulkan inside WSL2 immature (non-NVIDIA) | Honest matrix: CUDA covers WSL2/NVIDIA; Vulkan covers native; D3D12 = RF-G3 research |
| R6 | Fragile `nvcuda.dll` hooking (v2) | v2 gated; degrades to native path (Phase C RNF-4) |
| R7 | `wsl --shutdown` kills the dev host broker | = R2 (watchdog); document |
| R8 | Saturated native out-of-core (OptiX/geometry) | P0 with real scenes decides MVP vs. v2 |
| R9 | Tester availability | Confirmed; Annex B collects context |

## 10. Implementation Strategy (phases with anti-halo gates)

- **P0 — Measurement (no product code):** PSI idle/load in the 3 environments; VM↔WSL2 reachability and RTT; raw NBD/TCP p50/p99; Alex's scenes (Annex B) + native out-of-core behavior. **Gate:** documented numbers; without them, no subsequent phase begins.
- **P1 — Broker Core, Linux↔Linux:** RF-B1..B4, RF-L1..L4 (slices, NBD/TCP, agent, arbiter). Real e2e: action on civm + build on WSL2 with observed rebalancing. **Gate:** Scenario 1 demonstrated with PSI logs; D-state drill (kill broker → watchdog cleans up <5s).
- **P2 — Windows Bridge + DCC MVP:** RF-W1..W3 (Windows agent, MVP addon, lease bridge). **Gate:** ≥1 real scene from Alex that used to fail renders without manual editing; lease revokes/returns VRAM.
- **P3 — Any GPU:** RF-G1..G2 (trait + Vulkan). **Gate:** swap tier smoke tests passing on non-NVIDIA GPU (native).
- **P4 — Gated (numbers-only):** interposer v2 (RF-W4; gate: MVP does not unlock Alex's scene or cost >2× vs. VRAM working set), Windows swap driver, D3D12-WSL2 (RF-G3).

## 11. Documents to Update

`docs/specs/no-milestone/memory-broker/SPEC.md` (**next step**, from this PRD); IMPL per phase; copiable civm runbook; `README`/`ARCHITECTURE` (platform); `MEMORY.md`. Source PRDs remain as history (marked).

## 12. Out of Scope

Blender addon business model (pricing/licensing); custom auth/encryption (private network only); >2 Linux tenants in validation; rewriting Cycles; Windows-as-swap-consumer (disk driver); competing with RAM in latency (PCIe dominates); distributed rendering.

## 13. Acceptance Criteria (Platform)

1. P1: automatic WSL2↔civm rebalancing under real load, with "whoever needs it most" logs and clean D-state drill.
2. P2: Alex's scene that used to fail renders successfully without manual editing; lease revokes the swap tier and returns VRAM.
3. P3: functional swap tier on native non-NVIDIA GPU (Vulkan).
4. RNF-4: Phase B smokes remain green in all phases.
5. Installable product: Windows setup + Linux `.deb` with a single TOML config.

## 14. Validation (Kahneman)

- **Arbiter Counterfactual (#2):** wrong decision = draining whoever needed memory. **Rollback trigger:** PSI of the drained tenant worsens >2× in 60s post-rebalance ⇒ returns the slice + long cooldown (logged).
- **Lease Counterfactual (#2):** revoking swap for a render that doesn't use the VRAM is a loss. **Trigger:** VRAM usage of the requester <50% of lease in 5min ⇒ returns slice to swap tier.
- **Anti-halo (#11):** each phase only starts with the numeric gate of the previous one; Phase B success does not "approve" the broker — P0/P1 must prove themselves with their own numbers.
- **Worst-case (#5):** mandatory D-state drill in P1 (kill broker with active remote swap in a disposable environment — extended QEMU harness, never on host).

---

## Annex A — Evaluation of Source Documents (what changed during consolidation)

1. **`vram-arbiter/PRD.md`** — solid in topology and reuse of demote. **Corrected gaps:** (a) did not handle WSL2 lifecycle (`wsl --shutdown` kills brain → R7/RNF-1 watchdog); (b) assumed brain fixed in WSL2 — protocol-first makes the brain a deployment detail (artist persona has no WSL2); (c) D-state was a listed risk, here it became **RNF + mandatory drill** (P1), because it has already bitten us once.
2. **`dcc-out-of-core/PRD.md`** — correct in measurement-first and cheap MVP. **Corrected gaps:** (a) the isolated MVP did not talk to the broker — RF-W3 (lease bridge) is what joins the fronts and resolves real VRAM contention on the artist's host; (b) "any GPU" also holds for DCC (Cycles HIP/oneAPI for AMD/Intel — to be measured in P0); (c) v2 received an explicit numeric gate.
3. **`VISION.md`** — correct direction (platform, lease, cross-vendor, packaging), without SSDV3 rigor. This PRD operationalizes it: numbered RF/RNF, risks with mitigation, phases with gates, Kahneman validation. VISION remains as context reading.
4. **Resolved Conflict:** Phase B serves VRAM→OS and Phase C requests VRAM for the app — on the same host they would compete blindly for the GPU. The **revocable lease** (RF-B3) is the resolution: a single source of truth for VRAM.

## Annex B — Questions for the Tester (P0, context requested by Alex)

1. OS and version (Windows 10/11), GPU (model/VRAM), total RAM.
2. Blender version and backend (OptiX or CUDA — if known; if not, send screenshot of Preferences → System).
3. 1-2 `.blend` files (or description: number of objects, textures, and resolutions) that **failed** due to VRAM + the exact error message.
4. What was tried manually (reducing textures, decimate, tiles, passes) and how much time was lost.
5. Are you open to running our measurement script (reads VRAM/RAM during render; **does not alter the scene**)?
