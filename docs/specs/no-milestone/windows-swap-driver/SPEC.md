# SPEC — RamShared P4 / Track 2: native Windows swap-to-VRAM (StorPort virtual miniport)

> **Single-file RamShared model:** revision history remains in this path's `git log` — no `SPECvN.md`.
>
> **Reviewed after the Step 2.5 audit (2026-07-08):** no-go in round 1 → in-place corrections; re-audit = **GO**.
> Re-audit findings: only **LOW** (L1–L4, no new structural decision). C1–H4 from round 1 were re-verified and **closed** in this same file.
>
> **Active candidate for IMPL (Step 3):** this file. Residual product gate: EV/Partner Center (R9)
> process remains in progress before loading on the real host / ITEM-11; no driver on the real host before
> ITEM-8 (kernel-page) in a VM.
>
> Reason for the round-1 no-go: 2 structural CRITICAL + 4 HIGH findings (invented paths vs real code). Blocking findings addressed **in place**:
>
> - **C1** (RNF-5 / ITEM-8 Map / `Invoke-RevokeDrill`): the SPEC claimed that the broker signals revocation
>   to the lease holder. **False in code:** no broker→holder force-revoke `Msg` exists; the lease ends only
>   through holder `LeaseRelease` (`broker_srv.rs:427`) or **disconnect** auto-release (`:456-464`).
>   `RevokeForLease` revokes **swap from other tenants** to *grant* the lease, not the lease itself. This
>   SPEC redefines RNF-5 around the real mechanism (holder-cooperative + forced disconnect as a last admin
>   resort) — DT-19.
> - **C2** (provision / RF-5): a `Free→Leased` lease **does not free** `DeviceMem` in the daemon (it only
>   changes state in `slices.rs:89-95`); WinDrive performs `cuMemAlloc` **locally**. Without this DT, IMPL
>   silently double-claims the same GPU. It closes co-residency: logical broker budget + physical Windows
>   `cuMemGetInfo.free` gate + operational free floor — DT-20.
> - **H1** (R8): “shrink” was vague. It closes the observable revocation sequence with an active pagefile
>   (no magical shrink API) — DT-19.
> - **H2** (ITEM-8): biasing paged pool toward pagefile-VRAM via “minimum C:” is a heuristic, not a
>   guarantee. The gate requires pagefile-VRAM `% Usage` > 0 **before** the service is killed; otherwise
>   the drill ABORTs (not a silent “pass”) — DT-21.
> - **H3** (ITEM-3): the WinDrive heartbeat was “minimal” without a shape. It is closed as default
>   `Msg::Psi` (P2 DT-13 pattern); CUDA `Lib` Drop becomes `loader::close` (not unconditional `dlclose`).
> - **H4** (ITEM-5): the WDK build surface was absent. It lists `.vcxproj`/INF/package under
>   Files to CREATE.
>
> **GO re-audit — LOWs incorporated (no new decision):**
> - **L1:** DT-2 aligns with DT-22 (events = auxiliary; primary wake = COMMIT_AND_FETCH).
> - **L2:** `RAMSHARED_DISK_PARAMS` is explicit in `protocol.h` (not only on the IOCTL line).
> - **L3:** the DT-4 buffer path under SRBEX uses StorPort helpers (`StorPortGetSystemAddress`), not
>   classic-only `Srb->DataBuffer`.
> - **L4:** the RF-5/RNF-5 matrix points to DT-19/DT-20.
>
> **SSDV3 STEP 2** (after 2.5), from `PRD.md` (GO) + Step 0 drill (PASS-A + contained user-page B1).
> **Platform adaptation (DT-14):** Windows-kernel checklist (WDK/SDV/Driver Verifier/InfVerif),
> not `checkpatch.pl`/`make modules`.

## Closed implementation scope

**In now (RF-1..RF-8, RNF-1..RNF-8 from the PRD, in one SPEC):**

- **RF-4** — port the CUDA layer to `nvcuda.dll` (Windows), reusing the **same symbol table** from
  `ramshared-cuda` (ITEM-1).
- **RF-3** — Windows userspace service (`ramshared-winsvc`) that backs block I/O with VRAM, reusing the
  promoted `VramBackend` adapter (ITEM-2, ITEM-3, ITEM-6).
- **RF-5** — service becomes a tenant of the existing broker (`LeaseRequest`/`LeaseRelease`), with new
  `TransportKind::WinDrive` (ITEM-3).
- **RF-2** — definitive Day-0 driver↔service protocol: an SPSC ring pair (SQ/CQ) in service memory,
  locked+mapped by the driver, plus a data-area bounce buffer and doorbell IOCTL (ITEM-4 ABI, ITEM-5
  driver, ITEM-6 service).
- **RF-1** — from-scratch StorPort **virtual miniport** driver: virtual disk and secure control device
  (ITEM-4, ITEM-5).
- **RF-6** — secondary-pagefile activation through `NtCreatePagingFile` plus post-update smoke (ITEM-7).
- **RF-7** — ordered teardown plus service-crash containment (ITEM-5 driver behavior, ITEM-8 drill).
- **RF-8** — attestation-signed installer (ITEM-11; signing mechanics; EV/Partner Center onboarding is
  organizational, R9, outside code).
- **RNF-1** (N=72h, DT-12), **RNF-2** (K at first measurement, DT-13), **RNF-3** (Day-0), **RNF-4**
  (kernel-boundary validation), **RNF-5** (revocable lease with active pagefile), **RNF-6** (VM-only
  pressure/fuzz/drill), **RNF-7** (attestation loads), **RNF-8** (zero Linux regression).

**Out now (Day-0, no dual path):**

