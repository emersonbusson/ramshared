# Degradation Matrix — RamShared

> **Rare but costly** failure modes (Kahneman #5 — design for the worst case).
> Discipline #5 anchor: [`kahneman-disciplines.md`](../methodology/kahneman-disciplines.md).
>
> Priority = **probability × impact**. Cosmetic scenarios remain
> "monitored, not designed" (counterexample #5 in the discipline document).
> Every scenario has **detection** (an observable signal) and **unwind**
> (recovery in reverse allocation order — the `goto out_err` idiom).
>
> **Disabled-definition boundary:** The matrix is a failure-model reference, not
> an activation, recovery, storage, swap, VM, or pressure procedure. All
> current candidate managers remain disabled until their named qualification
> gates and separate attended approval exist.

## Matriz

| Scenario | Prob×Impact | Designed degradation behavior | Detection | Unwind / recovery | Status |
| --- | --- | --- | --- | --- | --- |
| **Concurrent managed workloads exhaust guest control-plane progress** | high × critical | Disabled-definition only: one aggregate admission ceiling would reserve `max(4 GiB,25% MemTotal)`; the model keeps the protected control slice serviceable and closes admission before emergency | MemAvailable, PSI full, `memory.high/max`, sample delay, active reservations | Historical model only: GUARDED close → CRITICAL cache target zero/freeze/reclaim → EMERGENCY thaw+TERM, then KILL after 5 s only if unrecovered; no current action | **source-validated; live VM pending** ([control SPEC](../specs/no-milestone/wsl2-control-plane-pressure-incident/SPEC.md)) |
| **Host volume reaches backing-storage capacity** | medium × critical | Classify as `host_volume_exhausted` only with temporally bound host evidence; do not infer RamShared or a specific writer | NTFS Event ID 137 plus `0xC000007F`/`STATUS_DISK_FULL`; free-space telemetry | Stop new pressure/build admission, preserve evidence, restore headroom by an attended storage policy outside the live device lifecycle; hardenings remain independent | **observed trigger; automatic storage cleanup out of scope** |
| **WSL 6.18 DXG FORTIFY warning or custom-distro init timeout during kernel promotion** | observed × critical | Treat bundled reproduction as an upstream confounder, not a waiver; never confirm a custom kernel from `uname` alone and never apply an issue-only patch | exact FORTIFY signature, systemd state, `/dev/dxg`, Xwayland, bounded NVIDIA metadata probe, fresh dmesg counts, same-host bundled query-error baseline | bounded canary disarms the custom kernel and shuts down the failed candidate; preserve logs and keep RamShared/device activation off | **source/static validated; live A/B NO-GO** ([2026-08-23 finding](incidents/2026-08-23-wsl2-dxg-fortify-systemd-no-go.md)) |
| **Heavy process runs outside the managed hierarchy** | medium × high | Do not count it as contained or silently ignore it; overall telemetry reports `UNMANAGED_PRESSURE` with sanitized identity only | top-N `comm`, unit/cgroup, RSS/swap/CPU/I/O; no argv | operator relaunches with `ramshared run/session` or a reversible launcher; no automatic capture of foreign process | **source-validated** |
| **Guest heartbeat expires while guest remains responsive** | medium × critical | Disabled-definition only: guardian model refuses termination when either bounded guest probe or independent WSL/HCS proof succeeds | 15 s heartbeat age plus two 5 s probes and independent host probe | Historical model only: capture status, publish guardian state, continue monitoring; no current terminate/reboot action | **static/manufactured validated; live VM pending** |
| **Guest is independently proven inaccessible** | low × critical | Disabled-definition behavior only: close host telemetry, durably write safe mode, issue at most one targeted `wsl --terminate <SANITIZED_WSL_DISTRO>`, validate a new boot ID; never reboot Windows. This is not a current execution path. | four proof gates, incident-bound termination record, boot ID | safe boot starts control/monitor only; `ramshared recover --resume` needs 60 healthy samples and matching gates | **static/manufactured validated; live VM pending** |
| **Targeted WSL terminate also hangs** | low × critical | Preserve artifacts and publish `BLOCKED`; do not escalate to broad shutdown or Windows reboot | bounded terminate timeout and incomplete incident record | attended operator diagnosis; no automatic host lifecycle action | **designed; live VM pending** |
| **GPU allocation/read/write or WDDM measurement fails** | high × high | Invalidate/release affected clean cache and continue through the SSD origin; unknown measurement sets physical target zero | cache invalidation/fallback counters, explicit measurement error, cache `UNAVAILABLE/RESTRICTED` | origin reads/writes remain authoritative; later healthy samples may repromote within rate/cap | **source-validated; live GPU pending** ([origin SPEC](../specs/no-milestone/wsl2-revocable-vram-origin/SPEC.md)) |
| **SSD origin write/sync/read fails** | medium × critical | Never acknowledge a write missing from origin; return I/O error and force origin `FAILED`/overall non-green | origin error and sticky state; three read+sync recovery probes | retain cache as non-authoritative, block healthy status, repair origin before lifecycle recovery | **source-validated; live device pending** |
| **Origin manifest selects existing WSL swap or non-block object** | low × critical | Refuse before daemon/device mutation; exact canonical PARTUUID must resolve to a block device, not `SANITIZED_EXISTING_WSL_SWAP_DEVICE` | manifest/config hash, PARTUUID, canonical type and size checks | disabled planning only: a separately sealed future origin would be corrected/reattached; the existing 4 GiB swap remains untouched | **source/static validated; provision pending** |
| `dma_map_single`/`dma_alloc` fails (`-ENOMEM`) | medium × high | abort the operation and propagate errno; never expose a partially mapped device | checked return != 0 | `goto out_err`: undo mappings in reverse order, free pages, no leak | **designed** |
| OOM killer fires while a lock is held | low × critical | swap daemon cannot depend on hot-path allocation; `mlockall` + `oom_score_adj=-1000` (SSDV3 §6.2/§11) | `dmesg` OOM; process death | preallocate before exposing the device; no `GFP_KERNEL` in a critical section | **designed** |
| **GPU-PV WDDM evicts live VRAM** (known case) | **high × high** | 4K latency explodes (~1.18 s measured, FASE0-FINAL); **DEMOTE**: `swapoff` only the VRAM tier, pages fall to VHDX, no process kill | latency canary (SSDV3 §9.1, trigger (a) p99 > K×baseline) | VRAM `swapoff` (timeout `T_demote`=30s) → `Demoted`; on expiry → `recover` | **designed** (SSDV3 §9.2) |
| **WDDM budget falls or `/dev/dxg` fails after activation** | high × high | stop new commits before `cuMemAlloc`; never silently move authority to CUDA | invalid/stale `QUERYVIDEOMEMORYINFO` or next chunk > `usable_budget` | keep NBD/chunks alive; lower tier receives new writes; full automatic demotion remains behind ITEM-4 gate | **partial** ([autotier SPEC](../specs/no-milestone/wsl2-native-vram-autotier/SPEC.md)) |
| **More than one dxg adapter without CUDA↔LUID identity** | medium × high | refuse startup; never query one adapter and allocate on another by assumed ordinal | `AmbiguousAdapters(N)` | operator reduces visibility to one adapter; explicit selection only after LUID proof | **designed** |
| VRAM `swapoff` stalls during eviction | medium × high | do not disconnect NBD with swap active (= panic); escalate | processes in `D` > `T_stuck`; `nbd: ... timed out` in `dmesg` | staged `recover` (SSDV3 §13); `wsl --terminate` only as a last resort | **designed** |
| PCIe bus reset / device removal mid-DMA | low × critical | in-flight I/O returns an error; mark `Failed`; never complete I/O as partial success | CUDA error / `dmesg` reset | stop queues, drain/reject pending work, `recover` | **designed** (SSDV3 §8.2) |
| IOMMU fault | low × critical | fail the operation that caused it; do not mask it | `dmesg` IOMMU fault | unwind the offending mapping and record context | **monitored** (outside the WSL2 MVP) |
| CXL link-down / lost coherence | low × critical | treat as device lost | `dmesg`/EDAC | offline the affected node/tier | **monitored** (future hardware) |
| Suspend/resume race (S3/S4) | low × high | re-arm CUDA context on resume or refuse; never assume state persisted | `cuCtx*` failure after resume | reinitialize the tier or enter `Demoted` | **monitored** (not in the manual MVP) |
| **WinDrive B1 — disk/pagefile disappears while pagefile is active** (surprise remove) | medium × critical | processes with pages on pagefile-VRAM may die; residual `KERNEL_DATA_INPAGE_ERROR` 0x7a risk if **kernel** paged-pool resided on the volume | BSOD 0x7a / process death; pagefile-VRAM `% Usage` | **Forbidden:** destroy with pagefile active (DT-9); VM-only through ITEM-8; ordered teardown first | **partial** (2026-07-09: DT-21 PASS; B1 **safe arm PASS**; B1/B2 hot = 0x7A → DT-9) |
| **WinDrive B2 — service dies while disk remains and I/O fails** (DT-10) | medium × high | driver completes in-flight SRBs with an error (`SRB_STATUS_ERROR` / not connected); storage stack does not remain pending | volume I/O errors; miniport ETW/WPP | `QTeardownOnCrash` on CLEANUP/CLOSE; service restart + reprovision | **split** (2026-07-09: pagefile-hot kill → **0x7A**; DT-9 **REFUSE_KILL** hot + **REBOOT_KILL** clean after unload) |
| **WinDrive VRAM co-residency double-claim** (logical lease ≠ physical free) | medium × high | fail closed: no CREATE_DISK if `cuMemGetInfo.free < size` (DT-20); immediate `LeaseRelease` | `coresidence_fail_closed` log; lease denied/released | size the WSL2 daemon free floor or stop the pool before WinDrive | **designed** (unit test in IMPL ITEM-3/6) |
| **WinDrive `NtCreatePagingFile` build outside the allow-list** (DT-24) | low × medium | NTFS disk continues; **no** secondary pagefile (feature degrades, not the host) | `PagefileError::UnsupportedBuild` | expand the allow-list only with a VM drill on the new build | **designed** |
| **WinDrive revocation with pagefile active** (RNF-5 / DT-19) | medium × high | holder-cooperative: pagefile off → (reboot if needed) → destroy → wipe → `LeaseRelease`; do not invent broker messages | residual broker lease; pagefile still listed | `Invoke-RevokeDrill.ps1`; never disconnect with pagefile active without DT-9 | **designed** (harness pending) |
| **Windows Update / ImDisk-style regression** (volume disappears at boot) | low × high | post-boot smoke disables the feature gracefully if disk/pagefile is missing | smoke failure + `degrade=true` log | reinstall package; never force a blind pagefile | **monitored** (smoke ITEM-7) |
| **Windows broker unavailable before exposure** | medium × medium | bounded named-pipe retry only for missing/busy; fail before lease/CUDA/LUN | stable broker error code 3 + retry count | reverse confirmed effects; consumer stops; broker may remain demand-ready | **validated** (VM retry/refusal matrix) |
| **Windows broker lost after Online** | low × high | keep the I/O pump alive; enter FailedSafe and request the existing safe teardown; never reconnect/replay | `broker_lost_online`, pipe EOF, lifecycle evidence | identity/pagefile/lock gates → flush/dismount → unregister/destroy → release if confirmable | **validated** (VM BrokerLossOnline) |
| **Windows broker restart budget exhausted** | low × medium | SCM restarts only abnormal broker crashes after 5/15/40 s; fourth action is NONE | SCM state + new broker instance ID | operator diagnoses config/process; consumer is never auto-replayed | **validated** (VM restart matrix) |
| **Windows package registration fails halfway** | low × high | no candidate start; restore both old SCM definitions and the active pointer | manufactured failure after broker registration | rollback to one complete immutable version; no in-place binary reversal | **validated** (package transaction matrix) |
| **Physical manifest drive-letter collision** | medium × critical | refuse before install/exposure; never remove, remap, reletter, or format an existing volume | drive map/pagefile preflight | build a distinct immutable package using a proven-free letter | **validated** (`R:` refusal; final `S:` campaign) |

## Usage

- Every new critical feature **adds or revises** rows here before merge
  (a measurable Discipline #5 signal: "the matrix was updated for the latest feature").
- A postmortem whose scenario **was not** listed here must add a corrective row
  (see [`postmortems/TEMPLATE.md`](../postmortems/TEMPLATE.md)).

## References

- [`kahneman-disciplines.md`](../methodology/kahneman-disciplines.md) §5
- [`wsl2-cascade-swap/SPEC.md`](../specs/no-milestone/wsl2-cascade-swap/SPEC.md) §8 (erros), §9 (eviction/DEMOTE), §13 (recovery)
- [`wsl2-fase0-final.md`](wsl2-fase0-final.md) — a medida de 1,18 s que fundou a linha de eviction

## CUDA storage-only product (windows-storport-cuda-vram)

| State | Symptom | Operator action | Auto recovery |
| --- | --- | --- | --- |
| CUDA alloc fail | probe/runtime refuses before CREATE | free VRAM / lower size_bytes | none |
| Device loss / stuck cuMemcpy | FailedSafe; health false | supervised reboot if stuck; no force kill | none |
| Pagefile on volume at stop | exit/code 7; Online preserved | clear pagefile; re-stop | none |
| Foreign process IOCTL | ACCESS_DENIED | only owner process | none |
| False backend (lab C#) | not product ImagePath | use Install-RamSharedService.ps1 Rust | none |