- **Primary**/boot-time pagefile (structural impossibility, PRD §2/§12).
- Distribution through **Windows Update** and full WHCP/HLK (Plan B is recorded, not in this MVP — PRD §12,
  §14 #2a).
- **Non-NVIDIA** GPUs (Vulkan/D3D12 → P3 track; the `VramProvider` trait keeps the door open, but no
  Vulkan-Windows backend enters here).
- `nvcuda.dll` v2 interposer; RAM↔VRAM tiering within the service (MVP = VRAM-only, like the Linux tier);
  compression/dedup; custom auth/cryptography (private network only, like P1/P2).
- **Multi-lease** (broker is one-lease-at-a-time, `crates/ramshared-wsl2d/src/broker_srv.rs:403`).
- **New lease force-revoke `Msg`** (C1/DT-19 — reuse disconnect/holder release).
- **Freeing daemon `DeviceMem` in GrantLease** (C2/DT-20 — logical budget + local allocation).
- SRB-buffer zero-copy (bounce buffer is the Day-0 choice — DT-4; zero-copy is a future
  measurement-gated optimization, not a dual path).

**Assumed-ready dependencies (confirmed in the codebase, verified in this pass):**

- `trait VramProvider` (`crates/ramshared-vram/src/lib.rs:61`, `alloc`+`mem_info`) and `trait VramMemory`
  (`:41`, `zero`/`read_at`/`write_at`), without `unsafe`, hardware agnostic.
- `ramshared-cuda`: `Cuda::load()` (`driver.rs:79`), `Syms` (`ffi.rs:47`) with `_v2` symbols
  (`cuInit`, `cuDeviceGetCount`, `cuDeviceGet`, `cuDeviceGetName`, `cuCtxCreate_v2`, `cuCtxDestroy_v2`,
  `cuCtxSynchronize`, `cuMemAlloc_v2`, `cuMemFree_v2`, `cuMemcpyHtoD_v2`, `cuMemcpyDtoH_v2`,
  `cuMemsetD8_v2`, `cuMemGetInfo_v2`, optional `cuGetErrorString`), RAII in reverse order.
- `ramshared-block`: `trait BlockBackend` (`request.rs:16`, methods `size_bytes`/`block_size`/
  `read_at`/`write_at`/`flush`), `serve()` (`request.rs:55`, validation→`NBD_EINVAL` before the
  backend), `pub struct IoError(pub String)` (`:13` — **struct**, not enum).
- `VramBackend<M>` (`crates/ramshared-wsl2d/src/backend.rs:11-55`): a `VramMemory`→`BlockBackend`
  adapter, **generic and decoupled from `ublk`** at lines 11-55 (`use crate::ublk` at `:8` serves
  `SliceView`/`RamBackend`/tests below). It is the promotion target (DT-6).
- `ramshared-broker`: `enum Msg` (`protocol.rs:19`) with `LeaseRequest{bytes}` (`:42`),
  `LeaseRelease{lease}` (`:45`), `LeaseGranted{lease,bytes}` (`:64`), `LeaseDenied{reason}` (`:68`),
  `Register{proto,tenant,transport}` (`:21`); **no** force-revoke-to-holder Msg (C1);
  `write_msg`/`read_msg` (`:132`/`:144`, **monomorphic in `Msg`**, `MAX_LINE_BYTES=64KiB` ceiling);
  `PROTO_VERSION=1` (`:12`); `enum TransportKind` (`model.rs:48` = `NbdUnix`|`NbdTcp` today).
- `BrokerCore` / `endpoint_for` / `on_tick` / lease: **`crates/ramshared-wsl2d/src/broker_srv.rs`**
  (not in crate `ramshared-broker` — P2 lesson). `endpoint_for` L182-195; `on_tick` L573;
  1-lease L403; capacity L412; grant L628-664; disconnect auto-release L456-464.
- `SliceMap::lease/unlease` (`crates/ramshared-broker/src/slices.rs:89,99`) only changes state — it
  **does not** free physical VRAM (C2/DT-20).
- Step 0 empirical precedent (VM drill 2026-07-03, `PASSO0-DRILL-RUNBOOK.md`): PASS-A + contained B1
  3× for a **user page**; **kernel page not refuted** (what ITEM-8 closes). Method finding:
  **incompressible data** (`RandomNumberGenerator`) is required to force real paging (Win11 Memory
  Compression masks compressible data).
- P2 pattern precedent (`docs/specs/no-milestone/memory-broker/SPEC.md`): `windows-service`+`windows-sys`
  under `[target.'cfg(windows)']`, bin with real `main` + `not(windows)` stub (green Linux workspace),
  new `TransportKind` breaks exhaustive `match` in `endpoint_for` and requires filtering in `on_tick`.

## PRD → SPEC traceability matrix

| PRD | SPEC implementation |
| --- | --- |
| RF-1 (StorPort virtual miniport) | ITEM-4 (ABI), ITEM-5 (driver) — DT-1, DT-17, DT-18 |
| RF-2 (driver↔service protocol) | ITEM-4 (ABI/`protocol.h`+mirror), ITEM-5 (rings/doorbell/inflight in driver), ITEM-6 (`driver_link` in service) — DT-2, DT-3, DT-4, DT-17, DT-18 |
| RF-3 (Rust userspace service) | ITEM-2 (promoted `VramBackend`), ITEM-3 (skeleton+broker), ITEM-6 (I/O ↔ VRAM loop) — DT-6, DT-15, DT-16 |
| RF-4 (CUDA port → `nvcuda.dll`) | ITEM-1 (cross-platform `ramshared-cuda`) — DT-5 |
| RF-5 (broker tenant) | ITEM-3 (`broker_tenant` + `TransportKind::WinDrive` + `on_tick` + `endpoint_for`) — DT-7, DT-19, DT-20 |
| RF-6 (secondary pagefile + smoke) | ITEM-7 (`ntpagefile` + `smoke`) — DT-8 |
| RF-7 (teardown + crash containment) | ITEM-5 (deterministic driver containment, DT-10), ITEM-8 (drill + ordered teardown, DT-9, DT-11) |
| RF-8 (attestation-signed installer) | ITEM-11 — organizational R9 outside code |
| RNF-1 (zero BSOD, N hours) | ITEM-10 (Driver Verifier soak) — DT-12, DT-14 |
| RNF-2 (numbers, not adjectives; K ceiling) | ITEM-9 (`Measure-PagefileVram.ps1`) — DT-13 |
| RNF-3 (Day-0) | all ITEMs; no shim/dual path (DT-4/DT-5/DT-15 justified) |
| RNF-4 (kernel-boundary validation) | ITEM-5 (IOCTL + untrusted MDL validation) — DT-14, DT-17, DT-18 |
| RNF-5 (revocable lease with pagefile) | ITEM-3, ITEM-7/8 (`Invoke-RevokeDrill`, R8) — DT-19 (holder-cooperative; no revoke Msg) |
| RNF-6 (non-disruptive, VM-only) | ITEM-8, ITEM-10 (pressure/fuzz/drill only in VM) |
| RNF-7 (attestation loads) | ITEM-11 (verification on 26200.8655, test-signing OFF) |
| RNF-8 (zero Linux regression) | ITEM-1, ITEM-2 (only shared crates touched) — gate = green Linux drills/tests |

## Technical decisions

Decisions closed here that the PRD left as “Inference: to be fixed in the SPEC”.

| # | Decision | Rationale |
| --- | --- | --- |
| DT-1 | **RF-1 = StorPort *virtual* miniport** through `VIRTUAL_HW_INITIALIZATION_DATA` (`StorPortInitialize`), **plus a separate control device** created with `IoCreateDeviceSecure` (SDDL restricted to SYSTEM+Administrators) and exposed by device-interface GUID. The disk is enumerated by the miniport; the service channel is the control device (not the SCSI path). | Exact pattern proven by WinSpd (real StorPort virtual miniport + control device — PRD §2/§3). A separate control device provides a distinct, securable IOCTL surface (RNF-4), without mixing it with the storage path. |
| DT-2 | **RF-2 = an SPSC ring pair (SQ driver→service, CQ service→driver)** in **service** memory, locked and mapped by the driver (`MmProbeAndLockPages` + `MmGetSystemAddressForMdlSafe`), plus a **data-area bounce buffer** (fixed `queue_depth × max_io_bytes` slots), plus `IOCTL_RAMSHARED_COMMIT_AND_FETCH` doorbell (pending IRP). Auto-reset events in REGISTER are **auxiliary signaling** (`KeSetEvent`); the **service primary wake is the pending IRP** (DT-22) — not a dual wait path. `ublk` model adapted to Windows IOCTL/MDL. | Rejects: **NBD-over-loopback**; **ImDisk proxy**; SRB-buffer **zero-copy** (DT-4). SPSC ring + doorbell = “one mode: disk delegated to userspace” (PRD §3). |
| DT-3 | **One VRAM I/O thread in the service** (SQ single-consumer, CQ single-producer). | CUDA-context thread affinity is thread-local (`ramshared-cuda` `driver.rs:176-181`; `VramMemory` documentation `lib.rs:38-40`); the Linux daemon already runs all VRAM I/O in one thread. Reusing the invariant avoids `cuCtxSetCurrent` and races. |
| DT-4 | **Bounce buffer** (driver copies SRB buffer ↔ data-area slot: WRITE before posting the SQE, READ after an OK CQE), **not zero-copy**. Under SRBEX (DT-23), the buffer pointer comes from **`StorPortGetSystemAddress` / StorPort helpers** — do not assume classic `Srb->DataBuffer` is the only path. | Extra memcpy is negligible versus PCIe in µs (RNF-2/R6). Zero-copy is a future measurement-gated optimization (ITEM-9), not a Day-0 dual path. |
| DT-5 | **RF-4 = make `ramshared-cuda` cross-platform**, not a new crate: extract the loader boundary (`dlopen`/`dlsym`/`dlclose` vs `LoadLibraryW`/`GetProcAddress`/`FreeLibrary`) to `loader_unix.rs`/`loader_win.rs` selected by `#[cfg]`; `Syms` (`ffi.rs:47`) and `driver.rs` (safe wrappers) stay **identical** and shared; candidates become `nvcuda.dll` on Windows. **This is not a dual path:** it is **one** symbol table (the `_v2` names are identical in `nvcuda.dll`), with two OS loaders. | RF-4 explicitly requires “the **same** symbol table” (PRD §2/§8). A parallel crate would duplicate `Syms`+`driver.rs` (violates DRY/Day-0). Cost: touches validated CUDA crate → RNF-8 (gate = green CUDA tests + Linux GPU roundtrip; #14). |
| DT-6 | **Promote generic `VramBackend<M>` adapter to `ramshared-block`** (crate gains `ramshared-vram` dependency); `ramshared-wsl2d` becomes `pub use ramshared_block::VramBackend` (deletes local definition, behavior preserved). Both OSs reuse the **same** tested adapter. | Hard rule #1 (reuse) + immutability/DRY: Windows service needs `VramMemory→BlockBackend`; duplicating 45 lines would diverge Linux/Windows. `ramshared-block` is the natural home (“where VRAM becomes a block device”). Lines 11-55 do not use `ublk` (verified). Gate: green `qemu-ublk-*` drills (RNF-8, #14). |
| DT-7 | **RF-5 = new `TransportKind::WinDrive`** (additive in `crates/ramshared-broker/src/model.rs:48`, currently only `NbdUnix`/`NbdTcp` — **`DccAgent` does not yet exist in code**). Adding the variant **breaks exhaustive `match`** in `endpoint_for` (`crates/ramshared-wsl2d/src/broker_srv.rs:182-195`) → mandatory `WinDrive => None` arm; the tenant is **excluded from swap round-robin/rebalance** by transport filtering in `on_tick` (`:573-584`) when constructing `present` from `TenantState.transport` (`:74`). If P2 `DccAgent` lands later, the filter generalizes to “lease-only transports”. **No `arbiter.rs` diff** (`TenantView` has no transport — L50). | Reuse of P2 pattern (C1/C2/DT-5 from memory-broker SPEC), verified in current code. `WinDrive` only leases; it never receives `SwapOn`. |
| DT-8 | **`NtCreatePagingFile`** isolated in `ntpagefile.rs`: **DT-24** allowlist (`26200.*`), graceful failure, minimum pagefile on `C:`. Removal: `NtSetSystemInformation` removes it; if the OS does not release it hot → **reboot** (the real “shrink” — H1/DT-19). | Undocumented API (R5); empty allowlist was M3 gap. |
| DT-9 | **Teardown NEVER removes the disk with an active pagefile** (the exact B1 BSOD vector). Required order (RF-7a): disable pagefile → (reboot if the OS does not release it hot) → drain in-flight I/O → destroy virtual disk → `VramBackend::zero()` (wipe — reuse Linux DT-17) → `LeaseRelease`. | The `PASSO0-DRILL-RUNBOOK.md` drill showed that pulling the disk with an active pagefile is the dangerous case; safe teardown is its opposite. Wipe before return because the pagefile contained process memory (PRD flow 5). |
| DT-10 | **Crash containment (RF-7b) = deterministic driver behavior.** When the service dies (control-device handle close → `IRP_MJ_CLEANUP`/`CLOSE`), the driver **completes ALL in-flight SRBs with `SRB_STATUS_ERROR`/`STATUS_DEVICE_NOT_CONNECTED`** — it never leaves an SRB pending (which would stall the storage stack) and never completes partial success. This is analogous to contained Linux SIGBUS and makes **B2 (driver-mediated error)** testable at last (the disk does NOT disappear; I/O fails cleanly). | This is the R7 mitigation **lever**: the driver can **fail** paging I/O instead of making the disk disappear — the hypothesis (PRD flow 4) that mediated error is more recoverable than a “pulled disk”. Proven/refuted in ITEM-8. |
| DT-11 | **Kernel-page drill** through **VM-only** test driver `ramshared-poolstress.sys`: GB-scale `ExAllocatePool2(POOL_FLAG_PAGED,...)` + `BCryptGenRandom` + touch + IOCTL read-back; minimum C: pagefile (heuristic); **DT-21 residency gate** before kill; B1 vs B2 (DT-10). | Closes Step 0 gap (user-page only). H2: placement in pagefile-VRAM is not guaranteed — hence DT-21. |
| DT-12 | **RNF-1: N = aggregate 72 h** (3× independent 24 h runs, in the spirit of ≥3 `benchmarks.md` rounds) with **Driver Verifier Standard** active + I/O-path and IOCTL fuzzing, **zero BugCheck**. | Reference-class anchor (#4/#8): HLK/WHQL stress durations (24-72 h). 3×24 h provides variation between runs rather than one sample. Number fixed; counterfactual: any BugCheck aborts promotion. |
| DT-13 | **RNF-2: K “fixed at the first real measurement,” NOT invented now.** `Measure-PagefileVram.ps1` measures side by side (pagefile-VRAM vs disk pagefile) **in the same window**, ≥3 runs, p50/p99+deviation, `idle`/`loaded` tags, dual `results.jsonl`+`BENCHMARKS.md` output. Gate = **(a)** capacity relief (pagefile-VRAM usage > 0 under pressure) **and (b)** page-in p99 ≤ **K×** disk, with **K defined by the first measurement** (not “faster than disk” — VRAM loses to NVMe, given Linux). | PRD RNF-2/§13.3 corrected by the 2.5 audit: the value is **capacity**, not speed. Inventing K would be anchoring (#4). The SPEC closes **how to measure**, not the number. |
| DT-14 | **Windows-kernel validation checklist replaces Linux** (recorded, not silent — task requirement): WDK/EWDK build through MSBuild with `TreatWarningsAsErrors`+`/W4 /WX`; clean **Static Driver Verifier** (`msbuild /p:RunCodeAnalysis=true` + SDV) report (or documented waivers); runtime **Driver Verifier** during RNF-1; universal-INF `InfVerif.exe /w`; `ApiValidator`; `signtool` + attestation submission (Partner Center); VM integration harness through **PowerShell Direct** (kselftest equivalent, RNF-6). Rust userspace retains `cargo fmt/clippy/test/audit/deny`. | There is no `checkpatch.pl`/`make modules` here. The checklist structure/rigor is preserved; the tools are real Windows-driver tools. |
| DT-15 | Service-owned **`WinDriveConfig`** now (self-contained, `[win_drive]` section); when P2 `ramshared-config` lands, it absorbs this section. It is not a shim: it is this feature's **own** config. | P2 (`ramshared-config`) is SPEC, not IMPL — do not assume it is ready. A local definition keeps Day-0 and avoids speculative dual paths. |
| DT-16 | **Cross-compile gating (P2 DT-12 pattern):** `ramshared-winsvc` + Windows dependencies (`windows`, `windows-service`, `windows-sys`, `ntapi`) under `[target.'cfg(windows)'.dependencies]`; Windows FFI modules `#[cfg(windows)]`; the bin has real `#[cfg(windows)] fn main` **and** stub `#[cfg(not(windows))] fn main` (`eprintln!`+`exit(2)`). | Keeps `cargo test --workspace` green on Linux host (C driver does not enter cargo; service compiles as stub). |
| DT-17 | **`protocol.h` (C) is the ONLY ABI source of truth** (`RAMSHARED_*` structs, IOCTL codes, `RAMSHARED_ABI_VERSION`). Rust side is a `#[repr(C)]` mirror with `const { assert!(size_of::<Sqe>()==32) }` (etc.) plus a golden-bytes cross-check test. Like a Linux kernel uapi header. | uAPI/ABI (SSDV3 category 4): layout exposed between Ring-0 and Ring-3 is irreversible after release; C↔Rust drift becomes silent corruption. |
| DT-18 | **The driver treats mapped memory (rings/data area) and every index/tag as UNTRUSTED** (defense in depth): CQ head/tail bounds-checked each iteration; every CQE tag validated against the inflight table (reject unknown/duplicate tag → never complete an SRB twice, which would be UAF/BugCheck). | Service is Ring-3; a buggy/compromised service must not induce OOB or double-complete in Ring-0 (RNF-4, #13 illusion of validity — validate the real failure mode, not the happy path). |
| DT-19 | **RNF-5 / R8 = holder-cooperative revocation + disconnect** (C1). Protocol **unchanged** apart from `TransportKind::WinDrive` (no new `Msg`). (a) **Normal:** service completes DT-9 before `LeaseRelease`. (b) **Admin / revocation test:** `Invoke-RevokeDrill.ps1` tells the **service** (SCM stop / named-pipe admin / CLI) to start (a) — it does **not** fabricate a nonexistent broker frame. (c) **Last resort:** close TCP session (broker `CloseSession` or socket kill) triggers broker auto-release; service treats `read_msg` EOF as “lease lost on paper” and, **if pagefile is still active**, enters emergency DT-9 (may require reboot). Abort: active pagefile + dead socket without DT-9 = residual B1 vector (documented in DEGRADATION-MATRIX). | Real code: a lease disappears only through `LeaseRelease` or disconnect. Inventing `LeaseRevoke` would be a broker uAPI change (outside this feature's Day-0 scope, and P1 deliberately did not mediate holder usage). |
| DT-20 | **VRAM co-residency (C2): lease is a logical budget; allocation is physical and local.** (1) Broker: `LeaseRequest` reserves `Free→Leased` slices (`slices.rs:89-95`) — it **does not** `cuMemFree` daemon `DeviceMem`; Linux-pool VRAM remains allocated. (2) WinDrive: after `LeaseGranted{bytes}`, measure `cuMemGetInfo` **in the Windows process** and only then `alloc(min(granted.bytes, free))`; if `free < config.size_bytes` → **fail closed** (log + immediate `LeaseRelease` + no disk creation). (3) Operating with WSL2 daemon on the same GPU: operator sizes **daemon free floor ≥ WinDrive size_bytes** (or stops the pool before Windows provision). Forbidden formula: assume the lease “transfers” Linux-pool bytes to Windows. (4) Test gate: with daemon holding pool > GPU−size, Windows provision **must** fail gracefully (test `coresidence_fail_closed`). | Same physical GPU (RTX 2060). Silent double-claim is the most expensive IMPL bug; closing it in the SPEC prevents host thrash/OOM. Aligned with P2 model (lease = permission/budget; CUDA usage is local). |
| DT-21 | **ITEM-8 — pagefile-VRAM residency evidence is a gate, not hope (H2).** Before killing the service in the kernel-page drill: (i) **incompressible** data in paged pool (`BCryptGenRandom`); (ii) `\Paging File(<volume-vram>)\% Usage` counter **> 0** (or `Win32_PageFileUsage.CurrentUsage` for VRAM volume > 0); (iii) if pagefile-VRAM usage == 0 after pressure, the drill **ABORTS AS INCONCLUSIVE** (does not count as PASS and does not count as BSOD) — minimum C: is a heuristic; the OS may keep kernel pages in C:. Only then: kill service / B1 vs B2, ≥3 executions with confirmed residency. | Step 0 already showed that Memory Compression + opaque placement mask the test. Without (ii), ITEM-8 would be theater (#13). |
| DT-22 | **Single Day-0 wake path (partial H3 / M1):** service **only** waits for work through pending `DeviceIoControl(IOCTL_RAMSHARED_COMMIT_AND_FETCH)` (one loop). `sq_event`/`cq_event` handles in REGISTER are **auxiliary driver signaling** (`KeSetEvent` on submit / optional on CQE) for future waiters; service MVP does **not** use `WaitForSingleObject` on them as the primary path. SPSC barriers: writer stores entries with release semantics **before** advancing `tail` (driver: `KeMemoryBarrier`/`MemoryBarrier`; Rust service: `Ordering::Release` on tail mirror if atomics are used; explicit barrier with `volatile` C). Reader loads `tail` with acquire-equivalent semantics before reading entries. | Dual wake path = disguised Day-0 dual path. One testable path. |
| DT-23 | **SRB surface (M2):** miniport declares **`STORAGE_REQUEST_BLOCK` (SRBEX)** support through `VIRTUAL_HW_INITIALIZATION_DATA` / modern StorPort feature bits; handlers accept SRBEX and read buffers through StorPort APIs (`StorPortGetSystemAddress`, etc.). Classic `SCSI_REQUEST_BLOCK` fallback **only** if SDV/harness on build 26200 requires it — recorded as a waiver, not a second product. | Win11 25H2 + current WDK; historical WinSpd uses classic paths, but Day-0 targets the current stack. |
| DT-24 | **`NtCreatePagingFile` allowlist (M3):** MVP-supported builds = **Windows 11 25H2 `26200.*`** (the drill and host build). `RtlGetVersion` outside 26200 series → `PagefileError::UnsupportedBuild`; disk remains usable without pagefile (RF-6 smoke). Expand the list only with VM drill evidence on the new build. | Prevents an empty allowlist (IMPL interpretation) and scope creep. |
| DT-25 | **Day-0 install = INF + signed `.cat` + root device (`Root\RamShared`).** Lab: `Inf2Cat` + test-sign `.cat`; `certutil -addstore Root/TrustedPublisher`; `pnputil /add-driver` + **`devcon install inf Root\RamShared`** (guest pnputil without `/add-device`). `sc create` is forbidden as product path (conflicts with PnP → status 1072). After `StorPortInitialize`, **hook dispatch** only on the control device and **forward** StorPort IRPs (DT-25). LUN 1 bus/target/LUN; CREATE → `BusChangeDetected`. **MDL data ≤ 4 MiB**. COMMIT with cancel routine. **R/W: `StorPortGetSystemAddress` only** (`MapBuffers=NON_READ_WRITE`); parse CDB LBA (10/16). | Evidence 2026-07-09: sc-only → 0 disk; INF+devcon → `Get-Disk N=1 RAMSHARE VRAMDISK 64MiB`; format with raw DataBuffer → BSOD **0xD1**. |

## Atomicity boundary and rollback policy

**Atomicity boundary of this implementation:**

- **Atomic:** (1) **one block I/O** (SQE→VRAM→CQE→SRB completion) completes **exactly once**, either OK
  **or** error, never partial success (`serve()`/`BlockBackend` already guarantees this in the reused
  layer; the driver guarantees exactly-once through the inflight table + DT-18). (2) The **REGISTER
  handshake** is all-or-nothing: either the entire queue is validated+locked+mapped, or
  `IOCTL_RAMSHARED_REGISTER_QUEUE` fails and **nothing** remains locked (reverse-order unwind, `goto
  out_err` idiom). (3) **Lease** reuses broker one-lease-at-a-time serialization
  (`crates/ramshared-wsl2d/src/broker_srv.rs:403`; `LeaseGranted` only after drained slices, `:628-664`).
  **Holder force-revoke DOES NOT exist in the protocol** (C1/DT-19) — see revocation boundary below.
- **Outside atomicity (eventual / multi-step; partial states accepted and documented):**
  - **Pagefile activation** (`NtCreatePagingFile`) is a multi-step OS operation, **not** transactional:
    accepted partial state = “disk active, pagefile not yet active” → feature degrades, not breaks (DT-8).
  - **Teardown** is a sequence (DT-9); accepted partial state = “pagefile disabled awaiting reboot, disk
    still present” — never “disk removed with active pagefile”.
  - **Lease revocation with active pagefile (R8/RNF-5 / DT-19):** **holder-cooperative only** in the
    current protocol. Real paths: (1) **service starts** `LeaseRelease` after ordered pagefile teardown
    (DT-9); (2) TCP-session **disconnect** → broker auto-`on_lease_release` (`broker_srv.rs:456-464`) —
    service MUST complete DT-9 *before* closing the socket, or an admin accepts documented residual risk.
    There is **no** `Msg::LeaseRevoke` nor “broker signals revoke” (C1). Observable sequence (H1):
    `pagefile off` (`ntpagefile::remove_secondary` / `NtSetSystemInformation` removes) → if OS does not
    release it hot, **reboot** (only real shrink; do not invent an “shrink under load” API) → drain I/O →
    destroy disk → `zero()` → `LeaseRelease`. Worst case = slow revocation (minutes if reboot), **never
    silent**.
  - **Capacity prediction** (VRAM budget vs pressure) is a snapshot → conservative margin.

**Rollback policy:**

- **Application rollback:** uninstall (remove driver through INF + stop/remove service). Pagefile config
  returns to `C:`-only. Each Rust ITEM compiles in isolation; `git revert` of an ITEM reverts its surface
  (reverting ITEM-1/ITEM-2 requires revalidating Linux drills — therefore gate #14).
- **Migration rollback:** **N/A** — no migratable persistent schema/state exists (VRAM is volatile by
  design; pagefile contents are transient).
- **Data rollback:** **N/A** — Day-0, no live production, no durable data (`zero()` wipe on teardown is
  hygiene, not migration).
- **Forbidden / `forward-only`:** removing/destroying the virtual disk with an active pagefile is
  **forbidden in every environment** (B1 BSOD vector, DT-9) — explicit `forward-only` operational
  restriction: once the pagefile is active, the only safe path is to disable it first (reboot if needed).
  Corresponding abort trigger is in ITEM-8.

## Kahneman map by critical step

| Step / ITEM | Kahneman discipline | Link | Required question | Minimum evidence | Abort trigger |
| --- | --- | --- | --- | --- | --- |
| ITEM-1 (RF-4 cross-platform loader) | #14 Mass-Refactoring + #1 WYSIATI | [`#14`](../../../methodology/kahneman-disciplines.md#disc-14) · [`#1`](../../../methodology/kahneman-disciplines.md#disc-1) | Does `nvcuda.dll` export the **same** `_v2` symbols from `ffi.rs`? Does the refactor change the Linux path? | Windows: `Cuda::load()` resolves all 13 symbols + `mem_info()` returns plausible `free/total` on RTX 2060. Linux: green `cargo test -p ramshared-cuda` + `gpu_roundtrip_256mib` (`--ignored`). | Any missing `_v2` symbol in `nvcuda.dll`, **or** any Linux test/roundtrip regression. |
| ITEM-2 (promote `VramBackend`) | #14 Mass-Refactoring | [`#14`](../../../methodology/kahneman-disciplines.md#disc-14) | Does promotion change Linux daemon behavior? | Green `qemu-ublk-daemon.sh` + `qemu-ublk-crash-e1b.sh` drills (SIGBUS 5/5); no `cargo test -p ramshared-wsl2d` regression. | Any daemon drill/test regression → revert promotion. |
| ITEM-4 (RF-2 ABI `protocol.h`+mirror) | #9 Question substitution | [`#9`](../../../methodology/kahneman-disciplines.md#disc-9) | “Is the protocol correct?” → does C layout match Rust mirror byte-for-byte? | Size `const { assert!(...) }` compiles on both sides; golden-bytes test (fixed bytes ↔ struct) passes; C `sizeof` == Rust `size_of` in CI. | Size/offset drift between `protocol.h` and Rust mirror. |
| ITEM-5 (driver: IOCTL surface + rings) | #13 Illusion of validity + #5 Availability | [`#13`](../../../methodology/kahneman-disciplines.md#disc-13) · [`#5`](../../../methodology/kahneman-disciplines.md#disc-5) | Are malformed REGISTER/doorbell inputs (short buffer, non-power-of-two `queue_depth`, null VA, unaligned offset, unknown/duplicate tag) **rejected before** `MmProbeAndLockPages`/touching VRAM/completing an SRB? | Clean SDV report; test under Driver Verifier: every malformed input → IOCTL fails with `STATUS_INVALID_PARAMETER`, **zero BugCheck**; paired “legitimate input still works” test. | Any BugCheck from malformed input; SDV defect without waiver; observable SRB double-complete. |
| ITEM-6 + ITEM-8 (crash with active pagefile — R7 vector) | #5 Availability + #2 Counterfactual | [`#5`](../../../methodology/kahneman-disciplines.md#disc-5) · [`#2`](../../../methodology/kahneman-disciplines.md#disc-2) | Kill service with a **kernel page** (paged pool, incompressible data) **confirmed in pagefile-VRAM** → contained **or** `KERNEL_DATA_INPAGE_ERROR` 0x7a? B2 (DT-10) vs B1? | `Invoke-KernelPageDrill.ps1`: (DT-21) pagefile-VRAM `% Usage` > 0 **before** kill; otherwise INCONCLUSIVE. ≥3 executions with residency; B1 vs B2; capture BSOD/`MEMORY.DMP`. | **B2 produces BugCheck 0x7a without a specified mitigation** → PRD §14 #2b abort. Drill without confirmed residency does **not** count as PASS. |
| ITEM-7 (`NtCreatePagingFile`, undocumented) | #1 WYSIATI + #2 Counterfactual | [`#1`](../../../methodology/kahneman-disciplines.md#disc-1) · [`#2`](../../../methodology/kahneman-disciplines.md#disc-2) | Does Windows **activate** a secondary pagefile on the volume of **our** miniport (untested — WYSIATI PRD §14 #1)? Does an out-of-allowlist build degrade gracefully? | `Win32_PageFileUsage` shows active `<vram>:\pagefile.sys` after `NtCreatePagingFile`; fallback test (unsupported build → no pagefile, disk remains formattable/usable). | Activation causes BugCheck/corruption, **or** no graceful-failure path exists (disk breaks together with pagefile). |
| ITEM-9 (RNF-2 numeric gate) | #3 Number, not adjective + #11 Halo | [`#3`](../../../methodology/kahneman-disciplines.md#disc-3) · [`#11`](../../../methodology/kahneman-disciplines.md#disc-11) | Does pagefile-VRAM **relieve capacity** (usage > 0 under pressure) without becoming **catastrophically** slower than disk? | `results.jsonl`+`BENCHMARKS.md`: side-by-side p50/p99, same window, ≥3 runs, `idle`/`loaded` tags; pagefile-VRAM usage counter > 0. | Capacity relief == 0 (never used under pressure) **or** p99 > K× disk (K from first measurement) → do not promote (PRD §14 #2c). |
| ITEM-10 (RNF-1 soak) | #5 Availability + #6 Calibrated confidence | [`#5`](../../../methodology/kahneman-disciplines.md#disc-5) · [`#6`](../../../methodology/kahneman-disciplines.md#disc-6) | 72 h (3×24 h) under Driver Verifier + fuzz with no BugCheck? | Driver Verifier logs + soak harness; 3 runs recorded with `run-id`. | Any BugCheck in any run. |
| ITEM-11 (RF-8 attestation) | #2 Counterfactual | [`#2`](../../../methodology/kahneman-disciplines.md#disc-2) | Does the attestation-signed driver **load** on a stable build with test-signing OFF? | Load on Windows 11 25H2 **26200.8655**, test-signing OFF, driver trusted by default (RNF-7). | Does not load on stable build (policy tightened) **and** WHCP cost is not justified → abort/park (PRD §14 #2a). |
| RNF-5 (revocation with active pagefile, R8) | #5 Availability + #2 Counterfactual | [`#5`](../../../methodology/kahneman-disciplines.md#disc-5) · [`#2`](../../../methodology/kahneman-disciplines.md#disc-2) | Does the service complete DT-9 before `LeaseRelease`, with no active pagefile on disconnect? | `Invoke-RevokeDrill.ps1`: SCM stop/admin → pagefile off (or reboot path) → destroy → wipe → `LeaseRelease` observed in broker log; worst-case duration measured. There is **no** revoke broker frame (C1/DT-19). | Pagefile still active after “release”; teardown deadlock; broker still shows lease after clean disconnect. |

## Security checklist (pre-implementation)

- [ ] **Isolation (RNF-4/DT-1):** control device created with `IoCreateDeviceSecure` + SDDL
  `D:P(A;;GA;;;SY)(A;;GA;;;BA)` (SYSTEM + Administrators only); service runs as LocalSystem. Nobody
  without privilege opens the device.
- [ ] **Buffer overflow / OOB (RNF-4/DT-18):** every `METHOD_BUFFERED` IOCTL validates
  `Parameters.DeviceIoControl.InputBufferLength == sizeof(expected struct)` **before** reading
  `SystemBuffer`; REGISTER validates `abi_version`, power-of-two `queue_depth` (≤ `RAMSHARED_MAX_QD`),
  `block_size ∈ {512,4096}`, bounded `max_io_bytes`, non-null VAs, and consistent lengths **before**
  `MmProbeAndLockPages`; every SQE validates offset/len (aligned to `block_size`, in range) before
  touching VRAM (mirrors `ramshared_block::validate`).
- [ ] **Untrusted mapped memory (DT-18):** CQ head/tail bounds-checked every iteration; CQE tag validated
  against the inflight table (reject unknown/duplicate → no SRB double-complete).
- [ ] **Preemption / IRQL:** bounce copies and MDL locking occur outside `DISPATCH_LEVEL` when required;
  SRB completion follows StorPort IRQL rules; no pageable allocation in hot I/O path (analogous to
  `GFP_ATOMIC`).
- [ ] **Input validation (service):** lease `bytes` revalidated in service before forwarding to broker;
  broker already rejects `> total` (`broker_srv.rs:412`).
- [ ] **`unsafe`/FFI (Rust):** CUDA-Windows (ITEM-1), `driver_link` (ITEM-6), `ntpagefile` (ITEM-7) each
  use `// SAFETY:` per block; safe surface without `unsafe` (`ramshared-cuda` pattern).
- [ ] **Secrets/pointers:** no hardcoded credential; **no kernel address logged** (WPP/ETW without
  pointers — aligned with `coding.md`: never leak KASLR); telemetry without PII (pagefile content is
  process memory — **never** log payload).
- [ ] **Kernel Oops/internal error:** failing IOCTL returns generic NTSTATUS; no implementation detail or
  internal offset leaks to Ring-3.

## Files to CREATE

### `drivers/windows/ramshared/protocol.h`  *(ITEM-4 — RF-1/RF-2, DT-17)*

- **Purpose:** single source of truth for the driver↔service ABI (Windows uAPI).
- **Requirements covered:** RF-2, DT-17, DT-18.
- **Structs/types (fixed `#pragma pack(push,8)` layout; all `UINTxx`):**
  ```c
  #define RAMSHARED_ABI_VERSION 1u
  #define RAMSHARED_MAX_QD      256u        /* maximum queue_depth (power of 2) */
  #define RAMSHARED_MAX_IO      (1u<<20)    /* 1 MiB per bounce slot */

  enum ramshared_op { RAMSHARED_OP_READ=0, RAMSHARED_OP_WRITE=1, RAMSHARED_OP_FLUSH=2 };
  /* status: 0=OK; otherwise errno-like and aligned with ramshared-block */
  #define RAMSHARED_ST_OK     0
  #define RAMSHARED_ST_EIO    5
  #define RAMSHARED_ST_EINVAL 22

  typedef struct _RAMSHARED_SQE {   /* driver -> service, 32 bytes */
      UINT64 tag; UINT32 op; UINT32 flags;
      UINT64 offset; UINT32 len; UINT32 buf_slot;
  } RAMSHARED_SQE;

  typedef struct _RAMSHARED_CQE {   /* service -> driver, 16 bytes */
      UINT64 tag; INT32 status; UINT32 reserved;
  } RAMSHARED_CQE;

  typedef struct _RAMSHARED_RING_HDR { /* precedes entries[]; SPSC */
      UINT32 magic; UINT32 entries;      /* entries = queue_depth (power of 2) */
      volatile UINT32 head; volatile UINT32 tail;
  } RAMSHARED_RING_HDR;

  typedef struct _RAMSHARED_REGISTER { /* REGISTER IOCTL payload */
      UINT32 abi_version; UINT32 disk_id; UINT32 queue_depth; UINT32 block_size;
      UINT32 max_io_bytes; UINT32 reserved;
      UINT64 sq_ring_va; UINT64 cq_ring_va;
      UINT64 data_area_va; UINT64 data_area_len;
      UINT64 sq_event_handle; UINT64 cq_event_handle; /* auxiliary (DT-22); primary wake = IRP */
  } RAMSHARED_REGISTER;

  typedef struct _RAMSHARED_DISK_PARAMS { /* IOCTL CREATE_DISK */
      UINT64 size_bytes;   /* multiple of block_size */
      UINT32 block_size;   /* 512 or 4096 */
      UINT32 reserved;
      UCHAR  serial[16];   /* INQUIRY VPD / stable identification */
  } RAMSHARED_DISK_PARAMS;
  ```
- **IOCTL codes:** `CTL_CODE(FILE_DEVICE_MASS_STORAGE, 0x800|N, METHOD_BUFFERED, FILE_READ_ACCESS|FILE_WRITE_ACCESS)`
  for `IOCTL_RAMSHARED_REGISTER_QUEUE` (N=0), `IOCTL_RAMSHARED_UNREGISTER_QUEUE` (N=1),
  `IOCTL_RAMSHARED_COMMIT_AND_FETCH` (N=2), `IOCTL_RAMSHARED_CREATE_DISK` (N=3, `RAMSHARED_DISK_PARAMS{size_bytes,block_size,serial[]}`),
  `IOCTL_RAMSHARED_DESTROY_DISK` (N=4).
- **Reference pattern:** Linux-kernel uapi headers (stable struct size); WinSpd `winspd.h`.
- **Required tests:** C compilation emits `C_ASSERT(sizeof(RAMSHARED_SQE)==32)`, etc.
- **Kahneman discipline:** #9 (see Map — ITEM-4).

### `drivers/windows/ramshared/protocol_check.rs` *(Rust mirror; lives in `crates/ramshared-winsvc/src/proto.rs`)*  *(ITEM-4 — RF-2, DT-17)*

- **Purpose:** exact `#[repr(C)]` mirror of `protocol.h` + size assertions + golden bytes.
- **Structs:** `#[repr(C)] pub struct Sqe { pub tag:u64, pub op:u32, pub flags:u32, pub offset:u64, pub len:u32, pub buf_slot:u32 }` (same for `Cqe`, `RingHdr`, `Register`); `pub const ABI_VERSION:u32=1; pub const MAX_QD:u32=256; pub const MAX_IO:u32=1<<20;`.
- **Functions:** `const _: () = { assert!(core::mem::size_of::<Sqe>()==32); assert!(core::mem::size_of::<Cqe>()==16); /* ... */ };`
- **Required tests:** `golden_sqe_bytes` (serializes a known `Sqe` and compares it with the fixed byte array produced by C).

### `drivers/windows/ramshared/driver.c` + `driver.h`  *(ITEM-5 — RF-1, DT-1)*

- **Purpose:** `DriverEntry`; registers the **StorPort virtual miniport** and creates the control device.
- **Requirements covered:** RF-1, DT-1.
- **Functions (exact WDK signatures):**
  - `NTSTATUS DriverEntry(PDRIVER_OBJECT, PUNICODE_STRING)` — constructs `VIRTUAL_HW_INITIALIZATION_DATA`
    (callbacks below) → `StorPortInitialize`; creates control device (DT-1) through `IoCreateDeviceSecure`
    (SYSTEM+Admin SDDL) + `IoRegisterDeviceInterface` (own GUID).
  - `ULONG HwStorFindAdapter(PVOID DevExt, ..., PPORT_CONFIGURATION_INFORMATION)` — 1 bus/target/lun
    virtual; no real port I/O.
  - `BOOLEAN HwStorInitialize(PVOID DevExt)`; `BOOLEAN HwStorResetBus(PVOID,ULONG)`.
  - `BOOLEAN HwStorStartIo(PVOID DevExt, PSCSI_REQUEST_BLOCK Srb)` — in practice receives SRBEX
    (DT-23); SCSI dispatch → `virtdisk.c`.
- **Dependencies:** `storport.lib`, `ntstrsafe.lib`. **Pattern:** WinSpd (virtual miniport + control device).
- **Tests:** SDV/InfVerif in ITEM-5; disk enumeration in VM harness.

### `drivers/windows/ramshared/virtdisk.c` + `virtdisk.h`  *(ITEM-5 — RF-1)*

- **Purpose:** virtual-disk state plus SCSI-command translation.
- **Structs:** `typedef struct _VIRTUAL_DISK { UINT64 size_bytes; UINT32 block_size; UCHAR serial[16]; RAMSHARED_QUEUE queue; volatile LONG state; } VIRTUAL_DISK;`
- **Functions:** `NTSTATUS VdCreate(PVIRTUAL_DISK,const RAMSHARED_DISK_PARAMS*)`; `VOID VdTranslateSrb(PVIRTUAL_DISK,PSCSI_REQUEST_BLOCK)` — handles `SCSIOP_READ/WRITE(10|16)`, `SYNCHRONIZE_CACHE`(→FLUSH), `INQUIRY`, `READ_CAPACITY(16)`, `TEST_UNIT_READY`; READ/WRITE/FLUSH become SQEs through `queue.c`.
- **Tests:** NTFS formatting in VM harness (ITEM-5).

### `drivers/windows/ramshared/queue.c` + `queue.h`  *(ITEM-5 — RF-2, DT-2, DT-10, DT-18)*

- **Purpose:** SPSC rings, inflight table, doorbell, MDL lock/map, crash containment.
- **Structs:** `typedef struct _RAMSHARED_QUEUE { PMDL sq_mdl,cq_mdl,data_mdl; PRAMSHARED_RING_HDR sq,cq; PUCHAR data; PKEVENT sq_event,cq_event; RAMSHARED_INFLIGHT inflight[RAMSHARED_MAX_QD]; KSPIN_LOCK lock; PIRP pended_fetch; } RAMSHARED_QUEUE;` (inflight keeps the `PSCSI_REQUEST_BLOCK` + `op` + `buf_slot` by tag).
- **Functions:**
  - `NTSTATUS QRegister(PRAMSHARED_QUEUE,const RAMSHARED_REGISTER*,KPROCESSOR_MODE)` — **validates everything**
    (DT-18) → `MmProbeAndLockPages`(sq/cq/data) → `MmGetSystemAddressForMdlSafe` → `ObReferenceObjectByHandle`
    for the 2 events. Failure → reverse-order unwind (nothing locked, REGISTER atomicity).
  - `NTSTATUS QSubmit(PRAMSHARED_QUEUE,PSCSI_REQUEST_BLOCK,enum ramshared_op,UINT64 off,UINT32 len)` —
    allocates tag+slot; if WRITE, copies SRB buffer (through StorPort helper/DT-23/DT-4) → slot; publishes SQE
    (**before** advancing `tail`, DT-22); auxiliary `KeSetEvent(sq_event)`; if `pended_fetch` exists,
    completes it (service primary wake).
  - `NTSTATUS QCommitAndFetch(PRAMSHARED_QUEUE,PIRP)` — CQ drain (validates tag/head/tail, DT-18): for
    each CQE, if READ+OK copy slot → SRB buffer (StorPort helper), map status→`SRB_STATUS_*`,
    `StorPortNotification(RequestComplete)`; if SQ is empty, **pend** the IRP (`pended_fetch`), otherwise
    complete with the new-SQE count.
  - `VOID QTeardownOnCrash(PRAMSHARED_QUEUE)` (DT-10) — in `IRP_MJ_CLEANUP`/`CLOSE`: **completes ALL
    in-flight SRBs with `SRB_STATUS_ERROR`** (deterministic, never pending); `MmUnlockPages`;
    `ObDereferenceObject` for events.
- **Kahneman discipline:** #13+#5 (ITEM-5) and #5+#2 (ITEM-6/8) in the Map.
- **Tests:** IOCTL fuzz under Driver Verifier (ITEM-5); crash drill (ITEM-8).

### `drivers/windows/ramshared/control.c` + `control.h`  *(ITEM-5 — RF-1/RF-2, RNF-4, DT-1)*

- **Purpose:** control-device IOCTL dispatch plus security.
- **Functions:** `NTSTATUS CtlDeviceControl(PDEVICE_OBJECT,PIRP)` — `switch(ioctl)` over the 5 codes;
  validates `InputBufferLength`/`OutputBufferLength` before using `SystemBuffer` (RNF-4); COMMIT_AND_FETCH
  may return `STATUS_PENDING`. `IRP_MJ_CLEANUP` → `QTeardownOnCrash`.
- **Tests:** malformed inputs → `STATUS_INVALID_PARAMETER`, zero BugCheck (ITEM-5, #13).

### `drivers/windows/ramshared/ramshared.inf`  *(ITEM-5/ITEM-11 — RF-1/RF-8)*

- **Purpose:** **universal** INF (attestation-signable), installs the miniport + control-device interface.
- **Tests:** clean `InfVerif.exe /w ramshared.inf` (DT-14).

### `drivers/windows/ramshared/ramshared.vcxproj` (+ `.vcxproj.filters`, `ramshared.sln`)  *(ITEM-5 — H4, DT-14)*

- **Purpose:** Day-0 WDK/EWDK build surface (do not make the implementer invent the project).
- **Props:** `ConfigurationType=Driver`, `DriverType=WDM`/`MiniPort` according to the StorPort template in
  WDK, `Platform=x64`, `TreatWarningAsError=true`, `/W4 /WX`, link `storport.lib` + `ntstrsafe.lib`.
- **Targets:** `Build` (Release), `Sdv` (`RunCodeAnalysis` + Static Driver Verifier), INF package.
- **Tests:** clean EWDK build; SDV report attachable to IMPL.

### `drivers/windows/ramshared/package/` (`ramshared.inf` already listed, optional WPP `ramshared.man`)  *(ITEM-5/11)*

- **Purpose:** attestation packaging layout (`signtool` + Partner Center).

### `crates/ramshared-winsvc/` (`Cargo.toml`, `src/main.rs`, `src/service.rs`, `src/driver_link.rs`, `src/ntpagefile.rs`, `src/broker_tenant.rs`, `src/smoke.rs`, `src/config.rs`, `src/proto.rs`)  *(ITEM-3/ITEM-6/ITEM-7 — RF-3/RF-5/RF-6, DT-15, DT-16)*

- **Purpose:** Windows service (LocalSystem) that backs VRAM I/O, arbitrates lease, and activates the pagefile.
- **Requirements covered:** RF-3, RF-5, RF-6, DT-15, DT-16.
- **Structs/types:**
  - `config.rs`: `#[derive(Deserialize)] struct WinDriveConfig { size_bytes:u64, block_size:u32, pagefile_min:u64, pagefile_max:u64, priority:i32, broker:SocketAddr, tenant:String }` (`[win_drive]` section, DT-15).
  - `driver_link.rs`: `struct DriverLink { ctl: HANDLE, q: QueueMap }`; `QueueMap` owns rings+data area (service memory) and the 2 events; method `run_io_loop<B: BlockBackend>(&mut self, backend:&mut B)` (single thread, DT-3) — blocking `DeviceIoControl(COMMIT_AND_FETCH)` → for each new SQE: `match op { READ=>backend.read_at(off, slot); WRITE=>backend.write_at(off, slot); FLUSH=>backend.flush() }` → post CQE (status mapped from `IoError`) → repeat. Isolated `unsafe` FFI (`// SAFETY:`).
  - `ntpagefile.rs` (DT-8): `fn create_secondary(volume:&Path, min:u64, max:u64) -> Result<(),PagefileError>` (`NtCreatePagingFile`); `fn remove_secondary(volume:&Path)`; `supported_build() -> bool` guard through `RtlGetVersion` (allowlist); graceful failure.
  - `broker_tenant.rs` (RF-5, DT-7, DT-19, DT-20): reuses `ramshared_broker::{Msg, write_msg, read_msg}` (monomorphic in `Msg`); `Register{proto:PROTO_VERSION, tenant, transport:TransportKind::WinDrive}`; `acquire(bytes)->LeaseRequest`; `release(lease)->LeaseRelease`; handles `LeaseGranted/Denied`. **Heartbeat (H3):** `Msg::Psi { sample: PsiSample::default(), swaps: vec![], mem: None }` at configurable interval (default 5s) — TCP keepalive + presence; PSI is ignored in arbitration because `on_tick` excludes WinDrive (DT-7). **EOF/`Error`/close:** active pagefile → emergency DT-9 (DT-19c). **After Granted:** `cuMemGetInfo` gate (DT-20) before `alloc`.
  - `smoke.rs` (RF-6/flow 6): `fn post_boot_smoke() -> SmokeResult` — checks enumerated disk + active pagefile (`Win32_PageFileUsage`); regression (such as ImDisk #38) → gracefully disables feature + logs.
  - `service.rs`: `fn provision()` (flow 1: config → `LeaseRequest` → `LeaseGranted` → **`mem_info` free≥size** (DT-20) → CUDA `alloc` → `IOCTL_CREATE_DISK` → REGISTER → NTFS volume → `NtCreatePagingFile` 26200 allowlist (DT-24)); fails closed at any step with `LeaseRelease` if grant already occurred. `fn teardown()` = DT-9. `fn on_revoke_request()` (admin/SCM) = DT-19a.
  - `main.rs`: `#[cfg(windows)] fn main()` (SCM through `windows-service`) + `#[cfg(not(windows))] fn main(){ eprintln!("ramshared-winsvc: Windows-only"); std::process::exit(2); }` (DT-16).
- **Internal dependencies:** `ramshared-cuda` (RF-4), `ramshared-vram`, `ramshared-block` (`BlockBackend`+`VramBackend`), `ramshared-broker`.
- **External dependencies (only `[target.'cfg(windows)']`):** `windows`/`windows-sys` (IOCTL, `MmXxx` through handles, `Win32_PageFileUsage`), `windows-service` (SCM), `ntapi` or own FFI for `NtCreatePagingFile`/`RtlGetVersion`, `serde`+`toml`.
- **Reference pattern:** `ramshared-agent` (broker client) + `ramshared-wsl2d/main.rs` (single-thread VRAM I/O loop, `run_nbd`); memory-broker SPEC P2 (cross-compile gating).
- **Required tests:** `driver_link` roundtrip against a **fake driver** (in-memory `DeviceIoControl` mock) — SQE READ/WRITE/FLUSH → RAM backend → CQE; `broker_tenant` `LeaseRequest`→`Granted` against fake broker; `ntpagefile` fallback (unsupported build → graceful `Err`); `config` parse. (Pure, run on Linux; bin is stub — DT-16.)
- **Kahneman discipline:** ITEM-6/ITEM-7 in the Map.

### `drivers/windows/tools/poolstress/` (`poolstress.c`, `poolstress.inf`)  *(ITEM-8 — RF-7, DT-11; VM-only)*

- **Purpose:** test driver that **forces a kernel page** (incompressible paged pool) into pagefile-VRAM
  and permits command-driven page-in. **Never** distributed (test-signing in VM only, RNF-6).
- **Functions:** `DriverEntry` creates control device; IOCTL `ALLOC(n_gb)` → `ExAllocatePool2(POOL_FLAG_PAGED,...)` + incompressible `BCryptGenRandom` + touch; IOCTL `READBACK` → reads all data (forces page-in); IOCTL `TRIM_WS` → forces working-set trim (`ZwSetSystemInformation`/pressure).
- **Tests:** it is the drill instrument itself (ITEM-8).

### `scripts/windows/` (`Invoke-DriverSoak.ps1`, `Invoke-KernelPageDrill.ps1`, `Measure-PagefileVram.ps1`, `Invoke-RevokeDrill.ps1`, `Build-Sign-Install.ps1`)  *(ITEM-8/9/10/11 — RNF-1/RNF-2/RNF-5/RNF-6/RF-8, DT-11/DT-12/DT-13)*

- **Purpose:** VM integration/measurement harness through **PowerShell Direct** (`PASSO0-DRILL-RUNBOOK.md`
  pattern).
- **Functions:** `Invoke-KernelPageDrill.ps1` (loads `poolstress`, active pagefile-VRAM, minimum C:,
  incompressible pressure, kills service, captures BSOD/`MEMORY.DMP`, ≥3 executions);
  `Measure-PagefileVram.ps1` (side by side vs disk, ≥3 runs, automatic context,
  `results.jsonl`+`BENCHMARKS.md`, DT-13); `Invoke-DriverSoak.ps1` (Driver Verifier Standard, 3×24 h,
  DT-12); `Invoke-RevokeDrill.ps1` (RNF-5/R8/**DT-19**: stops service through SCM/admin → DT-9 → checks
  `LeaseRelease` at broker; **does not** send an invented Msg).
- **Tests:** produce evidence for ITEMs 8/9/10/11 and the Map RNF-5 row.

## Files to MODIFY

### `crates/ramshared-cuda/src/ffi.rs` + `src/driver.rs` (+ new `src/loader_unix.rs`, `src/loader_win.rs`)  *(ITEM-1 — RF-4, DT-5) — RNF-8*

- **What changes:** extract the loader boundary. Today `ffi.rs:13-19` declares `dlopen/dlsym/dlclose/dlerror`
  with unconditional `#[link(name="dl")]` (does not compile on Windows). Afterwards: `loader_unix.rs`
  (`#[cfg(unix)]`, dlopen) e `loader_win.rs` (`#[cfg(windows)]`, `LoadLibraryW`+`GetProcAddress`+`FreeLibrary`);
  `Cuda::load()` (`driver.rs:79`) calls `loader::open`/`loader::sym`/`loader::close`.
- **Requirements covered:** RF-4, DT-5.
- **Affected function/block:** `ffi` (Unix-only extern block), `CANDIDATES` (`driver.rs:69-75`),
  `Cuda::load`, **`Lib` Drop** (`driver.rs:52-61` — currently always calls `ffi::dlclose`; becomes
  `loader::close`, otherwise Windows breaks in Drop — H3).
- **Before:** direct `dlopen`/`dlsym`; Linux/WSL2 candidates.
- **After:** OS-specific loader; Windows `CANDIDATES=["nvcuda.dll"]`. `Syms` (`ffi.rs:47-62`, **13
  mandatory + 1 optional** `cuGetErrorString`) and `driver.rs` wrappers are loader agnostic.
- **Why:** RF-4 requires the SAME symbol table in `nvcuda.dll` (PRD §2/§8); one crate avoids duplicating
  `Syms`+`driver.rs` (Day-0/DRY).
- **Impact:** does **not** break userspace ABI; Linux behavior **does not** change. `ramshared-vulkan`/`wsl2d`
  are untouched. **RNF-8** = gate.
- **Required tests:** Linux: green `cargo test -p ramshared-cuda` + `gpu_roundtrip_256mib --ignored`
  (no regression). Windows: `Cuda::load()` resolves the 13 symbols in `nvcuda.dll`; plausible `mem_info()`.
- **Kahneman discipline:** #14 + #1 (ITEM-1 Map).

### `crates/ramshared-cuda/Cargo.toml`  *(ITEM-1 — RF-4, DT-16)*

- **What changes:** Windows loader dependencies under `[target.'cfg(windows)'.dependencies]` (`windows-sys` for
  `LoadLibraryW`/`GetProcAddress`); Linux retains `#[link(name="dl")]`/libc. **Impact:** none on Linux.

### `crates/ramshared-block/src/lib.rs` + new `src/vram_backend.rs`  *(ITEM-2 — RF-3, DT-6) — RNF-8*

- **What changes:** create `vram_backend.rs` with promoted `VramBackend<M>` (move lines 11-55 from
  `wsl2d/backend.rs` verbatim, which **do not** use `ublk`); `lib.rs` `pub use vram_backend::VramBackend`.
- **Requirements covered:** RF-3, DT-6.
- **Before:** `ramshared-block` does not know VRAM; `VramBackend` lives in `wsl2d`.
- **After:** `ramshared-block` depends on `ramshared-vram`; exposes `VramBackend<M: VramMemory>`.
- **Why:** Windows service (`x86_64-pc-windows-msvc`) **does not** compile `wsl2d` (Linux-only); it needs
  the adapter from a shared library — reuse, not duplication.
- **Impact:** `ramshared-block/Cargo.toml` gains `ramshared-vram`; no API break (additive).
- **Required tests:** `backend.rs` tests that exercise `VramBackend` migrate with it; green `cargo test -p ramshared-block`.
- **Kahneman discipline:** #14 (ITEM-2 Map).

### `crates/ramshared-wsl2d/src/backend.rs`  *(ITEM-2 — RF-3, DT-6) — RNF-8*

- **What changes:** delete local `VramBackend` definition (lines 10-55) and add `pub use ramshared_block::VramBackend;`.
  `SliceView`/`RamBackend`/`use crate::ublk` **remain**.
- **Why:** behavior is preserved; Linux daemon uses the same shared type.
- **Impact:** `main.rs` (`run_nbd`) and `VramBackend` callers unchanged (same name/signature).
- **Required tests:** green `qemu-ublk-daemon.sh` + `qemu-ublk-crash-e1b.sh` drills (RNF-8, #14).

### `crates/ramshared-broker/src/model.rs`  *(ITEM-3 — RF-5, DT-7)*

- **What changes:** `enum TransportKind` gains `WinDrive` (additive on serde wire). **Impact:** additive,
  **but breaks exhaustive `match`** in `endpoint_for` → must come with the change below.

### `crates/ramshared-wsl2d/src/broker_srv.rs`  *(ITEM-3 — RF-5, DT-7)*

- **What changes:** (a) `endpoint_for` gains `TransportKind::WinDrive => None` arm (WinDrive has no NBD
  endpoint; keeps exhaustive `match` compiling); (b) `on_tick` **excludes** `transport == WinDrive`
  tenants when constructing `present` (swap round-robin/rebalance) — if P2 `DccAgent` already exists,
  filter becomes “lease-only transports”. **Why:** `WinDrive` is lease-only (DT-7).
- **Required tests:** `BrokerCore`: `windrive_nao_recebe_swap` (1 WinDrive + 1 swap tenant → only swap
  receives `SwapOn`); `windrive_pode_lease` (WinDrive lease revokes swap); **no `arbiter.rs` diff**.

### `Cargo.toml` (workspace) / `crates/ramshared-block/Cargo.toml`  *(ITEM-2/ITEM-3, DT-16)*

- **What changes:** workspace `members += "crates/ramshared-winsvc"`. `ramshared-block` depends on `ramshared-vram`.
  `ramshared-winsvc` inherits `publish=false`; Windows dependencies under `[target.'cfg(windows)']` (DT-16).

## Files to DELETE

| File | Reason |
| --- | --- |
| — | None. The local `VramBackend` definition in `wsl2d/backend.rs` is **replaced** by re-export (ITEM-2); it is not a file to delete. Additive Day-0. |

## Observability

**Metrics / counters (service — ETW or perf counters):**

- `ramshared_win_io_ops_total` (Counter, labels `op=read|write|flush`) — in `run_io_loop`.
- `ramshared_win_bytes_served_total` (Counter) — per OK CQE.
- `ramshared_win_inflight_depth` (Gauge) — inflight depth.
- `ramshared_win_vram_bytes{kind=free|used|total}` (Gauge) — from `mem_info()`.
- `ramshared_win_pagefile_vram_usage_bytes` (Gauge) — from VRAM-volume `Win32_PageFileUsage` (the
  RNF-2/DT-13 capacity-relief gate).
- `ramshared_win_lease_events_total` (Counter, `event=acquire|granted|denied|release|revoke`).

**Driver (WPP tracing, without kernel addresses):** disk enumeration, REGISTER/UNREGISTER, SQE/CQE count,
error injection in `QTeardownOnCrash`, malformed-IOCTL rejections.

**Structured logs (service):**

| Event | Level | Fields |
| --- | --- | --- |
| Pagefile activated/deactivated | Info | `volume`, `min`, `max`, `priority`, `build` |
| Lease acquire/granted/denied/release/revoke | Info | `tenant`, `bytes`, `lease` |
| Post-update smoke: regression | Warn | `check`, `detalhe`, `degrade=true` |
| Driver reported in-flight error (contained crash) | Error | `inflight_falhos`, `op` |
| Ordered teardown (phase) | Info | `fase` (`pagefile_off`/`drain`/`destroy`/`wipe`/`release`) |

**Benchmarks (RNF-2):** `docs/benchmarks/results.jsonl` (1 line/run) + human `docs/BENCHMARKS.md`,
append-only, automatic context (`benchmarks.md`).

## Contracts and living documentation

| Document | Required update | Reason |
| --- | --- | --- |
| `docs/specs/no-milestone/windows-swap-driver/IMPL.md` | Create (per ITEM) | SSDV3 traceability (after Step 2.5 GO); preflight in `PREFLIGHT.md` |
| `Documentation/` (uAPI/ABI) → `drivers/windows/ramshared/protocol.h` | Create | new Ring-0↔Ring-3 ABI (DT-17) |
| `docs/decisions/ADR-0006-storport-virtual-miniport.md` | Create | from-scratch StorPort decision + RF-2 protocol (SPSC ring) — architectural (anti-halo #11) |
| `docs/memory-broker/PRD.md` §10/§12 | Change | mark the “Windows swap driver” (P4/Track 2) as detailed here; remove it from global out-of-scope |
| `docs/memory-broker/VISION.md` (L28) | Change | the “out of scope for now” line points to this feature |
| `docs/reliability/DEGRADATION-MATRIX.md` | Change | new modes: backend crash with active pagefile (mediated B2), Windows update (ImDisk #38), lease revocation with pagefile, `NtCreatePagingFile` guard-fail |
| `docs/LIBRARIES.md` | Change | WDK/StorPort; `windows`/`windows-sys`/`windows-service`/`ntapi`; `nvcuda.dll` loader |
| `deny.toml` | Change | `windows*`/`ntapi`/`toml` licenses (MIT/Apache-2.0 — current allowlist) |
| `CLAUDE.md` | Change | new `drivers/windows/` tree (structural pattern) |
| `.claude/rules/*.md` | N/A | no new convention (`kernel.md` already covers “map/unmap explicitly” — applies to MDL) |
| `docs/methodology/kahneman-disciplines.md` | N/A | no new discipline/anchor |
| `README.md`/`ARCHITECTURE.md` | Change | new component (Track 2); `MEMORY.md` entry per ITEM |
| `docs/INDEX.md` | Change | feature status becomes `SPEC` |

## Implementation order

Numbered list, no gaps; **userspace before kernel** (PRD §10); each ITEM cites its RF/RNF/DT in commits
(SSDV3 hard rule #4); `IMPL.md` per ITEM. **Phase 0 (Step 0 drill) is already executed** with a caveat
(kernel page remains for ITEM-8).

1. **ITEM-1 — RF-4:** cross-platform `ramshared-cuda` (loader split, DT-5). Userspace-only testable on
   real host (allocates/writes/reads VRAM through `nvcuda.dll`); validates VRAM pillar and RNF-8. *(PRD §10.1)*
2. **ITEM-2 — RF-3 (base):** promote `VramBackend<M>` to `ramshared-block` (DT-6); gate = Linux drills.
3. **ITEM-3 — RF-3/RF-5:** `ramshared-winsvc` skeleton + `broker_tenant` + `TransportKind::WinDrive`
   (`model.rs`+`endpoint_for`+`on_tick`); e2e lease against existing broker, local VRAM, **no driver**.
   *(PRD §10.2)*
4. **ITEM-4 — RF-1/RF-2 (ABI):** `protocol.h` + Rust `proto.rs` mirror + size assertions + golden bytes
   (DT-17). **Contract frozen before driver** (template: structs/headers first).
5. **ITEM-5 — RF-1/RF-2 (driver MVP):** StorPort virtual miniport (`driver.c`/`virtdisk.c`) + secure
   control device (`control.c`, RNF-4) + rings/doorbell/inflight/MDL (`queue.c`, DT-2/DT-18) + deterministic
   containment (`QTeardownOnCrash`, DT-10). In VM (test-signing): disk enumerates → formats NTFS → clean
   SDV/InfVerif → IOCTL fuzz under Driver Verifier. *(PRD §10.3)*
6. **ITEM-6 — RF-3 (complete):** `driver_link.rs` (RF-2 service side) connected to `VramBackend`; e2e
   read/write/flush ↔ real VRAM in VM; Driver Verifier + I/O-path fuzz.
7. **ITEM-7 — RF-6:** `ntpagefile.rs` + secondary-pagefile activation (DT-8) + `smoke.rs` (flow 6). *(PRD §10.4 part)*
8. **ITEM-8 — RF-7 (the R7 gate):** `poolstress.sys` + `Invoke-KernelPageDrill.ps1` (DT-11) + ordered
   teardown (DT-9) + B1 vs B2 comparison. **Feeds `DEGRADATION-MATRIX` before any real host.**
   *(PRD §10.4)*
9. **ITEM-9 — RNF-2:** `Measure-PagefileVram.ps1` side by side vs disk pagefile (DT-13), VM then host. *(PRD §10.5)*
10. **ITEM-10 — RNF-1:** `Invoke-DriverSoak.ps1` (Driver Verifier, 72 h/3×24 h, DT-12), zero BugCheck.
11. **ITEM-11 — RF-8/RNF-7:** `Build-Sign-Install.ps1` (attestation + Partner Center submission); load on
    real host (test-signing OFF, 26200.8655), first supervised use (RNF-6). *(PRD §10.6)*

## Test plan

**Backend / pure (run here, Linux — Windows bin is stub, DT-16):**

- `ramshared-cuda`: no Linux regression (`cargo test -p ramshared-cuda`); `#[ignore]` `gpu_roundtrip_256mib`.
- `ramshared-block`: migrated `VramBackend` (write→read roundtrip; OOB→error).
- `ramshared-winsvc`: `driver_link` roundtrip against fake `DeviceIoControl` (READ/WRITE/FLUSH → RAM → CQE);
  `broker_tenant` LeaseRequest→Granted (fake broker); **`coresidence_fail_closed`** (DT-20: free < size →
  LeaseRelease + no CREATE_DISK); `ntpagefile` unsupported-build fallback; `config` parse.

- `ramshared-broker`/`wsl2d`: `BrokerCore` `windrive_nao_recebe_swap` + `windrive_pode_lease`;
  **no `arbiter.rs` diff**; `qemu-ublk-*` + `qemu-broker-drill.sh` drills (RNF-8).

**Windows driver (VM, test-signing — RNF-6):**

- **State/hooks:** disk enumeration; clean INF/SDV/InfVerif/ApiValidator.
- **Block flows:** NTFS formatting; READ/WRITE/FLUSH e2e ↔ VRAM; correct `READ_CAPACITY`/`INQUIRY`.
- **Ring-0↔Ring-3 isolation (RNF-4/#13):** malformed REGISTER/doorbell rejected (`STATUS_INVALID_PARAMETER`,
  zero BugCheck) **paired** with “legitimate input still works”; unknown/duplicate tag does not
  double-complete an SRB (DT-18).
- **Concurrency/atomicity:** full queue (`queue_depth`); flush drains; crash containment (DT-10) completes
  all in-flight SRBs with error; storage stack does not stall.
- **Worst case (ITEM-8, #5/#2):** `Invoke-KernelPageDrill.ps1` — incompressible **kernel** page in
  pagefile-VRAM, kills service, B1 vs B2, ≥3 executions; captures BSOD/`MEMORY.DMP`.

**Measurement (RNF-2/#3):** `Measure-PagefileVram.ps1` — side-by-side p50/p99, same window, ≥3 runs,
`idle`/`loaded`, `results.jsonl`+`BENCHMARKS.md`; pagefile-VRAM usage counter > 0.

**Soak (RNF-1):** `Invoke-DriverSoak.ps1` — 3×24 h Driver Verifier + fuzz, zero BugCheck.

**Manual / critical-step evidence:** attestation-signed driver loads (RNF-7); holder-cooperative revocation
with active pagefile (`Invoke-RevokeDrill.ps1`, RNF-5/R8/DT-19); fail-closed co-residency (DT-20);
kernel-page drill with confirmed residency (DT-21).

## Validation checklist

> **DT-14 — Windows-kernel checklist (replaces Linux; recorded, not silent).** Structure/rigor are
> preserved; tools are real Windows-driver tools.

**Driver (kernel mode, C — WDK/EWDK):**

- [ ] Clean Release x64 MSBuild with `TreatWarningsAsErrors=true` + `/W4 /WX` (replaces `make W=1`/`checkpatch.pl`)
- [ ] Clean **Static Driver Verifier** (`msbuild /p:RunCodeAnalysis=true` + SDV) report or documented waivers (replaces `sparse`)
- [ ] **Code Analysis / PREfast for drivers** without unwaived defect
- [ ] Clean `InfVerif.exe /w ramshared.inf` (universal INF); clean `ApiValidator`
- [ ] **Driver Verifier Standard** active during soak (ITEM-10) — zero BugCheck (replaces KASAN/lockdep)
- [ ] PASS VM integration harness through PowerShell Direct (replaces `make kselftest`): enumeration,
  NTFS, e2e I/O, malformed IOCTL rejected, crash containment (RNF-6)
- [ ] `signtool verify` + attestation-signed driver **loads** on 26200.8655, test-signing OFF (RNF-7)

**Service + libraries (Rust userspace):**

- [ ] Clean `cargo fmt --all -- --check`
- [ ] Clean `cargo clippy --workspace --all-targets -- -D warnings` (new crates + bin stub)
- [ ] Green `cargo test --workspace` (new pure tests + existing no regression; Windows bin = Linux stub, DT-16)
- [ ] Green `cargo audit` + `cargo deny check` with `windows*`/`ntapi`/`toml`
- [ ] **RNF-8:** PASS `qemu-ublk-daemon.sh` + `qemu-ublk-crash-e1b.sh` + `qemu-broker-drill.sh` drills; **no `arbiter.rs` diff**
- [ ] `#[ignore]` CUDA `nvcuda.dll` on RTX 2060 (ITEM-1) — plausible `mem_info`

**Docs:**

- [ ] Regenerated `docs/INDEX.md` (status `SPEC`); valid Kahneman-anchor links
- [ ] `DEGRADATION-MATRIX.md`, `LIBRARIES.md`, `ADR-0006`, `IMPL.md` updated in the same structural-slice commit

**Cognitive gates:**

- [ ] Every critical ITEM points to `docs/methodology/kahneman-disciplines.md` (Map) with exact anchor
- [ ] Every critical step records required question, minimum evidence, and abort trigger
- [ ] No vague language at a critical point without observable criterion
- [ ] **R7 gate (ITEM-8):** kernel-page drill has run and `DEGRADATION-MATRIX` is updated
  **before** any load on the real host


> Related focused slice: [windows-storport-cuda-vram](../windows-storport-cuda-vram/SPEC.md) (CUDA storage-only; pagefile/soak/attestation remain open here).
```
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-vulkan --files crates/ramshared-vulkan/src/lib.rs --min 80 --report-json tmp/vulkan-provider-cov.json
```
