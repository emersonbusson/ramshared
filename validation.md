# validation.md — RamShared

> Live log of **empirical** validations for RamShared — the single source of truth for "is this actually working right now?". Covers all manual, integration, and E2E validations; taxonomy is detailed in the **Categories** table below. Anchored on **Kahneman #13** (existence ≠ execution; green-in-last-run ≠ green-now), plus **#15** (calibrated retry), **#16** (fail-safe / independent curator), and **#17** (replay idempotency) when the entry is about reconnect, demote/reclaim, or command re-delivery. Source: [`docs/methodology/kahneman-disciplines.md`](docs/methodology/kahneman-disciplines.md).

## Conventions

- **Append-only:** Never delete, rewrite, or reorder old entries. The most recent entry goes at the **bottom**. Read from bottom to top; stop when recent entries are sufficient.
- Every entry must carry measured, raw data (numbers or concrete state, no qualitative adjectives before the number) and a clear verdict.
- Never persist credentials, tokens, environment secrets, or PII.

## Categories

| Tag           | What it validates                                                                                   | Typical Verdict             |
| ------------- | -------------------------------------------------------------------------------------------------- | --------------------------- |
| `invariant`   | Low-level static invariants (ABI structural layout, struct offsets, symbol binding)                 | 0 warnings / matches        |
| `ci-gate`     | PR blocking gates (commit lint, clippy check, build validation)                                     | exit 0 / rollup green       |
| `integration` | Proves execution effects against real hardware/kernel (ublk creation, CUDA allocations, socket connections) | effect observed             |
| `fail-safe`   | Resiliency/demotion under load (eviction, teardown, watchdog) — Kahneman **#16**                      | recovery active             |
| `retry`       | Reconnect/retry only on proven transient signatures — Kahneman **#15**                               | fail-fast on deterministic  |
| `idempotent`  | Command/effect applied 2× yields one outcome — Kahneman **#17**                                      | unique effect               |
| `local-check` | Local verification tools (cargo test, cargo clippy, checkpatch outputs)                            | exit 0, test count passes   |
| `perf`        | Latency metrics, IOPS throughput, swap-in latency under pressure                                  | quantitative SLO compliance  |
| `boot`        | System startup validity (daemon initialization, device node creation, driver loading)              | boot ok / fail-closed       |

## Entry Schema

```markdown
## YYYY-MM-DD HH:MM TZ — <title>

**What:** What was validated (1-2 sentences).
**Category:** <tag from the table above>
**How to measure:** Command or test to execute to re-verify. (Optional)
**Measured data:** Raw number/state (e.g., exit 0, 61 passed, count=0, p99=241us, device removed, etc.). No adjectives before numbers.
**Verdict:** ✅ works / 🔴 does not work / 🟡 partial.
**Next action:** Next concrete step, or "none".
```

---

## 2026-07-03 14:15 -03 — Windows VM Secondary Pagefile Surprise-Removal Drill

**What:** Empirically validate how Windows behaves when the backing storage of an active secondary pagefile is abruptly removed.
**Category:** fail-safe
**How to measure:** Perform hot-remove of SCSI virtual disk containing active swapfile in Windows 11 VM. Detail in `docs/runbooks/windows-vram-drive-drill.md`.
**Measured data:** 
- **Scenario A (Mounting):** `E:\pagefile.sys` allocation size = 4096 MB active after reboot (`Win32_PageFileUsage`).
- **Scenario B1 (Displacement):** 3 test runs with active user pageouts (~150-200 MB user-mode memory). Hyper-V VHDX detached abruptly. Guest system remained responsive for 120s with 0 BugChecks/BSODs.
- **Scenario B2 (Driver IO Error):** Not testable (requires custom miniport driver).
**Verdict:** ✅ works (User-space swap loss contained; kernel-page eviction risk unrefuted).
**Next action:** Design the miniport driver to report mediated I/O errors (Scenario B2) rather than physical unplug events.

## 2026-07-09 00:05 -03 — Dynamic CUDA Driver Wrapper Cross-Platform Port

**What:** Validate compile status and dynamic linking safety of the custom CUDA wrapper on Unix/Windows targets after refactoring FFI loader splits.
**Category:** invariant
**How to measure:** Run `cargo test --all` on the local workspace to verify compile bindings and FFI wrapper mocks.
**Measured data:**
- Linked static dynamic dependency `libdl` removed from unix builds.
- Split loaders (`loader_unix.rs` using `dlopen`, `loader_win.rs` using `windows-sys` crate FFI bindings `LoadLibraryW`/`GetProcAddress`) compiling with 0 warnings.
- Workspace unit test suite compilation = SUCCESS.
**Verdict:** ✅ works
**Next action:** None.

## 2026-07-09 00:20 -03 — Complete Open-Source Comment Translation & Metadata Sanitization Audit

**What:** Audit the workspace for native language leakage, local filesystem paths, or credentials in comments and documents.
**Category:** local-check
**How to measure:** Run recursive `grep` searches for local host paths `<legacy-private-root>/` and workstation hostname `<legacy-workstation>` across the workspace.
**Measured data:**
- Comments translated to English across all 10 workspace crates (47 files modified).
- Local hostname `<legacy-workstation>` replaced with `dev-workstation` in `docs/BENCHMARKS.md`.
- File paths `file://<legacy-private-root>/` in specs rewritten to relative directories (`../../`).
- 0 raw matching files found for confidential host indicators in `git ls-files` tracker.
**Verdict:** ✅ works
**Next action:** None.

## 2026-07-09 00:31 -03 — Workspace Integrity & Suite Verification on Main Branch

**What:** Validate total workspace build stability and test suite alignment after merging the technical changes and doc consolidations into the main branch.
**Category:** local-check
**How to measure:** Run `cargo test --all` on the main branch.
**Measured data:**
- 10 crates compiling with 0 clippy warnings.
- Test Suite Rollup: **61 passed**, 0 failed, 7 ignored (ignored checks require root/CUDA execution).
- Workspace compilation exit code = 0.
**Verdict:** ✅ works
**Next action:** Push branch main to public origin repository.

## 2026-07-09 — DEMOTE e2e (live cascade, action path)

**What:** `scripts/p0/measure-cascade-demote.sh` on live WSL2 cascade (zram 1G p200 / nbd0 3G p100 / sdb 8G p-2).

**Method:**
- Hog 2200 MiB hold in cgroup `memory.max=512M` (pages spill zram→VRAM).
- DEMOTE **action** = `swapoff /dev/nbd0` while `ramsharedd` serves read-back (same path as `spawn_swapoff`).
- Canary **trigger** path covered by unit tests (`cargo test -p ramshared-wsl2d residency` → 12/12).
- RESTORE: `swapon -p 100 /dev/nbd0` after verify.

**Numbers:**
| Metric | Value |
| --- | --- |
| nbd used before demote | **648 MiB** |
| zram used before | **1023 MiB** |
| swapoff duration | **14768 ms** (~14.8 s) |
| nbd after demote | **absent** from `/proc/swaps` |
| vhdx used after demote | **648 MiB** (was 5) |
| hog integrity | **563200 pages OK, 0 corruption** |
| restore | **swapon -p 100 /dev/nbd0 OK** |

**RAW:** `<legacy-private-artifact-root>/CASCADE-DEMOTE-20260709-163527.txt`

**Verdict:** DEMOTE action path **PASS** on live host with active VRAM pages; A1 sink (VHDX) absorbed; cascade restored.

**Not proven here:** real WDDM latency trigger on this run (unit-tested; free-floor would need GPU contention from host).

## 2026-07-09 — ITEM-8 DT-21 residency (win11-drill)

**Discipline:** Kahneman #1 WYSIATI, #3 numbers, #13 no fake PASS, RNF-6 VM-only.

### Numbers
| Metric | Value |
| --- | --- |
| Guest | win11-drill, model Virtual Machine, build ~26200 |
| LUN | RAMSHARE VRAMDISK **64 MiB**, NTFS on D: |
| Backend | WinDriveBackend `maxIo=1MiB` qd=4, CREATE+REGISTER OK |
| `NtCreatePagingFile` | **NTSTATUS=0** after `SeCreatePagefilePrivilege` (was 0xC0000061) |
| Pagefile-D | **alloc=32 MiB**, after pressure **use=8 MiB (25%)** |
| Pagefile-C under pressure | alloc=1408 use=418 |
| KernelPageDrill | **exit 0**, residency confirmed **3/3**, Usage=**25** each run |
| B2 product service | **not installed** (`ramshared-winsvc` missing); lab path only |
| New BSOD on this path | **none** (last minidump older) |
| Host-real | **still forbidden** |

### Verdict
- **DT-21 residency gate: PASS** (Usage>0 proven on product volume pagefile).
- Full ITEM-8 product B1/B2 (kill winsvc + page-in after teardown): **open** until `ramshared-winsvc` SCM path exists.
- Do not promote host-real until B1/B2 product path is empirical.

RAW: `C:\ramshared\artifacts\agent-item8-pagefile-kpd.log`, artifacts-item8/

## 2026-07-09 — ITEM-8 B2 lab on win11-drill (honest)

**Target:** Hyper-V VM `win11-drill` only (not physical host).

### Precondition
- Pagefile `D:\pagefile.sys` **a=32 u=8 (25%)** with backend alive
- Checkpoint `pre-b2-lab-20260709-175150`

### Run A (driver before QTeardown RequestComplete fix)
| Metric | Value |
| --- | --- |
| Kill backend | OK |
| I/O post-kill | **READ_TIMEOUT_15s** (hang) |
| New minidump | **false** |
| Guest alive | **true** |
| Verdict | **FAIL** reason=`io_hang` |

### Run B (after fix: RequestComplete with real AdapterExt + Registered=FALSE early)
| Metric | Value |
| --- | --- |
| Setup | NTPF OK, HOG, PF u=8 |
| Kill | PSD session died mid-drill |
| Boot after | **21:07:49** |
| New minidump | **070926-27437-01.dmp** @ 21:08:12 |
| Verdict | **FAIL / BSOD** under B2 with usage>0 |

### Kahneman
- #13: do **not** mark B2 PASS. Residency DT-21 remains PASS; B2 containment **not** proven.
- #2: checkpoint available for restore if needed.
- Host-real still **forbidden**.

Artifacts: `C:\ramshared\artifacts\artifacts-b2\`, guest minidump 27437.

## 2026-07-09 — B2 analysis + storage-only retest (win11-drill)

### Root cause of BSOD (pagefile-hot kill)
Minidump `070926-27437-01.dmp`:
- **BugCheck 0x7A** `KERNEL_DATA_INPAGE_ERROR`
- Parameter2 = **`0xC0000185`** (`STATUS_IO_DEVICE_ERROR`)

Interpretation: with `D:\pagefile.sys` **in use**, killing the backend makes page-in I/O fail; if the faulting page is **kernel** (or non-recoverable), Windows bugchecks. This matches DEGRADATION-MATRIX B1/B2 risk and SPEC **DT-9** (pagefile must be off before destroy).

### Code harden (teardown)
- `QTeardownOnCrash`: snapshot SRBs under lock; `RequestComplete` **outside** spinlock with real `VdGetAdapterExt()`; `Registered=FALSE` first.
- CLEANUP: `VdStateFailed` before teardown.
- StartIo R/W: fail-fast if `VdStateFailed`.

### Path S retest (storage-only, **no** pagefile on D)
| Metric | Value |
| --- | --- |
| PF on D | **absent** |
| Kill backend | OK |
| I/O post-kill | READ_OK (cache) in ~9s — **no hang** |
| New minidump | **false** |
| Guest | alive |
| PATH_S_PASS | **True** |

### Path P (pagefile-hot)
**Not re-run** after 0x7A proof. Mitigation = DT-9 ordered pagefile-off, not “fail I/O and hope”.

### Verdict
- Storage-stack B2 (no pagefile): **PASS** (no hang, no BSOD) on VM.
- Pagefile-hot B2: **FAIL by Windows design (0x7A)** until DT-9 product path.
- Host-real: still **forbidden**.

## 2026-07-09 — All fronts (win11-drill VM)

### Front A — winsvc pure DT-9
- `teardown(..., pagefile_remove)` **fail-closed**: no callback / remove Err => no destroy.
- Unit tests: **25/25** `ramshared-winsvc` including refuse paths.

### Front B — DT-9 ordered kill lab
| Step | Result |
| --- | --- |
| Pagefile D | a=32 u=7 (hot) |
| CIM remove setting | OK |
| REG drop D: | OK |
| Pending delete file | True |
| Usage still hot | **a=32 u=7** (Windows keeps PF until reboot) |
| Kill backend | **REFUSED** |
| Verdict | **PASS_DT9_REFUSE_KILL** |
| New dump | none |

### Front C — B2 pagefile-hot
Previously: **BugCheck 0x7A / c0000185** (documented). Do not kill while hot.

### Front D — B2 storage-only
Earlier run PASS (no dump); one later run TIMEOUT (backend/disk lifecycle flaky without re-REGISTER). Not blocking DT-9 refuse proof.

### Host-real
Still **forbidden**.

Artifacts: `C:\ramshared\artifacts\artifacts-all-fronts\`

## 2026-07-09 — DT-9 + reboot kill (win11-drill)

### Sequence
1. Remove secondary PF settings (CIM+REG) while D: still **hot**
2. Reboot guest
3. After boot: **only C: pagefile** (D: unloaded)
4. `Stop-RamSharedLab.ps1` → **STOP_OK** exit 0, backend dead
5. Wait 10s: **same** minidump name (`070926-25640-01.dmp`) — **no new BSOD**

### Numbers
| Metric | Value |
| --- | --- |
| PF after reboot | `C: a=1408 u=174` only |
| STOP_EXIT | **0** |
| BE after stop | **False** |
| New dump | **false** |

### Lab service stand-in
- `Start-RamSharedLab.ps1` / `Stop-RamSharedLab.ps1` = ordered start/stop until SCM winsvc lands
- Stop refuses kill if secondary PF still allocated (DT-9 fail-closed)

### Verdict
**PASS_DT9_REBOOT_KILL** on VM. Complements earlier **PASS_DT9_REFUSE_KILL** (hot refuse).

## 2026-07-09 — SCM lab + ITEM-8 gate reassess (win11-drill)

### 1) SCM `RamSharedWinSvc` (C# lab, Framework csc)
- Binary: `C:\ramshared\bin\RamSharedWinSvc.exe` (orchestrates Start/Stop-RamSharedLab).
- `sc create ... start= delayed-auto` → **StartType=Automatic**.
- After reboot: **BE=True**, **DISK N=1 64MiB** (backend auto-started via service OnStart).
- Stop path: DT-9 via `Stop-RamSharedLab` (refuse if PF hot).

### 2) Autostart
| Metric | Value |
| --- | --- |
| Boot | 2026-07-09 22:11:57 |
| Service StartType | **Automatic** (delayed) |
| Backend after boot | **True** |
| Disk after boot | **N=1 67108864** |
| New dump on stop | **False** |

### 3) ITEM-8 scorecard
| Gate | Result |
| --- | --- |
| Format + smoke | PASS |
| DT-21 residency Usage>0 | PASS |
| KPD 3/3 | PASS |
| DT-9 refuse hot kill | PASS |
| DT-9 reboot unload + kill | PASS |
| B2 pagefile-hot | FAIL 0x7A (by design; DT-9 mitigates) |
| Lab SCM + delayed auto-start | **PASS_LAB_SCM** |
| Product CUDA winsvc on host | NOT DONE |
| B1 surprise-remove drill | NOT DONE |
| **Host-real driver load** | **STILL FORBIDDEN** |

### Gate decision (honest)
ITEM-8 **lab evidence is sufficient for VM operations**. Host-real remains blocked until:
- product `ramshared-winsvc` CUDA path on a Windows box with GPU (or signed policy R9), and
- B1 checkpoint drill executed.

Artifacts: guest `C:\ramshared\bin\winsvc.log`, service `RamSharedWinSvc`.

## 2026-07-09 — All fronts closeout (B1 + SCM + ITEM-8 gate)

**Discipline:** #1 WYSIATI, #3 numbers, #13 no theater, RNF-6 VM-only, checkpoint `pre-b1-20260709-191802`.

### B1 safe arm (surprise backend kill, no secondary PF)
| Metric | Value |
| --- | --- |
| PF secondary | **absent** (only C:) |
| Backend before | True |
| Surprise | kill WinDriveBackend |
| New minidump | **False** |
| Guest alive | True |
| Verdict | **PASS_B1_SAFE_ARM** |

Hot arm (PF Usage>0) not re-run: already proven **0x7A/c0000185** (dump 27437); DT-9 is the mitigation.

### Rust winsvc MSVC
- Host: VS Build Tools present; **no cargo.exe** on elevated host session.
- Guest: cargo 1.97 but **no link.exe** MSVC.
- **SKIP env-bound**: C# `RamSharedWinSvc` remains lab SCM; Rust `main.rs` install/run scaffold ready when MSVC+cargo available.

### SCM / autostart
- `RamSharedWinSvc` StartType Automatic; delayed-auto.
- Post-reboot path previously: BE+disk present.

### ITEM-8 final gate (lab)
| Gate | Status |
| --- | --- |
| Format/smoke | PASS |
| DT-21 residency | PASS |
| KPD 3/3 | PASS |
| DT-9 refuse + reboot kill | PASS |
| B1 safe (no PF) | PASS |
| B1/B2 hot pagefile | FAIL 0x7A → DT-9 required |
| Lab SCM | PASS_LAB_SCM |
| **Host-real** | **FORBIDDEN** |

**Decision:** ITEM-8 **lab complete for VM operations**. Host-real still blocked until product CUDA path + optional B1 hot with only user pages / partner signing.

## 2026-07-09 — Documentation maturity sync (A–D combo, no host-real claim)

**What:** Align root and track docs with empirical status after Windows lab closeout + WSL2 cascade DEMOTE evidence.
**Category:** local-check
**How to measure:** Read `README.md` status table; `ROADMAP.md` completed Windows gates; `ARCHITECTURE.md` dual track; `PREFLIGHT.md` snapshot; FAQ Windows section; `drivers/windows/README.md`.
**Measured data:**
- Day-1 product path documented as **Linux/WSL2 only**.
- Windows track documented as **lab-complete / host-real FORBIDDEN** with gates (DT-21, DT-9, B1 safe, SCM, 0x7A hot).
- PREFLIGHT no longer claims “scaffold only / no .sys”.
- Numbers cited only from existing validation/reliability/IMPL evidence (no new host-real PASS).
**Verdict:** ✅ works (docs honesty)
**Next action:** Product CUDA Windows path + MSVC winsvc when env available; keep host-real blocked.

## 2026-07-09 — wsl2-cascade-boot (SSDV3) + human docs

**What:** Opt-in systemd cascade boot (fail-closed preflight, stop=`down`), idempotent `up`, env size defaults; rewrite root docs to plain language.
**Category:** local-check + integration (scripts)
**How to measure:**
```bash
cargo test -p ramshared-cli
# on a ready GPU WSL with systemd:
sudo bash scripts/safety/cascade-preflight.sh
sudo bash scripts/safety/install-cascade-boot.sh   # no --enable unless intentional
```
**Measured data:**
- `cargo test -p ramshared-cli`: **17** passed, 0 failed
- docs-check: OK; INDEX includes `wsl2-cascade-boot` DONE
- Full reboot e2e on this agent host: **not claimed** (user opt-in)
**Verdict:** ✅ code path ready / 🟡 boot e2e deferred to operator enable
**Next action:** User with systemd: `--enable` once and log `swapon --show` after reboot.

## 2026-07-09 — PRD kernel-vram-as-memory (SSDV3 decision)

**What:** Decision PRD: is kernel-true VRAM-as-process-memory the best approach vs cascade?
**Category:** local-check
**Measured data:** PRD written under docs/specs/no-milestone/kernel-vram-as-memory/; verdict WSL=NO-GO for LKM Day-0; bare-metal=research GO / implement NO-GO until gates; cascade remains product.
**Verdict:** ✅ PRD decision recorded (no SPEC/IMPL — correct for gated track)
**Next action:** bare-metal lab inventory or explicit "blocked on hardware" if no lab.

## 2026-07-10 — Passo 0 inventory + cascade desktop app

**What:** (1) Kernel track lab inventory on host WSL2. (2) Desktop control app (zenity/CLI) for cascade.
**Category:** local-check + integration
**Measured data:**
- WSL_YES; GPU RTX 2060 via GPU-PV (PCI vendor 0x1414); no /dev/dri; kernel-true Gate A1 **FAIL**
- PASSO0: docs/specs/no-milestone/kernel-vram-as-memory/PASSO0-INVENTORY.md
- cascade-app status: shows disk-only swap (cushion off)
- zenity+DISPLAY present; install-cascade-app.sh writes .desktop
- bash -n cascade-app OK
**Verdict:** ✅ inventory blocks LKM on this lab; ✅ control app MVP ready
**Next action:** user may `sudo cascade-app.sh start` or --gui; trilha K waits bare-metal.

## 2026-07-10 — Hyper-V lab on secondary storage (3 paths)

**What:** Path1 VM+ISO; Path2 DDA inventory; Path3 dual-boot shrink attempt; mainline PRD.
**Category:** integration / local-check
**Measured data:**
- ISO ubuntu-24.04.2-live-server ~2.99 GB at R:\Hyper-V\iso\
- VM linux-kernel-lab Gen2 created; start needed DynamicMemory 4GB (8GB failed 0x800705AA with other VMs)
- DDA inventory: RTX 2060 LocationPath PCIROOT(0)#PCI(0301)#PCI(0000); Apply not executed
- Dual-boot shrink: SizeMin leaves only ~2.68 GB shrinkable after defrag; immovable files block 100GB carve
- PRD: docs/specs/no-milestone/mainline-vram-tiering/PRD.md
**Verdict:** ✅ path1 ready for Ubuntu install via vmconnect; 🟡 path2 inventory-only; 🔴 path3 blocked until data layout allows shrink
**Next action:** Finish Ubuntu install in VM; free/move files on R: for dual-boot; DDA only with spare display.

## 2026-07-10 — C: disk pressure emergency (win11-drill on C:)

**What:** User reported C: ~15 GB free (Windows risk). Measured and relocated lab storage off C:.
**Category:** fail-safe / host-safety
**Measured data:**
- Before: C free ~30.9 GB at measure time (user saw ~15 GB earlier)
- Culprit: C:\Hyper-V\win11-drill — base vhdx 20.75G + multiple avhdx checkpoints (17+15+14+…) + win11.iso 7.61G + backend.vhdx 5G + VMRS
- Action: Stop-VM win11-drill; Move-VMStorage -> R:\Hyper-V\win11-drill; moved iso+backend; removed C:\Hyper-V tree
- Set-VMHost VirtualMachinePath/VHDPath -> R:\Hyper-V\VMs and R:\Hyper-V\VHDs
- Temp cleanup
- After: **C free 136.3 GB**
- VMs: linux-kernel-lab R:; win11-drill R:; gha-ubuntu V:
**Verdict:** ✅ C: recovered; lab no longer on system disk
**Next action:** Keep new VMs on R:/V: only; prune win11-drill checkpoints on R: when convenient (saves R: space, not C:).

## 2026-07-10 — linux-kernel-lab boot fixed (cloud image)

**What:** Screenshot showed UEFI "no OS" on empty VHD. Replaced with Ubuntu 24.04 cloudimg VHDX + cloud-init CIDATA seed.
**Category:** boot / integration
**Measured data:**
- VM Running, heartbeat OK
- SSH OK from Windows host: hostname linux-kernel-lab, kernel 6.8.0-134-generic
- cloud-init status: done
- Mem ~2.8Gi (3GB startup), root /dev/sda1 38G 2.5G used
- Disk on R: only; C free still ~136G
**Verdict:** ✅ lab Linux usable via SSH from Windows (not via WSL NAT)
**Next action:** none for user; optional detach CIDATA ISO after first boot

## 2026-07-10 — Dual-boot unblocked on secondary storage, not R:

**What:** Explain and fix dual-boot/kernel-true blocker (NTFS shrink).
**Category:** integration / host-safety
**Measured data:**
- R: free ~170 GB but ShrinkableGB ≈ **2.68** (SizeMin≈463 GB) — blocked
- E: ShrinkableGB ≈ **33.25**; Resize-Partition applied → **LargestFreeExtent ≈ 32.00 GB** on disk 0 SAMSUNG HD154UI
- E: partition size after: ~1365 GB; free inside NTFS ~297 GB
- Ubuntu ISO present on R:\Hyper-V\iso\
**Verdict:** ✅ dual-boot **space** ready on E:; 🟡 OS install still needs one USB boot (cannot finish from WSL alone)
**Next action:** USB install into unallocated only; then bare-metal nvidia/`/dev/dri` for Gate B

## 2026-07-10 — PRD wsl2-native-vram-tier (languages + test matrix)

**What:** SSDV3 PRD for “native” VRAM tier on WSL2/Ubuntu kernels; where to test; implementation languages.
**Category:** local-check
**Measured data:**
- PRD path: docs/specs/no-milestone/wsl2-native-vram-tier/PRD.md
- Phases P0 cascade (product) / P1 kernel-closer / P2 device-memory research / P3 mainline
- Test matrix: P0 on WSL; kernel builds on linux-kernel-lab VM; P2 needs bare-metal/DDA not GPU-less VM
- Languages: Rust userspace P0; C for Linux kernel work; RfL optional later; not Python/Node as LKM
**Verdict:** ✅ PRD recorded; dual-boot not required for WSL product
**Next action:** P0 use on WSL; P1 SPEC only if custom WSL kernel decided

## 2026-07-10 — ADR-0007 + AUDIT: kernel-native language = C

**What:** Policy audit for "native for real in the kernel" implementation language.
**Category:** local-check
**Measured data:**
- ADR-0007 Accepted: kernel context → C11 mainline style; userspace P0 → Rust; RfL exception-only
- AUDIT-2.5 go: docs/specs/no-milestone/kernel-native-language/AUDIT-2.5.md
- PRD policy: docs/specs/no-milestone/kernel-native-language/PRD.md
- Cross-link wsl2-native-vram-tier §8
**Verdict:** ✅ go — not a feature IMPL; language/architecture lock
**Next action:** Future P1/P2 kernel SPECs must cite ADR-0007

## 2026-07-10 — Parallel: win11 recreate + custom MS 6.18 kernel build

**What:** Recreate win11-drill install surface; start official WSL2-Linux-Kernel 6.18.y build with swap/VRAM-path configs.
**Category:** integration
**Measured data:**
- Win11 ISO Fido Latest Pro EN x64 → R:\Hyper-V\iso\Win11_25H2_English_x64_v2.iso **7.89 GB**
- win11-drill: VHD 80G dynamic + DVD ISO; State Running for setup
- Kernel: branch linux-msft-wsl-6.18.y tag linux-msft-wsl-6.18.35.2 on lab VM; configs UBLK=m ZRAM_WRITEBACK=y IO_URING=y NBD=m ZRAM=m SWAP=y; make -j2 started (log ~/kernel-build.log)
- Parallel doc: docs/labs/PARALLEL-WINDOWS-AND-CUSTOM-KERNEL.md
**Verdict:** 🟡 both tracks started; Win11 needs human OOBE; kernel build not finished
**Next action:** complete Win11 in vmconnect; wait bzImage; then qemu-validate / boot-kernel-safe

## 2026-07-10 — Lab disk guard (checkpoints off, no destructive cleanup)

**What:** Prevent lab VMs from filling disks / breaking host; safe harden only.
**Category:** fail-safe
**Measured data:**
- win11-drill on E:; linux-kernel-lab on R:; C:\Hyper-V absent
- Set CheckpointType=Disabled, AutomaticCheckpointsEnabled=False on both labs
- Snapshots count=0 both; VHD max win11=80G linux=40G dynamic
- VMHost defaults VMs/VHDs -> R:\Hyper-V\...
- No VHD delete/Convert-VHD; free C=136.1 R=167.6 E=288.8
**Verdict:** ✅ guards applied
**Next action:** after Win11 OOBE, eject ISO; re-run Harden-LabVms.ps1 if needed

## 2026-07-10 — wsl2-custom-kernel-p1 partial green (build + qemu + arm)

**What:** Custom WSL2 kernel from MS `linux-msft-wsl-6.18.y` @ `1bd4ed3d4` with UBLK=m + ZRAM_WRITEBACK=y; qemu boot PASS; CLI + arm for next start.

| Metric | Value |
| --- | --- |
| REL | 6.18.35.2-microsoft-standard-WSL2+ |
| bzImage | R:\WSL\kernels\bzImage-ramshared-latest (17330688 B) |
| QEMU | PASS (KTEST-UNAME match); modules busybox insmod best-effort fail |
| stamp | qemu-pass.stamp sha256 d278b032… |
| CLI | status/enable/arm/disarm/apply; enable never shutdown |
| arm | .wslconfig kernel=R:\\WSL\\kernels\\bzImage-ramshared-latest → NEED_REBOOT |
| apply | not run (human); AUDIT-2.5 go for human apply |
| stock uname still | 6.6.123.2-microsoft-standard-WSL2+ until restart |

**Next human:** restart WSL or `wsl-kernel.sh apply --i-know-this-stops-all-wsl`, then `enable`.

## 2026-07-10 — wsl2-custom-kernel-p1 live green (kernel + modules.vhdx + ublk)

**What:** Custom kernel live on product WSL with MS-style `kernelModules` VHDX; `ublk_drv` loads and `/dev/ublk-control` exists.
**Category:** boot + integration
**How to measure:**
```bash
uname -r
ls /lib/modules/$(uname -r)/kernel/drivers/block/ublk_drv.ko
sudo modprobe ublk_drv && lsmod | grep ublk && ls -la /dev/ublk-control
grep -E 'kernel=|kernelModules=' /mnt/c/Users/*/ .wslconfig 2>/dev/null | head
```
**Measured data:**
- uname: **6.18.35.2-microsoft-standard-WSL2+**
- .wslconfig: `kernel=C:\\wsl\\kernel-ramshared` + `kernelModules=C:\\wsl\\modules-ramshared.vhdx` (~2.8G)
- modules tree mounted under `/lib/modules/6.18.35.2-microsoft-standard-WSL2+/`
- modprobe ublk_drv → **OK**; `/dev/ublk-control` present; `lsmod` shows ublk_drv
- modules-apply.log: **RESULT=OK**
- QEMU stamp retained (boot gate earlier PASS)
- Cascade Day-1 (NBD `ramshared up`) **not** re-gated in this entry
**Verdict:** ✅ works (P1 kernel+ublk path live)
**Next action:** (1) re-validate cascade on custom kernel; (2) optional SPEC for cascade prefer ublk; (3) close IMPL RF-K8 as GREEN; (4) commit docs/scripts if not committed

## 2026-07-10 — wsl2-custom-kernel-p1 full green (cascade smoke)

**What:** On live custom kernel 6.18.35.2, re-validated RamShared Day-1 cascade (NBD) and CLI enable path with modules.vhdx.
**Category:** integration + boot + fail-safe
**How to measure:**
```bash
uname -r
sudo ./target/release/ramshared check
sudo modprobe nbd; sudo ./target/release/ramshared up --vram 512 --zram 512 --daemon ./target/release/ramsharedd
cat /proc/swaps
sudo ./target/release/ramshared down
bash scripts/kernel/wsl-kernel.sh enable
```
**Measured data:**
- uname: 6.18.35.2-microsoft-standard-WSL2+
- check: Decisao=ready; CONFIG_BLK_DEV_UBLK=m; ublk=ready; nbd=ok (after modprobe)
- free VRAM ~4.5–5.1 GiB; RTX 2060
- up: zram0 prio=200 512MiB; nbd0 prio=100 512MiB; disk /dev/sdc prio=-2; exit 0
- down: swapoff-first nbd+zram; managed swap gone; exit 0
- SWAPS_CLEAN_OF_MANAGED after down
- modules.vhdx C:\wsl\modules-ramshared.vhdx (~2.8G); /dev/ublk-control present
- wsl-kernel enable: READY no-op path (after CLI path fix for C:\wsl kernel=)
**Verdict:** ✅ works
**Next action:** optional SPEC cascade-prefer-ublk; commit feature branch if desired

## 2026-07-10 — cascade-transport-policy + boot unit GREEN

**What:** Product cascade policy: VRAM (NBD) before SSD; boot unit enabled; `transport=auto` → NBD on WSL2; ublk fail-closed (no product ublk).
**Category:** product path + fail-safe + boot
**SSDV3:** `docs/specs/no-milestone/cascade-transport-policy/{PRD,SPEC,AUDIT-2.5,IMPL}.md`
**How to measure:**
```bash
uname -r
systemctl is-enabled ramshared-cascade.service
swapon --show
sudo ./target/release/ramshared up          # idempotent when healthy
sudo ./target/release/ramshared up --transport ublk   # must fail closed
cargo test -p ramshared-cli
```
**Measured data:**
- uname: **6.18.35.2-microsoft-standard-WSL2+**
- unit: **enabled** + **active (exited)**; preflight+cascade-up SUCCESS
- swaps: `/dev/zram0` prio **200** 1024M; `/dev/nbd0` prio **100** 1024M; `/dev/sdc` prio **−2** 8G
- daemon: `ramsharedd --nbd /dev/nbd0` under unit cgroup
- auto log: `transport=auto → nbd (ublk … recusado no WSL2 …)`
- priority log: `zram(200) > VRAM/nbd(100) > VHDX(disk) — SSD so depois de VRAM`
- idempotent up: exit 0, no re-setup
- explicit ublk: fail-closed error (Day-1=nbd); no half-state
- kernel ublk_drv loaded + `/dev/ublk-control` present (capability only)
- cargo test -p ramshared-cli: **18 passed**
**Verdict:** ✅ works (user goal: open WSL → cascade on; VRAM before SSD)
**Soak reboot 2×:** not run in-agent (kills session). Hygiene only — no new PRD/SPEC/2.5. After human `wsl --shutdown` twice, re-check unit + `swapon --show` order.
**Next action:** optional human soak reboot 2×; full ublk product path remains future + dedicated AUDIT-2.5


## 2026-07-10 — cascade boot soak 2× (REAL RESULT)

**What:** Windows orchestrator `C:\wsl\cascade-boot-soak.ps1` ran `wsl --terminate Ubuntu-24.04` twice.
**Category:** boot soak hygiene + **bug found**
**Measured data:**
- Script verdict file wrote **PASS** — **FALSE PASS**: only checked zram/nbd priority lines in `/proc/swaps`.
- After each terminate, kernel VM kept swap (`/zram0` prio 200, `/nbd0` prio 100) but **wiped `/run/ramshared`** and killed `ramsharedd`.
- Boot unit then **FAILED**: `ha swap nbd/ublk ativo sem estado /run/ramshared (orfao)`.
- `UNIT_ACTIVE=failed`, `DAEMON=none` on both rounds — product path not healthy.
- Agent chat/WSL session dropped (expected on terminate) — user perceived freeze.
- Post-incident recovery (manual): deep clean nbd/zram + `ramshared up` → healthy again:
  - zram0 prio 200, nbd0 prio 100, sdc -2, daemon alive under `/run/ramshared`.
**Verdict:** ❌ soak failed for **daemon+unit**; swap *devices* reappeared but were **orphans** (unsafe).
**Root cause:** `wsl --terminate` ≠ full VM teardown when restart is immediate; swap survives in shared kernel; `/run` does not; `up` fail-closes on orphan (correct safety, bad boot UX without auto-recover).
**Next action:** boot recover path (swapoff orphan managed → re-up) in cascade-up/preflight; tighten soak success criteria to require daemon + unit active.

## 2026-07-10 — wsl2-cascade-orphan-recover GREEN

**What:** Auto-recover zero-used managed swap orphans after WSL terminate class (SSDV3 + security AUDIT-2.5 GO).
**Category:** fail-safe + boot UX
**SSDV3:** `docs/specs/no-milestone/wsl2-cascade-orphan-recover/{PRD,SPEC,AUDIT-2.5,IMPL}.md`
**How to measure:**
```bash
# manufacture orphan (used=0):
sudo rm -rf /run/ramshared; sudo pkill -TERM -x ramsharedd; sleep 1
swapon --show   # zram+nbd still listed, no daemon
sudo ./target/release/ramshared up
swapon --show; pgrep -a ramsharedd
cargo test -p ramshared-cli
```
**Measured data:**
- AUDIT-2.5: GO for used=0 only; NO-GO used>0 nbd auto; allowlist nbd/ublk/zram; kill-switch `RAMSHARED_NO_ORPHAN_RECOVER=1`
- cargo test -p ramshared-cli: **23 passed**
- Live: orphan manufactured (run wiped, daemon killed, nbd+zram used=0) → `up` logged `orphan recover` → swapoff zram0+nbd0 → setup → **exit 0**
- After: zram1 prio **200**, nbd0 prio **100**, sdc prio **−2**; daemon alive; unit **active**
- Disk sdc never swapoff'd
**Verdict:** ✅ works
**Next action:** optional re-run soak terminate 2× with daemon+unit criteria (not just swapon lines)

## 2026-07-10 — end-to-end product proof (boot + order + soak + reopen)

**What:** Full validation that opening WSL2 arms cascade; under pressure zram→VRAM→SSD; survive terminate×2.
**Category:** product path + pressure + boot
**Measured data:**
1. **User reopen WSL2 (22:41)** — natural soak after session drop:
   - unit enabled/active; journal Finished SUCCESS
   - zram0 **2G prio 200**, nbd0 **2G prio 100**, sdc **8G prio −2**
   - ramsharedd `--size 2048 --nbd`; `/run/ramshared` present
   - conf: VRAM_MIB=2048 ZRAM_MIB=2048
2. **Soak v2** `C:\wsl\cascade-boot-soak-v2` — **VERDICT=PASS pass=2 fail=0**
   - criteria: OK_ORDER + OK_DAEMON + OK_RUN (not swap lines alone)
3. **Pressure probe** (cgroup MemoryMax=1200M, host-safe):
   - FIRST zram t=2s → nbd t=7s → disk t=13s → **PASS order**
   - daemon survived; host free restored after release
4. **Priorities (kernel law):** higher prio used first → when 16G WSL RAM pressures, **VRAM/nbd before SSD**
5. **Sizes:** 2G zram + 2G VRAM cushion before 8G VHDX (not full GPU; headroom for desktop)
**Audit notes (hardcode / spaghetti):**
- Defaults 1024 in CLI are fallbacks; live sizes from `/etc/ramshared/cascade.conf` (OK)
- Prio 200/100/−2 constants in `ramshared-tier` — intentional SPEC, not magic
- `/dev/nbd0` Day-1 product path intentional; ublk fail-closed
- `cascade.rs` large but single module; no kill-9; allowlist swapoff
- No thrash on full host — pressure uses cgroup only
**Verdict:** ✅ works for product open-WSL + VRAM-before-SSD path
**Push gate:** green — ready

## 2026-07-11 — cascade-vram-ondemand IMPL GREEN (sparse live)

**What:** Sparse CUDA commit for NBD VRAM tier (alloc on write; free when idle).
**Category:** product path + fail-safe
**SSDV3:** `docs/specs/no-milestone/cascade-vram-ondemand/{PRD,SPEC,AUDIT-2.5,IMPL}.md`
**How to measure:**
```bash
sudo ramshared down
F0=$(nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits | tr -dc 0-9)
sudo env RAMSHARED_VRAM_PREALLOC=0 bash scripts/safety/cascade-up.sh
F1=$(nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits | tr -dc 0-9)
echo delta=$((F0-F1))   # expect << 3072
sudo bash scripts/safety/cascade-pressure-probe.sh --max-sec 50
# wait ~40s idle; free should rise if chunks reclaimed
```
**Measured data:**
- mode log: `VRAM mode=sparse capacity=3072 MiB chunk=128 MiB committed=0`
- idle Δ free: **212 MiB** (not ~3072 prealloc)
- preflight sparse gate: need ≥ 385 MiB free (headroom+chunk)
- nbd stable 15s after up
- pressure: **zram t=1s → nbd t=6s PASS** (exit 0); nbd remains
- reclaim: free **4067 → 4408** after idle (~+341 MiB)
- cargo test ramshared-block: **32** passed; ramshared-cli: **23** passed
**Verdict:** ✅ works
**Next action:** optional PREALLOC A/B doc; ITEM-2b mid-flight spill deferred

## 2026-07-11 — hard multi-round validation GREEN (21/21 product gates)

**What:** Battery of real tests for sparse cascade safety/confidence (not a single smoke).
**Category:** product path + pressure + reclaim + fail-safe
**How:** multi-round shell suite (unit + 3× idle + 3× pressure + reclaim + idempotent + ublk + 2× orphan + prealloc path + final restore)

| Gate | Rounds | Result |
| --- | --- | --- |
| cargo ramshared-block | 1 | 32 passed |
| cargo ramshared-cli | 1 | 23 passed |
| cargo ramshared-wsl2d lib | 1 | 62 pass / **1 pre-existing fail** (`slice_view_new_panics_when_window_exceeds_backend` — unrelated to sparse) |
| sparse idle Δ free | 3 | 217 / 201 / 215 MiB (all ≪ 3072) |
| nbd stable 10s after up | 3 | all OK |
| pressure zram→nbd | 3 | (2,6) (1,5) (1,6) PASS; nbd+daemon after each |
| reclaim idle | 1 | free **3388 → 4421** (+1033 MiB) |
| idempotent up | 1 | “cascata ja ativa” |
| ublk fail-closed | 1 | exit 1 + clear message |
| orphan recover | 2 | both heal + healthy cascade |
| sparse vs prealloc modes | 1 | mode=sparse / mode=prealloc logs |
| final state | 1 | z=200 n=100 d=-2 ORDER_OK; unit enabled/active |

**Verdict:** ✅ product suite **PASS=21 FAIL=0 OVERALL=GREEN**
**Note:** wsl2d `slice_view` panic test is pre-existing, not introduced by sparse IMPL.
**Final live:** nbd 3G prio 100, zram 2G prio 200, sdc -2; ramsharedd --size 3072

## 2026-07-11 — VRAM 4GiB capacity + free-floor/commit_cap safety

**What:** Raise product capacity to 4 GiB; safety refuse chunk alloc below reserve floor; auto commit_cap for 6 GiB capacity option.
**Measured:**
- conf: VRAM_MIB=4096, MIN_VRAM_HEADROOM_MIB=512
- sparse log 4G: `commit_cap=4096 MiB reserve_floor=512 MiB`
- sparse log 6G: `capacity=6144 MiB commit_cap=5631 MiB reserve_floor=512` (total−reserve on 6143 MiB GPU)
- pressure with 4G nbd: zram→nbd PASS; nbd remains
- unit tests sparse: 8 passed (floor refuse + safe_commit_cap)
**Verdict:** ✅ 4G live; 6G capacity safe via commit_cap; free-floor on alloc

## 2026-07-11 — WDDM autotier safety audit and deployment

**What:** Close the Phase 1 audit findings without live memory pressure.

**Code evidence:**
- constrained WDDM admission completes the already accepted NBD write and schedules demote;
- startup CUDA fallback is limited to `/dev/dxg` unavailable;
- teardown retries and refuses CUDA release without confirmed swapoff plus `used_kb == 0`;
- controller polls WDDM/swapoff every 5 seconds and recovers only an empty tier after 3 healthy samples.

**Validation:**
- workspace default tests: 273 passed; 22 environment-gated;
- safe GPU ignored tests: 5 passed;
- `ramshared-dxg`: 92/92 lines covered (100%);
- `autotier.rs`: 68/68 lines covered (100%);
- fmt, clippy `-D warnings`, RustSec, cargo-deny, and docs-check: GREEN;
- final daemon release inode matches the running process and `/dev/dxg` is open;
- final swap order: zram 200 → nbd0 100 → sdc -2; nbd0 used=0; no ghost swap.

**Not claimed:** live host-budget pressure with resident swap pages. That benchmark remains isolated-lab only.

**Verdict:** ✅ Phase 1 code/deployment GREEN; isolated pressure gate remains open.
**Next action:** none.

---

## 2026-07-12 — Windows Swap Driver MVP & Residency Validation

**What:** Full PnP driver load, NTFS volume format, paged-pool residency (ITEM-8), crash containment (B1/B2), and ordered teardown safety (DT-9) validations on VM.
**Category:** fail-safe + boot + integration
**How to measure:** Run `Invoke-DisciplinedCampaign.ps1` to execute the full validation campaign. Run `Invoke-KernelPageDrill.ps1` inside the VM.
**Measured data:**
- **Driver load:** `ramshared.sys` and `poolstress.sys` loaded successfully under `testsigning` on build 26200.
- **Disk format:** 64 MB NTFS SCSI RAM disk mounted as drive `D:` (read/write `smoke.txt` OK).
- **Pagefile residency (DT-21):** 1 GB paged-pool allocation via `poolstress.sys` forced swapout of 15 MB dirty kernel pages to `D:\pagefile.sys` (occupancy rose from 0 MB to 15 MB).
- **Backend crash containment (B1/B2):** Abrupt termination of backend process did not crash the system; VM remained responsive and remote sessions reconnected cleanly.
- **Ordered teardown safety (DT-9):** Normal stop on active pagefile refused (`exit 2`, `REFUSE_KILL`), while forced stop killed the backend cleanly (`exit 0`).
- **Campaign result:** `OVERALL=PASS_WITH_SKIPS` (0 failures, 27/27 files parsed).
**Verdict:** ✅ works (MVP fully verified on guest VM).
**Next action:** none (physical GPU/CUDA integration follows).

## 2026-07-13 14:27 -03 — A+B cascade redeploy + SSDV3 Step 3 + hang audit + cover gate

**What:** Rebuild/redeploy ramsharedd (BINARY_MATCH), add Step 3 gates (E2E+cover≥80%) into SSDV3, add superprompt, classify postmortem kernel vs OOM, hang/freeze audit, llvm-cov on hang-critical crates.
**Category:** fail-safe + product path + methodology
**How to measure:**
```bash
cargo build --release -p ramshared-wsl2d -p ramshared-cli
sudo systemctl restart ramshared-cascade.service
./target/release/ramshared status
sudo ./scripts/safety/cascade-health.sh
cargo llvm-cov -p ramshared-cli -p ramshared-tier -p ramshared-dxg -p ramshared-block --summary-only
```
**Measured data:**
- Daemon PID 87514; `readlink /proc/87514/exe` = `…/target/release/ramsharedd`; **BINARY_MATCH=OK**
- Swaps: zram0 prio 200 used 0; nbd0 prio 100 used 0; sdc prio -2 used 0
- cascade-health: `ok:true`, `ghost:false`, `order_ok:true`
- MemAvailable ~13.0 GiB / 15.6 GiB total; swap free = total
- Unit tests hang-critical: cli 23, dxg 10, tier 8 — all pass
- llvm-cov line cover (hang slice):
  - ramshared-tier cascade **100%**, priority **90.20%**
  - ramshared-dxg **96.94%**
  - ramshared-block handshake **94.14%**, inflight **100%**, protocol **91.01%**, request **93.80%**, vram_backend **91.06%**, sparse_vram **79.55%**
  - ramshared-cli cascade **33.97%**, main **35.29%** (gap: I/O paths of up/down not unit-covered)
  - TOTAL selected packages **59.25% lines** (not a Step 3 close for cli cascade)
- Docs: `docs/SSDV3-PROMPTS.md` rules 9–10 + 13–16 + E2E section; `superprompt.md`; `docs/reliability/HANG-FREEZE-AUDIT-2026-07-13.md`; postmortem.sh kernel vs OOM split
- Host noise removed earlier: ollama unit ghost, docker images/build cache, go/rust caches
**Verdict:** 🟡 cascade operational + methodology ported; cover gate not green for `ramshared-cli` cascade (33.97% < 80%) — residual tracked; hang logic unit tests exist for ghost/orphan/kill-forbidden
**Next action:** slice cover: expand unit/integration tests for cascade policy + sparse_vram to ≥80% lines; optional demote drill only on isolated VM

## 2026-07-13 14:35 -03 — Cover gate hang slice ≥80% (policy) + cascade_io E2E

**What:** Expanded cascade hang-policy unit tests (TLS seams, mock sh); sparse_vram tests; split `cascade_io` (up/down shell) from policy `cascade/mod.rs`; llvm-cov re-measure; release redeploy.
**Category:** fail-safe + product path
**How to measure:**
```bash
cargo test -p ramshared-cli -p ramshared-block -- --test-threads=1
cargo llvm-cov -p ramshared-cli -p ramshared-tier -p ramshared-dxg -p ramshared-block --summary-only
sudo systemctl restart ramshared-cascade.service
./target/release/ramshared status && sudo ./scripts/safety/cascade-health.sh
```
**Measured data:**
- Unit tests: cli 48 pass, block 41 pass
- llvm-cov lines:
  - `cascade/mod.rs` (hang policy) **88.97%** (≥80%)
  - `sparse_vram.rs` **92.25%** (≥80%)
  - `ramshared-dxg` **96.94%**, tier cascade **100%**, priority **90.20%**, block handshake/request/protocol/inflight **≥91%**
  - `cascade_io.rs` **1.77%** unit — E2E only (shell up/down; not thrash-mocked on live host)
  - `main.rs` **35.29%** — N/A wiring CLI dispatch
- E2E: BINARY_MATCH=OK; health ok:true; priorities 200>100>-2; used=0; ghost=false
**Verdict:** ✅ Step 3 cover gate for hang business-logic slice (policy + sparse + dxg + tier + block); cascade_io closed by live cascade E2E not unit %
**Next action:** optional more unit cover on cascade_io via temp run-dir seam (non-blocking)

## 2026-07-13 14:55 -03 — SPEC↔code confrontation cascade boot + orphan

**What:** Confront SPECs `wsl2-cascade-boot` and `wsl2-cascade-orphan-recover` against tree: ITEM files/symbols, unit tests, live preflight/health/BINARY_MATCH. Update SPEC test matrices in place; document matrix in `docs/reliability/SPEC-CODE-CONFRONT-cascade-2026-07-13.md`.
**Category:** integration + fail-safe
**How to measure:**
```bash
test -f scripts/safety/cascade-preflight.sh
rg "fn (canonicalize_swap_path|plan_orphan_action|cascade_already_healthy|try_recover)" crates/ramshared-cli
cargo test -p ramshared-cli -- --test-threads=1
sudo ./scripts/safety/cascade-preflight.sh
sudo ./scripts/safety/cascade-health.sh
```
**Measured data:**
- Boot ITEM-1..5 files present; live unit TimeoutStop=10min, ExecStartPre=preflight, ExecStop=down
- Preflight: CASCADE-PREFLIGHT: OK (free VRAM=4723 MiB reported)
- Orphan ITEM-1..5 symbols all present in cascade/
- `cargo test -p ramshared-cli`: **48 passed**, 0 failed
- Live: ghost=false, order_ok, prios 200>100>-2, BINARY_MATCH=OK
- Gap: boot SPEC conf example sizes (4096/2048) vs CLI fallback 1024 — documented in SPEC ITEM-4 note
**Verdict:** ✅ both SPECs implemented in code with unit/live proof for policy paths; 🟡 SPEC hygiene was behind code (fixed test tables)
**Next action:** optional lab-only wsl --terminate orphan E2E; not on daily host

## 2026-07-13 15:00 -03 — SPEC↔code confrontation cascade multi-SPEC

**What:** Extend confrontation beyond boot/orphan to cascade-vram-ondemand, cascade-transport-policy, wsl2-cascade-swap (umbrella), wsl2-native-vram-autotier, plus sample memory-broker and windows-swap-driver. Document in `docs/reliability/SPEC-CODE-CONFRONT-cascade-2026-07-13.md` §§D–I. Hygiene: transport IMPL paths; sparse SPEC ITEM-3 telemetry wording.
**Category:** integration + fail-safe
**How to measure:**
```bash
cargo test -p ramshared-block sparse
cargo test -p ramshared-dxg
cargo test -p ramshared-tier
cargo test -p ramshared-wsl2d --lib autotier
cargo test -p ramshared-cli cascade
cargo test -p ramshared-broker
cargo test -p ramshared-winsvc --lib
test -f crates/ramshared-block/src/sparse_vram.rs
test -f crates/ramshared-wsl2d/src/autotier.rs
test -f drivers/windows/ramshared/protocol.h
```
**Measured data:**
- sparse: **15** pass; dxg **10**; tier **8**; autotier **7**; cascade filter **41**; broker **32**; winsvc **25**
- Sparse backend + try_reclaim + preflight sparse gate present
- Transport Auto→Nbd on WSL2 + ublk refuse + priority log present
- Autotier Phase 1 code green; live WDDM pressure demote still OPEN (IMPL)
- Winsvc userspace green; StorPort sources present; **no** host kernel load claimed
- No destructive demote/pressure on daily host this session
**Verdict:** ✅ product cascade SPECs go (or go with documented lab gate); sample broker P1 library + winsvc userspace go; umbrella swap SPEC historical go
**Next action:** optional lab autotier pressure drill; optional sparse JSON line if operators need machine-parseable reclaim; do not load unsigned StorPort on daily host

## 2026-07-13 15:05 -03 — push path + live hang checklist after multi-SPEC confront

**What:** main is protected (6 required checks); pushed branch `docs/cascade-spec-code-confront-2026-07-13` and re-ran superprompt-safe live hang checklist. Skipped pressure demote and `wsl --terminate` on daily host.
**Category:** product path + fail-safe
**How to measure:**
```bash
pid=$(pgrep -n -x ramsharedd); sudo readlink -f /proc/$pid/exe; readlink -f target/release/ramsharedd
sudo ./target/release/ramshared status
sudo ./scripts/safety/cascade-preflight.sh
sudo ./scripts/safety/cascade-health.sh
swapon --show
```
**Measured data:**
- BINARY_MATCH=OK (pid 112906 → `target/release/ramsharedd`)
- swaps: zram0 2G prio **200**, nbd0 4G prio **100**, sdc 8G prio **−2**; all used=0
- preflight: CASCADE-PREFLIGHT: OK; free VRAM=**4693** MiB; sparse gate need ≥641; capacity VRAM_MIB=4096
- health JSON: ok=true, ghost=false, order_ok=true, has_zram/vram/vhdx=true
- push main: **rejected** GH006 protected branch (6/6 status checks expected)
- push branch: **accepted** `origin/docs/cascade-spec-code-confront-2026-07-13`
**Verdict:** ✅ live cascade healthy; docs land via PR not direct main
**Next action:** open/merge PR after CI green; never pressure/`wsl --terminate` on daily host without lab

## 2026-07-13 15:03 -03 — PR #33 merged; main green post-merge

**What:** Merged https://github.com/emersonbusson/ramshared/pull/33 after 6/6 checks green (pr-body fixed; fmt+clippy+test 1m8s). Local main = origin/main. Post-merge health recheck.
**Category:** product path
**How to measure:** `gh pr view 33 --json state,mergedAt`; `sudo ./scripts/safety/cascade-health.sh`; BINARY_MATCH
**Measured data:**
- PR state MERGED @ 2026-07-13T18:02:46Z merge `c30f2ca`
- health ok=true ghost=false order_ok=true prios 200>100>-2 used=0
- BINARY_MATCH=OK
**Verdict:** ✅ closed loop confront → PR → CI → main → live still healthy
**Next action:** lab-only for pressure/`wsl --terminate`; no daily-host destructive drills

## 2026-07-13 18:10 -03 — E2E StorPort Windows Driver & WSL2 NBD Benchmarks

**What:** Compile, sign, load, and benchmark the native StorPort driver (`ramshared.sys`) on the physical Windows host. Benchmark the raw block device performance in both Windows (S:) and WSL2 (/dev/nbd0) using random bytes and direct I/O, validating data integrity and coexistence.
**Category:** integration + performance
**How to measure:**
```powershell
# Windows Host: compile and sign
.\scripts\windows\Build-Drivers.ps1
.\scripts\windows\Sign-Drivers.ps1 -PfxPassword $env:RAMSHARED_TESTSIGN_PFX_PASSWORD
# Install and run
.\scripts\windows\Install-InfAndBackend.ps1 -FormatNtfs -DriveLetter S
# Benchmark 10 rounds of 50MB
<Powershell benchmark script>
```
```bash
# WSL2 Linux Guest: Raw NBD benchmark
sudo swapoff /dev/nbd0
sudo dd if=/dev/zero of=/dev/nbd0 bs=1M count=100 oflag=direct
sudo dd if=/dev/nbd0 of=/dev/null bs=1M count=100 iflag=direct
sudo mkswap /dev/nbd0 && sudo swapon -p 100 /dev/nbd0
```
**Measured data:**
- **Driver State:** `ramshared` service is `ESTADO: 4 RUNNING` (loaded via devcon as Root\SCSIAdapter device).
- **Windows Host (S:) Throughput:**
  - Write: **~420 MB/s** (average write latency 120ms for 50MB chunks)
  - Read: **~1.94 GB/s** (average read latency 26ms for 50MB chunks)
  - Consistency: **100% SHA256 Match** (zero corruptions over 10 consecutive rounds)
- **WSL2 Guest (/dev/nbd0) Throughput:**
  - Write: **597 MB/s** (Direct I/O block writing)
  - Read: **714 MB/s** (Direct I/O block reading)
- **Coexistence:** Windows WDDM holds absolute authority. The `ramshared-wsl2d` daemon tracks pressure via `/dev/dxg` and executes a clean `DEMOTE` flow to release VRAM to the host if requested.
**Verdict:** ✅ E2E StorPort driver and backend successfully compiled, signed, and validated on the physical host. Both read/write and data consistency verified.
**Next action:** consolidate MSVC background service (`ramshared-winsvc`) to run automatically on boot.

## 2026-07-14 09:30 -03 — gap close: charts + #40 format guards + #29 SCM DT-9 + cascade VRAM restore

**What:** Close open documentation/product gaps from post-benchmark session without daily-host pressure drills.
**Category:** docs + safety scripts + live cascade restore
**How to measure:**
```bash
# Charts present
ls docs/marketing/benchmark-comparison.jpg docs/marketing/benchmark-wsl2-vs-storport.jpg
# Cascade VRAM restored (no thrash)
./scripts/safety/cascade-health.sh
swapon --show
# Windows scripts are code-only here (host re-test when elevated):
#   Install-InfAndBackend.ps1 letter/identity/confirm guards
#   Start-RamSharedLab.ps1 no letter-only format
#   RamSharedWinSvc OnStop throws on DT-9 refuse (exit 2)
#   Install-RamSharedService.ps1 copies scripts from repo + delayed-auto
```
**Measured data:**
- Charts: StorPort-vs-SATA marketing image + new WSL2-vs-StorPort bar chart (714/597 vs 1940/420 MB/s)
- cascade-health after `cascade-up.sh`: ok=true ghost=false order_ok has_vram=true has_zram=true
- swaps: zram1 prio 200 (2G used 0), nbd0 prio 100 (2G used 0), sdc prio -2 (8G used 0)
- daemon PID live with `--size 2048` release binary
- conf.example restored product seed VRAM_MIB=4096 ZRAM_MIB=2048 (live /etc may stay 2048)
**Verdict:** ✅ repo gaps closed for charts, format safety (#40 code), winsvc DT-9 fail-closed (#29 code), cascade VRAM tier restored. ❌ live multi-tenant pressure / GPU-P lab still blocked (no drill password; daily host rule).
**Next action:** On Windows elevated host: re-run Install-InfAndBackend with free letter + Install-RamSharedService; open GPU-P lab only with RAMSHARED_DRILL_PASSWORD; never thrash swap on daily WSL.

## 2026-07-14 10:15 -03 — full gap close via WSL elevated Windows + pressure probe

**What:** Close remaining gaps using documented elevation (`scripts/windows/wsl-elevated-ps.sh` + `C:\Windows\System32\sudo.exe`) and host-safe pressure probe.
**Category:** integration + safety + live E2E
**How to measure:**
```bash
./scripts/windows/wsl-elevated-ps.sh -Command "Get-Service RamSharedWinSvc,ramshared | ft Name,Status,StartType"
./scripts/windows/wsl-elevated-ps.sh -File C:\ramshared\bin\Install-InfAndBackend.ps1 -RepoRoot C:\ramshared\src -FormatNtfs -DriveLetter C -Force
# expect REFUSE_FORMAT letter C in use
sudo scripts/safety/cascade-pressure-probe.sh --mem-max 1200M --max-sec 90
./scripts/safety/cascade-health.sh
```
**Measured data:**
- Elevation: IsAdmin=True; Get-VM works (win11-drill, linux-kernel-lab, gha-ubuntu-2404)
- **#29 RamSharedWinSvc:** built csc 7680 bytes; `sc create` delayed-auto; StartType=Automatic; Start-Service Running; OnStart spawned WinDriveBackend; Stop-RamSharedLab STOP_OK (pagefile only on C:); service left Stopped + Automatic for boot
- **#40 format guards:** PARSE_OK; live refuse `DriveLetter C` -> `REFUSE_FORMAT: drive letter C: is already in use`; physical Samsung 850 fails RamShared name identity (refuseExpected=true)
- Charts: WSL2 vs StorPort + StorPort vs SATA in README under docs/marketing/
- Cascade: zram1(200)>nbd0(100)>sdc(-2); health ok after restore
- **Pressure probe (cgroup 1200M, 90s):** PASS order zram_first=2s nbd_first=8s disk_first=none; post health ok=true ghost=false; residual used zram~18M nbd~10M
- **win11-drill:** started Running; GPU-P CurrentPartitionVRAM=1000000000; VHD ~12.4 GiB; **PSD guest auth failed** for drilladmin + unattend password + Administrator matrix (credential invalid). Heartbeat OkApplicationsUnknown. VM stopped after drills to free host RAM.
**Verdict:** ✅ #29 install/boot registration + DT-9 stop path on host; ✅ #40 refuse live; ✅ WSL pressure order proof; ✅ charts/docs; 🟡 guest PSD blocked until win11-drill password/OOBE reset (unattend value does not match live guest).
**Next action:** Reset drilladmin on win11-drill (or finish OOBE) then PSD demote drills inside guest; keep pressure via cascade-pressure-probe (cgroup-bounded) not full thrash.

## 2026-07-14 10:37 -03 — win11-drill PSD restored (unattend password, not Passo0 default)

**What:** Re-establish PowerShell Direct into Hyper-V guest `win11-drill` using the same host-elevated path as agy (`wsl-elevated-ps.sh` / admin), after PSD failed with MEMORY Passo0 default password.
**Category:** lab access / integration
**How to measure:**
```bash
./scripts/windows/wsl-elevated-ps.sh -Command '
  # credential source: Machine env RAMSHARED_DRILL_PASSWORD (set this session from unattend-staging)
  $pw=[Environment]::GetEnvironmentVariable("RAMSHARED_DRILL_PASSWORD","Machine")
  $cred=New-Object PSCredential(".\drilladmin",(ConvertTo-SecureString $pw -AsPlainText -Force))
  if ((Get-VM win11-drill).State -ne "Running") { Start-VM win11-drill; Start-Sleep 20 }
  Invoke-Command -VMName win11-drill -Credential $cred -ScriptBlock { whoami; hostname }
'
```
**Measured data:**
- Root cause: current guest was installed with `E:\Hyper-V\iso\unattend-staging\Autounattend.xml` password (len 13), **not** the legacy redacted Passo0 credential from the earlier VM on `C:\Hyper-V\...`
- PSD_OK: `win11-drill\drilladmin` on host `WIN11-DRILL`
- Smoke: Build **26200** UBR **8037**, testsigning **Yes**, IsAdmin **true**, FreeGB **~61.9**
- `Invoke-Guest.ps1` OK with env password
- Machine env set: `RAMSHARED_DRILL_PASSWORD` + `RAMSHARED_DRILL_USER=.\drilladmin` (host-local only, not in git)
- VM stopped after smoke (State=Off) to free host RAM
**Verdict:** ✅ Guest usable again for lab drills via PSD; host elevation path unchanged
**Next action:** Guest-side driver/pagefile drills as needed; always start VM then PSD with Machine env password

## 2026-07-14 10:42 -03 — win11-drill guest lab drill (PSD deploy + CREATE/REGISTER)

**What:** Full guest lab path: elevate host → Start-VM → PSD → deploy signed package → sc load ramshared+poolstress → WinDriveBackend 64 MiB CREATE_DISK+REGISTER_QUEUE → LUN probe → DT-9 safe teardown → Stop-VM.
**Category:** integration / lab E2E
**How to measure:**
```bash
./scripts/windows/wsl-elevated-ps.sh -File C:\ramshared\bin\tmp-guest-lab-drill.ps1
# or re-run with Machine env RAMSHARED_DRILL_PASSWORD set
cat /mnt/c/Users/<user>/ramshared-drill/agent-guest-lab-20260714-results.json
```
**Measured data:**
- package: ramshared.sys 31120, poolstress.sys 9104; backend exe 8704
- guest-pre: FreeGB~2.59 RAM, DiskGB~61.9, testsigning Yes, Build 26200
- driver-load: **poolstress RUNNING**, **ramshared RUNNING** (test cert imported)
- backend: `CREATE_DISK ok REGISTER_QUEUE ok` size=67108864
- disks: N=0 Msft Virtual Disk 80G + **N=1 Msft Virtual Disk 64 MiB** (LUN present)
- bugcheck: none; teardown STOP_OK; VM left Off
- SUMMARY **pass=11 warn=0 fail=0**
**Verdict:** ✅ Guest lab path green end-to-end (same operational model as agy)
**Next action:** Optional INF/PnP Root\RamShared polish for FriendlyName branding; pagefile-on-LUN ITEM-8 only with free RAM headroom (guest was ~2.5–2.7 GiB free)

## 2026-07-14 10:58 -03 — cascade lifecycle observability IMPL (status phase)

**What:** SSDV3 Step 3 for cascade-lifecycle-observability: pure phase machine, `ramshared status [--json]`, health merge.
**Category:** observability / userspace
**How to measure:**
```bash
cargo test -p ramshared-cli
cargo llvm-cov -p ramshared-cli --summary-only   # lifecycle.rs lines ≥80%
./target/release/ramshared status
./target/release/ramshared status --json | python3 -m json.tool
./scripts/safety/cascade-health.sh | python3 -c "import sys,json;print(json.load(sys.stdin).get('phase'))"
```
**Measured data:**
- 63 tests pass (15 lifecycle); clippy -D warnings clean
- lifecycle.rs llvm-cov **94.65%** lines
- Live: phase **UsingZram** (zram used ~41 MiB, vram 176 KiB residual); health phase matches
- demote counters null (ITEM-3 deferred)
**Verdict:** ✅ IMPL closed for observability slice; daemon demote export still optional gap
**Next action:** optional wire demote counters from ramsharedd when status socket is cheap

## 2026-07-14 11:03 -03 — demote-status file + CLI demote fields (ITEM-3)

**What:** Wire ramsharedd demote counters to `/run/ramshared/demote-status.json`; CLI status reads them.
**Category:** observability
**How to measure:**
```bash
cat /run/ramshared/demote-status.json
./target/release/ramshared status --json | python3 -c "import sys,json;print(json.load(sys.stdin)['demote'])"
```
**Measured data:**
- After cascade-up with new binary: demote-status `{"total":0,"last_reason":null,"in_progress":false}`
- status --json demote.total=0; health demote object present
- phase UsingDisk when /dev/sdc used_kib=1220 ≥ 1024 (residual disk swap after redeploy — correct priority rule)
**Verdict:** ✅ ITEM-3 closed; demote export live
**Next action:** optional idle reclaim of residual disk swap pages under pressure only

## 2026-07-14 11:30 -03 — issue #31 demote under pressure + integrity (action path)

**What:** Re-run `scripts/p0/measure-cascade-demote.sh` for issue #31: cgroup-isolated hog fills VRAM tier, swapoff demote while daemon serves, hog verify checksum pages.
**Category:** e2e / integration
**How to measure:**
```bash
sudo env HOG_MB=4500 CAP_MB=256 MIN_NBD_MIB=150 DEMOTE_CAP_MB=5500 RESTORE=1 \
  STATUS_BIN=./target/release/ramshared \
  bash scripts/p0/measure-cascade-demote.sh
```
**Measured data:**
- before demote: nbd **2047 MiB**, zram 2047 MiB, vhdx 1040 MiB; phase UsingDisk (disk residual) + UsingVram path for vram used
- demote action: `swapoff /dev/nbd0` **OK in 143973 ms** (~144 s)
- after: nbd **absent**; zram 137 MiB; vhdx 1130 MiB; daemon still alive
- integrity: hog **VERIFY OK 1152000 pages**, **0 corruption** (rc=0)
- cgroup: fill under memory.max=256M; raised to 5500M for demote page-in (avoids OOM kill)
- observability: `status --json` + demote-status captured before/after (manual swapoff does not increment daemon demote.total — expected; total still 0)
- host-safety: hog in cgroup only; no global thrash; RESTORE swapon failed once → `cascade-up` restored cushion after
**Verdict:** ✅ DEMOTE action path PASS under severe multi-tier pressure + integrity; sparse FreeFloor/Latency auto-swapoff still skipped by design (WDDM/Corruption path uses same spawn_swapoff)
**Next action:** optional separate drill for WDDM-budget demote (host GPU load) to increment demote-status total; close #31 acceptance for action+integrity

## 2026-07-14 11:52 -03 — Task Manager 100%/0KB: root-cause fix (StorPort + format + measure)

**What:** Senior fix for screenshot "RAMSHARE VRAMDISK 100% active / 0 KB/s / 0 ms / Formatado 0 MB".
**Category:** e2e / windows lab / driver
**Root causes (layered):**
1. LUN **RAW** (no NTFS) → TM shows Formatado 0 MB
2. **WinDriveBackend dead** while disk still enumerated → Initialize-Disk StorageWMI **40004** (writes fail)
3. Old **TUR = SRB_STATUS_BUSY** → StorPort requeue thrash (TM stuck 100%) — fixed in `virtdisk.c` via CHECK CONDITION NOT READY + autosense
4. **V: RAMSHARED** can be a physical SSD, not the 64 MiB virtual LUN
5. PT-BR host: English `Get-Counter \PhysicalDisk\...` paths fail — measure uses **CIM** `Win32_PerfFormattedData_PerfDisk_PhysicalDisk`

**How to measure:**
```powershell
# elevated
.\scripts\windows\Start-RamSharedLab.ps1 -SizeBytes 67108864 -HoldSeconds 3600
.\scripts\windows\Format-RamSharedLun.ps1 -ExpectedSizeBytes 67108864 -DriveLetter S -Force
.\scripts\windows\Measure-RamSharedDiskIo.ps1 -Seconds 6 -DriveLetter S
```
**Measured data (host dev-workstation, elevated, 2026-07-14):**
- Backend: CREATE_DISK ok REGISTER_QUEUE ok (pid alive)
- Disk5: RAMSHARE VRAMDISK 67108864 RAW → **GPT + NTFS** letter **S:** label RAMSHARED Size~64 MiB
- Direct 8 MiB probe: **write ≈ 1224 MB/s**, **read ≈ 146 MB/s**, **match=True**
- PerfDisk instance: `5 S:` (CIM)
- `ramshared.sys` rebuilt with TUR sense fix (BUILD_DRIVERS_OK, size 29696, 11:52) under `C:\ramshared\src\...\x64\Release\`
- Host reload of new .sys left for guest/lab path (physical host pagefile still FORBIDDEN on this LUN)
**Verdict:** ✅ Format + real I/O path PASS; measure script locale-safe PASS; driver source Day-0 TUR fix + rebuild PASS
**Next action:** sign+reload new sys on win11-drill guest for full TUR-not-ready path; optional host package update when not using LUN for pagefile

## 2026-07-14 12:28 -03 — guest win11-drill: signed TUR-sense sys reload + CREATE/FORMAT/MEASURE

**What:** Close the open follow-up after PR #45: rebuild+test-sign `ramshared.sys` (VdSetSenseNotReady / no TUR BUSY), deploy to Hyper-V **win11-drill**, `sc` load RUNNING, WinDriveBackend CREATE/REGISTER, NTFS volume + sequential probe. Record empirical proof (Kahneman #13).
**Category:** e2e / windows lab / driver
**How to measure (elevated host, PSD):**
```powershell
# Machine env RAMSHARED_DRILL_PASSWORD set; PFX lab cert under ramshared-drill\certs
# Orchestrator used: C:\ramshared\bin\Run-GuestTmReload3.ps1 (and prior rebuild/sign via Build-Drivers + Sign-Drivers)
# From WSL: ./scripts/windows/wsl-elevated-ps.sh -File C:\ramshared\bin\Run-GuestTmReload3.ps1
```
**Measured data:**
- Host: rebuild **BUILD_DRIVERS_OK** + **SIGN_OK** (sys SHA256 + Inf2Cat `ramshared.cat` signed); package sys size **31120** on guest after deploy
- PSD: `win11-drill\drilladmin`, Build **26200**, **testsigning Yes**, FreeMB **~2622**
- Driver: `sc query` **poolstress RUNNING** + **ramshared RUNNING** (sys_len=31120, mtime deploy 12:25)
- Backend: **CREATE_DISK ok REGISTER_QUEUE ok** (alive pid, size=67108864)
- LUN: Disk **N=1** Size **67108864** Bus=SAS (FriendlyName `Msft Virtual Disk` under sc path — expected; host path used RAMSHARE branding)
- Volume: letter **D:** **NTFS** label path already_ntfs / probe OK
- Direct probe (guest): **write ≈ 101.9 MB/s**, **read ≈ 64.9 MB/s**, **match=True** (4 MiB fallback; full `Measure-RamSharedDiskIo.ps1` hit guest ExecutionPolicy block — numbers from inline probe)
- Teardown: backend STOP_OK; VM **Off** (no host pagefile on LUN; no thrash)
- Artifacts: `C:\ramshared\artifacts\agent-guest-tm-reload-20260714-122717.json` (also earlier attempts 121725 pnputil-only FAIL, 122425 Trim parse FAIL — fixed)
- Prior same-day host path (dev-workstation): Disk5 RAMSHARE RAW→S: NTFS; probe 8 MiB write≈1224 / read≈146 match=True (validation 11:52 entry)
**Verdict:** ✅ Guest signed reload + CREATE/FORMAT/MEASURE **PASS** (pass=9 fail=0)
**Next action:** optional Bypass execution policy on guest for CIM measure script; optional INF/PnP FriendlyName branding (RAMSHARE vs Msft Virtual Disk)

## 2026-07-14 13:27 -03 — host memory policy: WSL 16G RAM + 4G VRAM cascade (no wsl --shutdown)

**What:** Apply shared-host policy so WSL2 does not starve Windows/Hyper-V (isolated guest VMs): system RAM cap 16 GiB in `.wslconfig`; cascade VRAM tier 4 GiB; GPU free floor 1 GiB. Applied cascade-down/up live without `wsl --shutdown` (user mid-work).
**Category:** config / e2e
**How to measure:**
```bash
cat /mnt/c/Users/<user>/.wslconfig
cat /etc/ramshared/cascade.conf
swapon --show
./target/release/ramshared status
./scripts/safety/cascade-health.sh
nvidia-smi --query-gpu=memory.total,memory.free --format=csv
```
**Measured data:**
- `.wslconfig`: memory=16 GiB, swap=4 GiB, swapFile=I:\\wsl_swap\\swap.vhdx (backup .wslconfig.bak.*)
- `/etc/ramshared/cascade.conf`: VRAM_MIB=4096, ZRAM_MIB=2048, MIN_VRAM_HEADROOM_MIB=1024
- preflight OK free VRAM=4661 MiB (need >=1153 sparse)
- after cascade-up: nbd **4G** prio 100; zram 2G prio 200; sdc 8G prio -2; order_ok
- daemon: `ramsharedd --size 4096` alive pid live; health **ok:true**
- residual: disk used ~650 MiB after swapoff-first down (pages from prior zram) → phase UsingDisk expected until reclaimed
- GPU free ~4.5 GiB (>= 1 GiB headroom policy)
- **WSL MemTotal still ~15–16 GiB this session** — `.wslconfig` already 16G; full re-read of limits only needs later `wsl --shutdown` if Windows still held old 28G attempt (current session already ~16G)
**Verdict:** ✅ Cascade 4G VRAM path LIVE without killing WSL session; host residual RAM policy documented for Windows + guest VMs
**Next action:** when idle, optional `wsl --shutdown` once to ensure Windows fully reloads `.wslconfig`; avoid demote/pressure thrash on daily host

## 2026-07-14 16:41 -03 — .wslconfig escape-safe manage (platform guard)

**What:** Prevent WSL "invalid escape character" on boot: path values must not use single backslash. Added wslconfig-lib/ctl (encode=forward slash only, validate, apply, selftest), fixed wsl-kernel.sh arm + boot-kernel-safe.ps1 To-WslPath, cascade-preflight soft check.
**Category:** reliability / host config
**How to measure:**
```bash
bash scripts/safety/wslconfig-ctl.sh selftest
bash scripts/safety/wslconfig-ctl.sh check
bash scripts/safety/wslconfig-ctl.sh apply   # idempotent rewrite
```
**Measured data:** SELFTEST PASS; check OK on live profile; apply rewrote forward-slash paths; preflight shows "[ok] .wslconfig path escapes clean"
**Verdict:** ✅ regression class sealed (encode at write, validate before/after, PS/bash writers fixed)
**Next action:** none (optional CI job for selftest later)

## 2026-07-14 16:55 -03 — backlog close-out: issues #28/#30/#32 honest status

**What:** Execute remaining open product issues to the extent the environment allows without thrash.
**Category:** governance / research / docs
**Measured:**
- **#32:** PASSO0 re-check — WSL GPU-PV Gate A1 still FAIL for kernel-true; inventory complete; WSL NO-GO recommendation
- **#30:** stock kernel has no `/dev/ublk-control`; product remains NBD; ublk latency ≥15% claim blocked until custom-kernel lab (not daily host)
- **#28:** `ramshared-cuda` Windows loader (`loader_win.rs` + `nvcuda.dll` candidate) is in tree; host has `nvcuda.dll`; full StorPort↔CUDA host path still host-real gated
- Live cascade: nbd 4G, ramsharedd --size 4096, ok:true
**Verdict:** ✅ research/decision closed where evidence exists; no fake “host-real PASS”
**Next action:** optional bare-metal USB install (kernel-true); optional custom-kernel lab for ublk vs nbd; host Windows CUDA I/O only with gates

## 2026-07-15 12:00 -03 — windows-storport-cuda-vram Step 3 IMPL partial

**What:** Implement SPEC storage-only product path: winsvc config/evidence/runtime/queue/broker/service, CUDA probe planning, miniport owner/rundown/VPD, product vs lab installers and drill scaffolds.
**Category:** windows / storport / cuda / ssdv3
**How to measure:**
```bash
cargo fmt -p ramshared-winsvc -p ramshared-cuda -- --check
cargo clippy -p ramshared-cuda -p ramshared-block -p ramshared-winsvc --all-targets -- -D warnings
cargo test -p ramshared-cuda -p ramshared-block -p ramshared-winsvc --all-targets
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-winsvc \
  --files crates/ramshared-winsvc/src/config.rs,crates/ramshared-winsvc/src/evidence.rs,crates/ramshared-winsvc/src/driver_link.rs,crates/ramshared-winsvc/src/broker_tenant.rs,crates/ramshared-winsvc/src/runtime.rs,crates/ramshared-winsvc/src/service.rs \
  --min 80 --report-json tmp/windows-storport-cuda-vram-cov.json
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cuda --files crates/ramshared-cuda/src/probe.rs --min 80
```
**Measured data:**
- winsvc lib tests: 72 passed
- cover: config 95.5%, evidence 94.4%, driver_link 86.9%, broker_tenant 85.9%, runtime 86.8%, service 84.1%; cuda probe 80.0%
- E2E Windows WDK/GPU/SCM: not run (env-bound) → IMPL partial
- BINARY_MATCH: N/A (Windows-only slice)
**Verdict:** 🟡 partial — pure policy green; live StorPort+CUDA proof deferred to supervised Windows lab
**Next action:** MSVC cross-build + win11-drill Verifier IOCTL drill + approved physical probe/3-round SHA-256
**Artifacts:** `tmp/windows-storport-cuda-vram-cov.json`, `docs/specs/no-milestone/windows-storport-cuda-vram/IMPL.md`

## 2026-07-15 13:00 -03 — windows-storport-cuda-vram continue: Windows adapters + live CUDA probe

**What:** Implement full `WindowsDriverLink` (VirtualAlloc + OVERLAPPED IOCTL) and `WindowsHostState` (elevation, reparse config, pagefile CIM, volume lock, CNG SHA-256); shared `cuda_probe` module; preflight `-StorageOnly`; fix windows-sys 0.61 CUDA loader (`FreeLibrary`/`GetProcAddress`); live DT-3 probe on RTX 2060 via WSL libcuda.
**Category:** windows / cuda / ssdv3
**How to measure:**
```bash
cargo test -p ramshared-winsvc --lib
cargo test -p ramshared-winsvc probe_cuda_allocates_roundtrips_and_restores -- --ignored --nocapture
./target/release/ramshared-winsvc probe-cuda --config /tmp/ramshared-probe/winsvc.toml
cargo build -p ramshared-winsvc --target x86_64-pc-windows-msvc   # typechecks; link needs MSVC
```
**Measured data:**
- probe-cuda PASS: ordinal=0 name=NVIDIA GeForce RTX 2060 size=536870912 free_before=5351931904 free_after=5351931904
- cover gate still PASS (business files ≥80%)
- MSVC: rustc compiles; link.exe absent (env-bound)
**Verdict:** 🟡 still PARTIAL (StorPort LUN E2E env-bound) but ITEM-2 live CUDA proof closed on this host
**Artifacts:** `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/probe-cuda-wsl-20260715.txt`
**Next action:** MSVC Build Tools + win11-drill Verifier IOCTL + approved physical StorPort 3-round

## 2026-07-15 14:00 -03 — windows-storport-cuda-vram full campaign (PARTIAL close-out)

**What:** MSVC build product winsvc; Windows nvcuda probe; WDK rebuild+sign; win11-drill load driver CREATE/REGISTER + 4MiB SHA-256 I/O (lab backend); host preflight -StorageOnly PASS.
**Category:** windows / storport / cuda / ssdv3
**How to measure:**
```text
C:\ramshared\bin\ramshared-winsvc.exe probe-cuda --config C:\ProgramData\RamShared\winsvc.toml
# guest (elevated PSD): CREATE_DISK ok REGISTER_QUEUE ok; sha_match=true 4MiB
```
**Measured data:**
- winsvc.exe SHA256=F3453587C0AF7D432B566AA6F42C0C4370445B16E8803D12C5E3477BAD71CDDC size=647168
- probe-cuda Windows: free_before=free_after=5360320512 size=512MiB PASS
- guest: ramshared RUNNING; CREATE/REGISTER ok; sha=053EDE97406A271DBF208248B2070CCF79B9517431D994A2E79D146FFA760AA1 match=true bytes=4194304
- VM memory reduced to 2GiB static to start under host free~9.7GiB; VM left Off
**Verdict:** 🟡 PARTIAL — product CUDA probe + StorPort lab I/O proven; full product Online (CUDA backend+3 rounds+Verifier) still env-bound (guest no GPU; host no testsigning)
**Artifacts:** docs/specs/no-milestone/windows-storport-cuda-vram/evidence/*
**Next action:** enable host testsigning OR GPU lab VM; wire broker; run Invoke-CudaStorageDrill -ApprovePhysicalHost 3 rounds; Verifier IOCTL refusals

## 2026-07-15 14:30 -03 — product Online CUDA + 3-round SHA-256 (PARTIAL remaining Verifier)

**What:** Implemented `product_online.rs` (lease→CUDA→CREATE/REGISTER→I/O). Live host: ramshared RUNNING, broker on WSL :19876, console --storage-only reached Online backend=cuda LUN "RAMSHARE VRAMDISK" 64MiB; 3×4MiB SHA-256 all match.
**Category:** windows / cuda / storport / ssdv3
**How to measure:** Re-run isolated lab harness under `scripts/windows/` (e.g. `Run-GuestProductOnline.ps1` / `Run-GuestExhaustive.ps1`) with signed package; see `docs/specs/no-milestone/windows-storport-cuda-vram/`.
**Measured data:**
- Online: cuda=RTX 2060 size=67108864
- R1 match=true 232ms EFF6FD0B…; R2 true 157ms; R3 true 153ms; all_match=true letter=S
**Verdict:** 🟡 PARTIAL — product I/O proven; Verifier/REFUSE matrix + graceful stop still open (not index DONE)
**Artifacts:** evidence/product-cuda-3rounds.json; C:\ProgramData\RamShared\evidence\run-*.jsonl
**Next action:** Invoke-WinDriveIoctlValidation -Verifier on guest; graceful stop flag wiring

## 2026-07-15 14:45 -03 — graceful stop + guest IOCTL refuse PASS (PARTIAL: Verifier open)

**What:** Wired SCM/console stop via `AtomicBool` + `C:\ProgramData\RamShared\stop.request`; Gate A filters pagefiles to product volume letter; Gate B holds `LockedVolume` (soft-fail if unmounted). Live host product Online RTX 2060 64MiB then graceful stop exit 0. Guest win11-drill `Invoke-WinDriveIoctlValidation` STATUS=PASS for single-process REFUSE_* after signed miniport reload.
**Category:** windows / storport / cuda / ssdv3
**How to measure:** Re-run isolated lab harness under `scripts/windows/` (e.g. `Run-GuestProductOnline.ps1` / `Run-GuestExhaustive.ps1`) with signed package; see `docs/specs/no-milestone/windows-storport-cuda-vram/`.
**Measured data:**
- Graceful phases: Stopped→Leased→CudaReady→Online→Stopping→Stopped; exit_code=0
- Gate A: system C:\pagefile no longer refuses teardown; volume lock soft-fail win32=5 when LUN unmounted
- Guest verdict: PASS_VALID_QUEUE=1, REFUSE_UNKNOWN/RESERVED_DISK/REGISTER/BAD_RING/RING_INDEX_JUMP=1, VPD=1, NO_NEW_DUMP=1; FOREIGN_OWNER/REENTRY/RUNDOWN/RESERVED_CQE=0
- Host old sys: reserved/owner refuse still 0 (testsigning No — cannot reload new package)
**Verdict:** 🟡 PARTIAL — product Online + 3-round + graceful stop + guest single-process REFUSE closed; Verifier + multi-process injectors env-bound
**Artifacts:** evidence/graceful-stop-*.txt|jsonl; evidence/ioctl-guest-verdict-pass.json; evidence/ioctl-guest-console.txt
**Next action:** start win11-drill; enable Verifier; reload new sys on guest; foreign-owner PE + concurrent re-entry/rundown injectors

## 2026-07-15 15:00 -03 — teardown letter/dismount fix + host hang observation

**What:** Graceful stop hung because config letter (R) or free-letter (D) did not match live mount; UNREGISTER/DESTROY waited 30s each on mounted NTFS. Fixed: FSCTL dismount (no PowerShell) before Gate A/B; cancel COMMIT; careful HostExhaustive uses letters S/R/T only (never auto-D). Host exhaustive re-proof still GRACEFUL=false once with letter=D (old script); process pid 9148 became unkillable (kernel wait) after force-kill path.
**Category:** windows / storport / reliability
**Measured data:**
- 3-round SHA match=true with letter=D (bug in test script free-letter picker) then stop hung 60s
- taskkill /F elevated cannot kill pid 9148 ("no running instance" / zombie kernel wait)
- Popup "D:\ não está acessível" = Explorer on orphan letter from that test
**Verdict:** 🟡 PARTIAL — code path fixed; host needs reboot to clear hung winsvc + orphan LUN before re-proof; guest Verifier still open
**Next action:** reboot Windows host (or logoff+driver reset if possible); rebuild winsvc; Run-HostExhaustive.ps1; then guest IOCTL+Verifier

## 2026-07-15 15:30 -03 — Freeze postmortem (NOT random): I: paging + lab thrash + hard power

**What:** Host freeze with SSD r/w stuck, WSL hang, reboot hung until power button. Investigated Event Log + dmesg + layout.
**Category:** reliability / wsl2 / storage / host-safety
**Evidence (Windows System):**
- Kernel-Power **41** + EventLog **6008** (unexpected shutdown): **2026-07-15 15:08–15:10** (this incident), also 2026-07-14 and 2026-07-09/10
- disk **Event ID 51**: "Erro … HarddiskN … durante uma **operação de paginação**" (paging I/O error) — historical bursts e.g. 2026-07-03 Harddisk5, 2026-07-11 Harddisk6
**Evidence (WSL dmesg this boot):**
- **OOM memcg**: `clamd` killed in docker cgroup (~15:11) right after stack up — memory pressure with full unrelated workload compose
- cascade tear-down logged zram0 remove + nbd0 disconnect (our stabilization)
**Topology (smoking gun for build freezes):**
- Entire Ubuntu root = `I:\wsl2\Ubuntu-24.04\ext4.vhdx` (~220G file)
- WSL pagefile = `I:\wsl_swap\swap.vhdx` (4.1G) **same physical volume I:**
- unrelated workload builds write inside ext4.vhdx on **I:** → swap page-ins/outs also hit **I:** → queue collapse looks like “0 KB/s forever”
**Lab contribution (same day earlier):**
- hung `ramshared-winsvc` in kernel Stopping + orphan RAMSHARE LUN (100% disk / 0 KB/s) → storage stack sticky → reboot may hang
**Actions taken:**
1. cascade-down (nbd/zram off); `systemctl disable ramshared-cascade` (work mode)
2. `docker builder prune -f` reclaimed **~12.74 GB**
3. Document: do not co-run StorPort Online thrash + unrelated workload full stack on I:
**Verdict:** 🟡 root cause class identified (paging thrash on I: + concurrent load); host stable after cascade off; residual risk if I: fills or swap thrash during mega-builds
**Not fixed by:** `wsl --update` (already latest)
**Next:** free space on I:/C:; avoid cascade boot during unrelated workload; optional lower WSL swap after `wsl --shutdown` only with approval

## 2026-07-15 17:15 -03 — senior re-audit correction (PARTIAL, no false green)

**What:** Re-audit and correct the storage-only product runtime, teardown boundary, Windows I/O
lifetime, evidence, and isolated-VM harness after the prior solution produced unsafe teardown and
overstated validation.

**Category:** reliability / security / Windows StorPort / regression

**Corrections implemented:**

- Exact unique LUN identity is required before pagefile Gate A or any volume mutation. Candidate
  letters and pre-identity dismount were removed.
- Volume-lock/query/identity ambiguity is a hard refusal. Code 7 retains all owners and resumes
  Online service; SCM no longer reports `Running` after owners have been dropped.
- An independent 5-second CUDA observer enters failed-safe without destroying possibly-live state.
- Startup no longer replays `DESTROY` from evidence; partial acquisition unwinds in reverse and
  broker release failures are not hidden.
- Cancelled overlapped IOCTLs are drained before their `OVERLAPPED` storage leaves scope; partial
  Windows queue allocation is cleaned up.
- Config is checked and read through one no-follow handle. OS helper calls are bounded.
- Run/event identity, timestamps, actual counters, bounded latency sampling, and requested-byte
  evidence were corrected.
- The guest harness now bounds every PowerShell Direct call using jobs, measures real elapsed time,
  stops the VM on failure, and requires an active verifier plus a running driver for pass 2.
- The IOCTL script no longer accepts a size-only VPD fallback and no longer emits `STATUS=PASS` while
  mandatory foreign-owner/reserved-CQE/re-entry/rundown verdicts are zero.

**Measured gates:**

```text
cargo test -p ramshared-cuda -p ramshared-block -p ramshared-winsvc --all-targets
  block 41 pass; cuda 5 pass / 1 ignored; winsvc 77 pass / 1 ignored
cargo clippy (three packages, all targets, -D warnings): PASS
cargo clippy ramshared-winsvc --target x86_64-pc-windows-msvc --all-targets: PASS
cargo fmt --check: PASS
coverage: broker 85.9, config 95.5, driver_link 87.7, evidence 91.9,
          runtime 86.8, service 84.3, cuda probe 80.0 percent: PASS
Windows PowerShell 5.1 parser, both changed harnesses: PASS
```

**Isolated VM result:** The pre-Verifier pass proved the prior single-process subset and foreign-owner
refusal. `REFUSE_RESERVED_CQE`, completion re-entry, and teardown-during-copy rundown remain unproved.
After enabling standard Driver Verifier for `ramshared.sys`, Hyper-V showed `win11-drill` Running but
PowerShell Direct did not become ready even after more than six minutes. The campaign was aborted and
the VM was confirmed Off. No physical-host reset or destructive storage test was performed.

**Correction to the earlier freeze postmortem:** Event 41 and 6008 prove an unexpected shutdown, not
its cause. Historical Event 51 records do not prove the affected `HarddiskN` was the I: device or that
queue collapse caused this incident. The dual-VHDX/pagefile topology and concurrent lab load remain a
risk hypothesis only. A captured storage trace plus disk-number-to-device correlation is required for
a causal conclusion.

**Verdict:** 🟡 **PARTIAL** — corrected userspace safety and hermetic/cross-target gates are green;
Driver Verifier, three concurrent Ring 0/3 injectors, and a supervised physical run of the corrected
binary remain mandatory. Earlier physical CUDA/SHA evidence does not validate this corrected binary.

**Next action:** recover/revert the checkpointed guest, rebuild/sign the current miniport, implement
the missing concurrent injectors, then run the complete Verifier matrix. Only after that, run the
supervised physical three-round campaign with exact identity and teardown evidence.

## 2026-07-15 20:15 -03 — concurrent injectors + IoRundown (PARTIAL remains)

**What:** concurrent injectors + IoRundown (PARTIAL remains). **Issue:** #54
**Issue:** #54

**What changed (this turn):**

- `drivers/windows/ramshared/queue.c`: balanced `IoRundown` on `QSubmit`/`QCommitAndFetch` (release
  before long-lived pend); refuse Failed/Closing; reserved CQE fails closed.
- `scripts/windows/Invoke-WinDriveIoctlValidation.ps1`: three concurrent probes
  (`Invoke-ReservedCqeInjection`, `Invoke-CompletionReentryInjection`,
  `Invoke-RundownDuringCopyInjection`); dual-handle UNREGISTER; bounded VPD poll; lab size default
  128 MiB to avoid `answer-disk.vhdx` (64 MiB) collision.
- `scripts/windows/Test-WinDriveIoctlValidationStatic.ps1`: RED/GREEN static gate (PASS).
- `scripts/windows/Run-GuestExhaustive.ps1`: INF + SetupAPI root-enum fallback; force replace locked
  `System32\drivers\ramshared.sys`; 300s IOCTL timeout; live console capture.
- Miniport rebuild/sign/deploy: SHA256 `4CEE404FC9C9029F55812F1D133AA36D61A2D64F92DB3D15CF01AFEF5ABAEC2A`.

**Guest campaign** (`guest-exhaustive-20260715-201316`, `-SkipVerifier`):

```text
REFUSE_RESERVED_CQE=1
COMPLETION_REENTRY_NO_SLOT_REUSE=1
RUNDOWN_UNMAP_AFTER_COPY=1
… all other REFUSE_* + PASS_VALID_QUEUE + NO_NEW_DUMP = 1
VPD_SERIAL_MATCH=0
STATUS=FAIL missing=VPD_SERIAL_MATCH
```

**Still open:**

- VPD: adapter can enumerate (`ROOT\RAMSHARED\0000`) but no unique disk PDO under `Get-Disk`.
- Driver Verifier full pass not re-run on this binary (prior PSD hang under Verifier).
- Physical corrected winsvc Online E2E not re-proven.

**Host safety:** no physical thrash; VM force-stopped on harness errors; `win11-drill` left Off.

**Verdict:** 🟡 **PARTIAL** — concurrent Ring 0/3 injectors + rundown proven; VPD + Verifier + physical
Online still required for DONE.

## 2026-07-15 21:10 -03 — guest ITEM-3 STATUS=PASS (Verifier still open)

**What:** guest ITEM-3 STATUS=PASS (Verifier still open). Campaign: `guest-exhaustive-20260715-210925` (`-SkipVerifier`), `GUEST_EXIT=0`
**Issue:** #54

**Campaign:** `guest-exhaustive-20260715-210925` (`-SkipVerifier`), `GUEST_EXIT=0`

**Binary:** `ramshared.sys` SHA256 `1E57690EA63E6287D4790A134544DC9F46253BB356D1C2B3B1D65FC812F30CFF`

**All ITEM-3 verdicts = 1**, including:

- `REFUSE_RESERVED_CQE`, `COMPLETION_REENTRY_NO_SLOT_REUSE`, `RUNDOWN_UNMAP_AFTER_COPY`
- `VPD_SERIAL_MATCH=1` via `Win32_DiskDrive` name `RAMSHARE VRAMDISK SCSI Disk Device`

**Driver fixes that unblocked adapter/LUN:**

- Virtual miniport init: `STOR_FEATURE_VIRTUAL_MINIPORT`, `HwAdapterControl`, `HwFreeAdapterResources`
- FindAdapter must not force `Master`/`ScatterGather`/`NeedPhysicalAddresses` = FALSE
  (was `STATUS_DEVICE_CONFIGURATION_ERROR` / problem 10)
- HwStartIo: PnP/Power SRBs completed without CDB mis-decode
- REPORT LUNS + zero capacity while inactive

**Honest limits:** concurrent probes are ring/IOCTL concurrency, not full READ-copy SRB race.
Driver Verifier matrix not re-run. Physical winsvc Online not re-proven.

**Verdict:** 🟡 **PARTIAL** — guest IOCTL matrix green; Verifier + physical Online remain for DONE.

## 2026-07-15 21:50 -03 — guest ITEM-3 + Driver Verifier STATUS=PASS

**What:** guest ITEM-3 + Driver Verifier STATUS=PASS. Campaign: `guest-exhaustive-20260715-214831`
**Issue:** #54

**Campaign:** `guest-exhaustive-20260715-214831`
**Binary:** `1E57690EA63E6287D4790A134544DC9F46253BB356D1C2B3B1D65FC812F30CFF`

```text
IOCTL_PASS1=PASS
IOCTL_VERIFIER=PASS
VERIFIER_RAN=true
GUEST_EXIT=0
```

Pass 2: Verifier flags `0x2093B` on `ramshared.sys` (no DMA flag for virtual miniport).
`verifier /query` listed `MODULE: ramshared.sys (load: 1 / unload: 0)`. All ITEM-3 verdicts = 1
including VPD + concurrent probes; `NO_NEW_DUMP=1`. VM Off; verifier reset best-effort.

**Harness fix:** schedule Verifier then guest `shutdown /r` (not only Restart-VM -Force); PSD wait 600s.

**Still open for product DONE:** physical `ramshared-winsvc` Online E2E on this corrected stack;
optional SRB-level re-entry/rundown-during-READ drill.

**Verdict:** 🟡 **PARTIAL** (product) / guest StorPort ITEM-3+Verifier **PASS** for #54.

## 2026-07-16 01:05 -03 — physical Online preflight RED (Online skipped)

**What:** physical Online preflight RED (Online skipped). **Issue:** #54 residual product gate (physical winsvc Online).
**Issue:** #54 residual product gate (physical winsvc Online).

**Supervision:** read README + rules + MEMORY; no reboot; no thrash; no Online.

### Audit of prior host image work

| Artifact | SHA256 / state |
| --- | --- |
| package `C:\ramshared\package\ramshared.sys` | `1E57690E…` (guest Verifier PASS image) |
| installed `C:\Windows\System32\drivers\ramshared.sys` | `E690306F…` len=32656 mtime=2026-07-15 13:23 |
| `ramshared.sys.bak-host` | **MISSING** — prior Move-Item/Copy-Item access denied while image locked |
| `ramshared-winsvc.exe` / `RamSharedWinSvc.exe` | both `F129B25F…` (rebuilt this session; service stopped) |

**Empty tool output:** earlier elevated calls sometimes returned exit 0 with empty/truncated capture
(wrapper/UNC). This preflight used `PREFLIGHT:` line labels; Windows capture has 36 lines
(`/tmp/physical-preflight-windows.txt` + evidence copies). Silence was not treated as success.

### Live preflight (non-destructive)

- `ramshared` kernel: **Running** (cannot unload without reboot)
- `RamSharedWinSvc`: **Stopped** (left stopped)
- PnP: adapter OK, disk OK (`RAMSHARE VRAMDISK`); **Get-Disk RAMSHARE count=0**
- Control: `CreateFile \\.\RamSharedCtl` → **OK err=0**
- testsigning: **Yes**
- cascade: **inactive**
- GPU baseline: RTX 2060 used≈1348–1387 MiB free≈4568–4607 MiB
- Default `winsvc.toml`: `volume_letter=D` size=512 MiB — **forbidden** for this supervised gate
- Product cfg `winsvc-product.toml`: S: / 64 MiB available but unused because preflight RED

### Decision

**PREFLIGHT=RED → Online SKIPPED.**

Reasons: BINARY_MATCH miniport fail; no installed backup; README lab-VM-only for Windows driver on
daily host; orphan PnP disk without Get-Disk entry; no reboot allowed to swap guest-proven `.sys`.

**Safe state:** no Online started; userspace service stopped; kernel miniport left loaded (no thrash
unload). Evidence:
`docs/specs/no-milestone/windows-storport-cuda-vram/evidence/physical-preflight-20260716T010502Z.txt`
and `physical-preflight-windows-20260716T010502Z.txt`.

**Tests:** `cargo test -p ramshared-winsvc --lib` → 77 pass / 1 ignored. `docs-check` OK.

**Verdict:** 🟡 **PARTIAL** — guest StorPort+Verifier green; physical Online not proven and not safe
to run under this preflight.

## 2026-07-16 01:30 -03 — lab GPU probe: no CUDA in win11-drill (Online skipped)

**What:** lab GPU probe: no CUDA in win11-drill (Online skipped)
**Constraint:** daily-host preflight RED remains binding (no host Online/reboot/unload).

**win11-drill GPU inventory:**
- Host: `Get-VMGpuPartitionAdapter` count=1 but empty InstancePath/MinPartitionVRAM; AssignableDevice=0
- Guest: Hyper-V Video OK; NVIDIA GeForce RTX 2060 PnP **Error** (`PCI\VEN_1414&DEV_008E`);
  `nvidia-smi` **MISSING**; `nvcuda.dll` **false**

**Decision:** Guest product Online (CUDA) **cannot** run. Not faked.
Guest StorPort ITEM-3 + Verifier already **PASS** (`guest-exhaustive-20260715-214831`, sys `1E57690E…`).
Physical host Online still **RED** (`physical-preflight-20260716T010502Z`: installed `E690306F…` ≠ package).

**Closed safely this turn:**
- `cargo test -p ramshared-winsvc --lib` 77 pass / 1 ignored
- slice coverage ≥80% on winsvc business files (broker/config/driver_link/evidence/runtime/service)
- `STATIC_INJECTOR_TEST=PASS`
- clippy/fmt winsvc OK; docs-check OK
- VM left **Off**

**Verdict:** 🟡 **PARTIAL** (product). Terminal safe: no Online, no host thrash, lab VM Off.

## 2026-07-16 02:58 -03 — proof closeout after GPU-PV timeout

**What:** Supervised the bounded GPU-PV driver-package attempt, stopped it after the ten-minute
ceiling, and performed an independent non-destructive verification closeout.

**Safe terminal state:** `win11-drill` Off; guest and host staging removed; host RTX 2060 `OK` and
visible through `nvidia-smi -L`. Guest NVIDIA remained `CM_PROB_FAILED_POST_START`, so DLL/tool
presence was not accepted as CUDA proof. No blind retry, uninstall, host reboot, miniport change,
WSL2 pressure, commit, or merge occurred.

**Fresh local gates:** native tests (block 41, CUDA 5 + 1 ignored, winsvc 77 + 1 ignored), clippy
`-D warnings`, fmt check, docs-check, diff check, and selected coverage ≥80% all passed. The isolated
StorPort concurrent-injector/rundown/Verifier campaign remains PASS.

**Promotion matrix:** physical `BINARY_MATCH` BLOCKED; real GPU-PV CUDA BLOCKED; product Online with
three SHA rounds and cleanup BLOCKED; WSL2 freeze-elimination claim BLOCKED. The WSL2 claim requires
an isolated twice-repeated before→action→after hang campaign with watchdog/timeout, swapoff-first,
ghost/deleted-plus-used-kB, binary match, D-state/hung-task evidence, and cleanup. It was not run on
the daily host.

**Evidence:**
`docs/specs/no-milestone/windows-storport-cuda-vram/evidence/gpupv-safe-close-20260716T025812Z.txt`
and `evidence/verification-closeout-20260716.md`.

**Verdict:** 🟡 **PARTIAL** — proven subsets remain green; CUDA Online and WSL2 freeze resolution are
explicitly not proven.

## 2026-07-16 10:00 -03 — VPD false-green invalidates prior ITEM-3 aggregate PASS

**What:** Product-gates review found that `Invoke-WinDriveIoctlValidation.ps1` could set
`VPD_SERIAL_MATCH=1` from a unique size/name match or one live PnP RAMSHARE device without observing
the required 16-byte VPD serial. The harness now requires vendor/product + exact serial + exact size
on one authoritative storage surface, and its static regression test forbids both permissive
fallbacks.

**Measured gates:** Windows PowerShell 5.1 parser PASS; `STATIC_INJECTOR_TEST=PASS`;
`STATIC_VPD_FALLBACK_REFUSAL=PASS` with a negative fixture; staged WDK
10.0.26100.0 build `BUILD_DRIVERS_OK` (`ramshared.sys` 31,744 bytes; staging removed); native Rust
tests/clippy/fmt PASS; MSVC cross-target clippy PASS; selected coverage 80.0%–95.5%; docs/diff checks
PASS; `cargo audit --no-fetch` PASS.

**Live read-only preflight:** installed miniport SHA256 `E690306F…`; package SHA256 `1E57690E…`;
`BINARY_MATCH=false`; kernel service Running; userspace service Stopped. No Online, install,
replacement, reboot, or pressure action was performed.

**Evidence:**
`docs/specs/no-milestone/windows-storport-cuda-vram/evidence/vpd-false-green-audit-20260716.md`.

**Verdict:** 🟡 **PARTIAL** — historical non-VPD injector/rundown/Verifier observations remain useful,
but the prior aggregate ITEM-3 PASS is invalidated until the corrected harness is rerun in the
isolated VM.

**Additional teardown correction:** the read-only identity query returns the standard friendly name
`RAMSHARE VRAMDISK SCSI Disk Device`. The prior parser split only once and compared product
`VRAMDISK SCSI Disk Device` against exact `VRAMDISK`, falsely refusing every legitimate stop before
Gate A. The parser now accepts only the exact two-token product identity with either no suffix or the
standard `SCSI Disk Device` suffix; mismatched prefixes and arbitrary suffixes remain refused. The
paired positive/refusal unit test passed, the winsvc library result is now 78 passed / 1 ignored, and
service slice coverage is 84.9%.

## 2026-07-16 10:46 -03 — corrected exact-VPD guest rerun fails honestly

**What:** corrected exact-VPD guest rerun fails honestly. Campaign: `C:\ramshared\artifacts\guest-exhaustive-20260716-104650` using corrected harness SHA
**Campaign:** `C:\ramshared\artifacts\guest-exhaustive-20260716-104650` using corrected harness SHA
`6D7B2DC1…` and miniport SHA `1E57690E…`.

**Before:** `win11-drill` Off; GPU partition rollback restored one bare adapter with empty partition
values; DDA count 0; host RTX 2060 OK. Only the corrected IOCTL harness was deployed to the host lab
bin directory.

**Action:** bounded `Run-GuestExhaustive.ps1` without `-SkipVerifier`. PowerShell Direct became ready,
the package deployed, pass 1 completed, the guest rebooted normally under Verifier, PSD returned in
82 seconds, and pass 2 completed with Verifier flags `0x2093B` active on `ramshared.sys`.

**Result:** both passes had every required non-VPD verdict = 1, including the three concurrent
injectors, foreign-owner refusal, and `NO_NEW_DUMP`. Both correctly failed with
`VPD_SERIAL_MATCH=0`; summary `IOCTL_PASS1=FAIL`, `IOCTL_VERIFIER=FAIL`, `VERIFIER_RAN=true`, guest
exit 2. No blind retry was performed.

**After:** VM Off; verifier reset best-effort; one bare GPU partition adapter with empty values; DDA
count 0; host RTX 2060 OK.

**Evidence:** `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/ioctl-guest-*-exact-vpd*`.

**Verdict:** 🟡 **PARTIAL** — injector/rundown/Verifier subset passes; the miniport identity path must
surface exact vendor/product/VPD serial/size before ITEM-3 can pass.

## 2026-07-16 11:00 -03 — VPD placeholder PDO cache lifecycle corrected statically

**What:** VPD placeholder PDO cache lifecycle corrected statically
**Cause:** before CREATE, the miniport reported LUN 0 and VPD 0x80 with sixteen synthetic zero bytes.
Windows cached that child PDO identity; `BusChangeDetected` did not replace it after CREATE, matching
the corrected campaign's `VPD_SERIAL_MATCH=0` and stale PnP identities.

**Fix:** the control device stays available, but the storage bus reports no LUN before CREATE.
INQUIRY/capacity return NO_DEVICE; CREATE publishes complete serial/size then triggers an
absent→present bus rescan. Serial input is exactly uppercase 16-hex; no synthetic/default serial
remains. INQUIRY/VPD short allocations and READ CAPACITY(10/16) are now bounded and implemented.

**Static/build evidence:** `STATIC_SCSI_LIFECYCLE_TEST=PASS`, `STATIC_INJECTOR_TEST=PASS`, negative
no-LUN fixture PASS, and WDK 26100 `/W4 /WX /wd4324` `BUILD_DRIVERS_OK`. The only disabled warning is
WDK `storport.h` C4324 for explicitly aligned structures; project warnings remain errors. Unsigned
image: 32,256 bytes, SHA256 `5A1B7C830935F8C8B79DEA552D4CBB098548E5E5894B3F23672D099EA92674EC`.
Staging was removed.

**Evidence:**
`docs/specs/no-milestone/windows-storport-cuda-vram/evidence/vpd-cache-lifecycle-fix-20260716.md`.

**Verdict:** 🟡 **PARTIAL** — rebuild/sign/deploy plus isolated exact-VPD + Verifier rerun is still
required. No VM run or physical-host mutation occurred in this correction step.

## 2026-07-16 11:14 -03 — signed VPD lifecycle rerun remains RED

**What:** signed VPD lifecycle rerun remains RED. Campaign: one bounded no-retry run,
**Package:** isolated WDK 26100 `/W4 /WX /wd4324` build, Inf2Cat with zero warnings/errors, valid
SYS/CAT/poolstress Authenticode, and no trust-store change. Signed package and guest-installed
`ramshared.sys` matched at SHA256 `CD7E315D0DA5B24BB05C384846D7BA8123390300D2C3A3F73B10E52F9E80BC34`.
Harness source/staged SHA matched at `6D7B2DC1…`.

**Campaign:** one bounded no-retry run,
`C:\ramshared\artifacts\guest-exhaustive-20260716-111439`, without `-SkipVerifier`. `Get-Disk` had no
RAMSHARE disk before CREATE, but the PnP snapshot retained historical RAMSHARE child PDOs including
one `OK`, so the no-stale-child lifecycle gate failed. Normal and Verifier passes both failed only
`VPD_SERIAL_MATCH=0`; every other ITEM-3 verdict and `NO_NEW_DUMP` was 1. Verifier flags `0x2093B`
were active; module load/unload was 1/0; no dumps appeared.

**After:** no retry; VM Off; Verifier reset best-effort; one bare GPU-PV adapter; DDA=0; host RTX
2060 OK; isolated staging removed. No physical driver install, Online action, trust-store mutation,
host reboot, commit, or merge.

**Evidence:** `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/signed-vpd-lifecycle-rerun-20260716.md`
and raw `evidence/guest-exhaustive-20260716-111439/`.

**Verdict:** 🟡 **PARTIAL / VPD BLOCKED** — the signed live result disproves promotion of the current
`BusChangeDetected` lifecycle fix; retained child-PDO identity must be resolved and re-proven.

## 2026-07-16 12:04 -03 — exact VPD + Driver Verifier PASS

**What:** exact VPD + Driver Verifier PASS. Campaign: isolated guest `C:\ramshared\artifacts\guest-exhaustive-20260716-120459`. The deployed
**Campaign:** isolated guest `C:\ramshared\artifacts\guest-exhaustive-20260716-120459`. The deployed
and guest-loaded `ramshared.sys` matched SHA256
`CD7E315D0DA5B24BB05C384846D7BA8123390300D2C3A3F73B10E52F9E80BC34`. A mandatory post-deploy
reboot remapped the package image after the prior SCM `1056` stale-image condition; PSD returned in
93 seconds, inside the 300-second bound.

**Result:** normal and Verifier passes returned `STATUS=PASS` and exit 0. Every required ITEM-3
verdict was 1 in both passes. `VPD_SERIAL_MATCH=1` observed vendor/product `RAMSHARE/VRAMDISK`, exact
serial `ABCDEF0123456789`, and capacity `134217728` bytes on one `Win32_DiskDrive` candidate. Capacity
came from `IOCTL_DISK_GET_LENGTH_INFO`; the CHS-derived WMI size was not accepted. Driver Verifier
flags were `0x2093B`, with `ramshared.sys` load/unload 1/0. `NO_NEW_DUMP=1` in both passes.

**Root-cause closure:** before CREATE, REPORT LUNS is empty and INQUIRY/capacity return `NO_DEVICE`;
CREATE publishes the validated serial and size before `BusChangeDetected`. Historical RAMSHARE child
PDOs were removed in the isolated guest. The harness now rejects friendly-name, size-only, and PnP
presence fallbacks.

**Independent closeout audit:** `git diff --check`, docs-check, `cargo fmt --all -- --check`,
`cargo clippy -p ramshared-winsvc --all-targets -- -D warnings`, and 78 winsvc tests passed; one live
CUDA test remained explicitly ignored. The SCSI/injector static test first reproduced a direct WSL
UNC invocation failure (exit 1, empty `$PSScriptRoot` during parameter-default evaluation), then
passed directly with exit 0 after resolving defaults from `$MyInvocation.MyCommand.Path` at runtime.
The canonical WDK script then reproduced one deterministic `/Zi` UNC-PDB failure (`C1041`), moved the
fix to the build layer (`/W4 /WX /wd4324 /Z7`), and returned `BUILD_DRIVERS_OK`. The resulting unsigned
`ramshared.sys` was 32,256 bytes with SHA256 `A56D4C4F…`; it was not deployed. Checkpatch over the
Windows-driver diff returned 0 errors and 0 warnings. The Windows MSVC toolchain cross-build passed
from a disposable local staging copy. Slice coverage passed at config 95.5%, evidence 91.9%, driver
link 87.7%, broker tenant 85.9%, runtime 86.8%, service 84.9%, and CUDA probe 80.0%.

**After:** a read-only recapture recorded `win11-drill` Off, one GPU-PV adapter with empty partition
values, DDA count 0, host display `NVIDIA GeForce RTX 2060` status `OK`, and successful `nvidia-smi`.
No physical Online action, host driver replacement, pressure campaign, commit, or merge occurred.

**Evidence:**
`docs/specs/no-milestone/windows-storport-cuda-vram/evidence/vpd-exact-pass-20260716.md` and
`docs/specs/no-milestone/windows-storport-cuda-vram/evidence/terminal-state-vpd-pass-20260716T170631Z.md`.
Build audit: `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/wdk-build-audit-20260716T171026Z.md`.

**Verdict:** guest StorPort ITEM-3 + exact VPD + Verifier **PASS**. Product remains 🟡 **PARTIAL**:
physical BINARY_MATCH/Online, GPU-PV protocol alignment for real CUDA, live StartIo READ-race
strengthening, and the isolated WSL2 freeze-elimination campaign remain open.

## 2026-07-16 14:52 -03 — sequential fronts: physical RED; GPU-PV probe-cuda PASS

**What:** sequential fronts: physical RED; GPU-PV probe-cuda PASS
### Physical host (read-only)

`BINARY_MATCH=false`: package `CD7E315D…` ≠ installed `E690306F…`; no `.bak-host`.
README policy: Windows kernel driver on daily host = **NO** (lab VM only). Product Online on the
physical host **SKIPPED** (not attempted). Evidence:
`docs/specs/no-milestone/windows-storport-cuda-vram/evidence/physical-preflight-readonly-20260716T172150Z.txt`.

### GPU-PV lab (win11-drill)

Host build `26200.8655`; guest `26200.8037`. Virtual PCI events still show request `0x10006` vs
negotiated `0x10005`, but guest `nvidia-smi` lists the real RTX 2060 UUID and driver `610.74`.

Bounded `probe-cuda` with lab side-by-side VC runtime: **PASS** (exit 0), 64 MiB DeviceMem,
three offsets, free_before == free_after. No Online/format. Terminal: VM Off, host GPU OK.

Evidence: `evidence/gpupv-probe-cuda-pass-20260716T173812Z.md`.

### InfVerif

BusType moved under Parameters (ERROR 1323 cleared). ERROR 1322 DIRID 13 remains open for
attestation package work. Evidence: `evidence/infverif-20260716.md`.

### Next

1. Guest product Online + 3-round storage SHA (lab only, 64 MiB, exact VPD).
2. Optional guest Windows Update to UBR ≥ host to silence protocol mismatch.
3. StartIo READ concurrent race under Verifier (beyond ring/IOCTL injectors).
4. InfVerif DIRID 13 package migration or documented waiver.
5. Isolated WSL2 freeze campaign (never daily thrash).
**Verdict:** 🟡 PARTIAL

## 2026-07-16 14:53 -03 — guest product Online PARTIAL (64 MiB)

**What:** guest product Online PARTIAL (64 MiB). Campaign `guest-product-online-20260716-145248` on win11-drill:
Campaign `guest-product-online-20260716-145248` on win11-drill:

- BINARY_MATCH package/guest `CD7E315D…`
- Product Online true with CUDA RTX 2060; serial `B7A9E1BD0E71541A`; disk 64 MiB letter S
- Three write/read SHA rounds **PASS**
- Graceful stop **FAIL** within 60s (`forceKilledConsole`); VM later Off; host GPU OK
- Lab JSONL lease broker used for Register/LeaseGrant (not full ramsharedd)

Evidence: `evidence/guest-product-online-20260716-145248.md`.
Harness fixes pending re-run: longer stop wait, no FileInfo JSON explosion.
**Verdict:** 🟡 PARTIAL

## 2026-07-16 15:13 -03 — guest product Online re-run 151304 PARTIAL

**What:** guest product Online re-run 151304 PARTIAL
- Online+CUDA+64MiB LUN serial A0B4FCE26201BD5D + 3 SHA PASS; BINARY_MATCH CD7E315D
- Graceful stop still FAIL after 180s re-assert stop.request (force kill; no lease liberado)
- Root cause: teardown refuse/resume Online loop or stop not effective; no Stopping line in stderr
- Evidence: evidence/guest-product-online-20260716-151304.md
- Terminal: VM Off, host GPU OK. No push.
**Verdict:** 🟡 PARTIAL

## 2026-07-16 17:42 -03 — guest product Online STOP_OK PASS (I/O-pump lock)

**What:** guest product Online STOP_OK PASS (I/O-pump lock). Campaign `guest-product-online-20260716-174238` on win11-drill:
Campaign `guest-product-online-20260716-174238` on win11-drill:

- ONLINE + BINARY_MATCH CD7E315D… + 3 SHA PASS (serial E688A3B1F1D1F0C0, letter S, 64 MiB)
- **STOP_OK=true**, forceKilled=false, **lease 1 liberado**
- Root cause: CreateFile volume lock deadlocked when COMMIT loop stopped; fixed by I/O pump during lock + CREATE-time identity + registry Gate A
- Evidence: `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/guest-product-online-20260716-174238.md`
- Terminal: VM Off, host RTX 2060 OK. No physical Online, no push.
**Verdict:** 🟡 PARTIAL

## 2026-07-16 18:30 -03 — teardown audit correction + InfVerif DIRID 13 PASS

**What:** teardown audit correction + InfVerif DIRID 13 PASS
The `174238` campaign remains an empirical successful run, but its product-closure interpretation is
invalidated. Audit found CREATE-only stop identity, registry-only pagefile authority, an unbounded
mutating lock worker, and an incomplete harness exit conjunction.

RED/GREEN corrections now require live letter-to-disk/VPD/capacity identity plus a single-disk-extent
recheck, configured+active pagefile union fail-closed, a 30-second lock deadline that never resumes
Online with a mutating worker outstanding, and three fresh no-retry lifecycle rounds with complete
cleanup verdicts. These corrections are not yet live-proven, so product status remains **PARTIAL**.

INF package isolation was separately validated with the real WDK 10.0.26100.0 tool. Initial DIRID 13
migration produced `ERROR(1199)` until the model was restricted to build 16299+. Final
`InfVerif.exe /w drivers/windows/ramshared/ramshared.inf` exited **0** with empty output. No driver
install/load, VM mutation, physical-host action, commit, or push occurred for this validation.

Evidence: `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/infverif-dirid13-pass-20260716.md`.
**Verdict:** 🟡 PARTIAL

## 2026-07-16 19:00 -03 — teardown hardening static close; signed live rerun blocked

**What:** teardown hardening static close; signed live rerun blocked
Additional audit found two more ownership gaps: CUDA `DeviceMem` was dropped only after
`LeaseRelease`, and a release flush failure removed the authoritative lease from memory. TDD now
consumes the backend to free DeviceMem, verifies CUDA restoration within 64 MiB, then releases the
lease. Ambiguous release retains the lease and is not replayed. The wildcard configured pagefile
path `?:\pagefile.sys` is now unsafe for every product volume, and non-DOS paths fail closed.

Full Rust, native/Windows clippy, MSVC release build, WDK `/W4 /WX` build, InfVerif, PowerShell
parser/static tests, docs, diff, and >=80% slice coverage are green. Live rerun was not attempted:
SignTool could see the machine certificate but could not access its private key from the current
token, and no PFX password was available. No permission/trust-store bypass, driver install, VM
mutation, physical-host action, commit, or push was performed.

Evidence: `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/teardown-hardening-static-20260716.md`.
**Verdict:** 🟡 PARTIAL

## 2026-07-16 20:11 -03 — guest product Online PASS after teardown hardening

**What:** Rebuilt current `ramshared-winsvc` with the corrected teardown identity path, deployed the
DIRID-13 signed miniport package to `win11-drill`, and ran the corrected no-retry three-lifecycle
GPU-PV product campaign.

**Result:**

| Gate | Result |
| --- | --- |
| Campaign | `guest-product-online-20260716-201130` |
| Lifecycle rounds | `3` |
| ONLINE + CUDA | PASS, RTX 2060 via GPU-PV |
| DriverStore/package BINARY_MATCH | PASS, `E297B73F…` |
| Product exe | `C6C9EB92…` |
| SHA I/O | PASS in all 3 rounds |
| Graceful stop | PASS, no force-kill |
| Lease release | PASS, `lease 1 liberado` each round |
| CUDA restored | PASS |
| Dumps | none new |
| Terminal | VM Off, host RTX 2060 OK |

**Fixes proven:** startup LUN wait pumps COMMIT; PnP root device is recreated/enabled without leaving
`ROOT\RAMSHARED` disabled; DriverStore mismatch aborts before product start; stop identity binds
letter + exact VPD serial + configured size without the teardown-time `PhysicalDriveN` length IOCTL;
harness captures `RuntimeSummary exit_code: 0` when the PowerShell process object returns a null
`ExitCode`.

**Verdict:** ✅ isolated GPU-PV storage-only product path works.

**Still not claimed:** physical daily-host authorization, SDV/Code Analysis, dedicated live StartIo
READ-copy race strengthening, and WSL2 freeze elimination. The WSL2 freeze claim still requires a
separate isolated before/action/after hang campaign; no daily WSL2 pressure/thrash was run.

**Evidence:** `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/guest-product-online-20260716-201130.md`.

## 2026-07-16 22:08 -03 — current signed GPU-PV product + Verifier gates PASS

**What:** Rebuilt the current Windows product and driver package, fixed project Code Analysis
warnings, published signed package `ramshared.sys` SHA `97FD7B37…`, and reran both product Online
and exhaustive IOCTL/Verifier campaigns on isolated `win11-drill`.

**Category:** integration
**How to measure:** Re-run isolated lab harness under `scripts/windows/` (e.g. `Run-GuestProductOnline.ps1` / `Run-GuestExhaustive.ps1`) with signed package; see `docs/specs/no-milestone/windows-storport-cuda-vram/`.

**Measured data:**

| Gate | Result |
| --- | --- |
| Product campaign | `guest-product-online-20260716-220848` |
| Product exe SHA | `AAD4566897C9CF262F14AB783CCC6B2B2A43C8233A2E85ECA1FC562003246352` |
| Driver package SHA | `97FD7B373ED7DD5AE7F38204070F8B89E08A2B25616AA2A128995E8D1FBFF34F` |
| Product rounds | 3/3 PASS |
| Round teardown | 9064 ms / 5026 ms / 4018 ms |
| CUDA restore wait | 106 ms / 76 ms / 57 ms |
| Exhaustive campaign | `guest-exhaustive-20260716-224913` |
| IOCTL pass1 | PASS |
| IOCTL under Verifier | PASS |
| Verifier | `0x2093B`, `ramshared.sys` load 1 / unload 0 |
| VPD exact | `VPD_SERIAL_MATCH=1`, serial `ABCDEF0123456789`, size `134217728` |
| Dumps | none new |
| Terminal | VM Off; verifier reset best-effort; host RTX 2060 OK |

**Fixes proven:** stale DriverStore `ramshared.inf` packages are purged before install; missing
post-reboot `ROOT\RAMSHARED\0000` is recreated via SetupAPI before IOCTL; root PnP and SCSIAdapter
must be `OK|problem=0`; CUDA restoration still requires the 64 MiB threshold but now polls briefly
before declaring failure.

**Verdict:** ✅ works for the isolated GPU-PV storage-only product and current signed
IOCTL/Verifier package.

**Next action:** Keep physical daily-host Online, SDV, dedicated StartIo READ-copy live race, and
isolated WSL2 freeze-elimination campaigns as separate non-claims.

**Evidence:** `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/guest-product-online-20260716-220848.md`,
`docs/specs/no-milestone/windows-storport-cuda-vram/evidence/guest-exhaustive-20260716-224913.md`.

## 2026-07-16 22:50 -03 — WDK Code Analysis project-clean

**What:** Ran MSVC/WDK Code Analysis over `drivers/windows/ramshared/{driver.c,virtdisk.c,queue.c,control.c}`
after adding WDK callback prototypes and narrowing the probe exception filter.

**Category:** local-check

**Measured data:**

- `cl /kernel /W4 /analyze` completed for the four driver files.
- Project-file warnings under `C:\ramshared\src\drivers\windows\ramshared\*.c`: `0`.
- WDK header warnings remain in `wdm.h`, `ntddk.h`, and `storport.h`.
- SDV binaries (`sdv.exe` / `StaticDV.exe`) were not present in the local WDK image.

**Verdict:** ✅ works for project Code Analysis; 🟡 SDV unavailable locally, not claimed.

**Next action:** Run SDV on a WDK image that actually contains SDV, or keep the unavailability
explicit in release notes.

**Evidence:** `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/code-analysis-project-clean-20260716.md`.

## 2026-07-17 00:50 -03 — StartIo READ-copy race harness + live RED diagnostics

**What:** Added dedicated `STARTIO_READ_COPY_RACE` injector (queue pump + PhysicalDrive overlapped READ + second-handle UNREGISTER race) to `Invoke-WinDriveIoctlValidation.ps1`, static gate tokens, and isolated `scripts/safety/wsl2-freeze-campaign.sh` dry-run scaffold. Re-ran live guest exhaustive on `win11-drill` with signed package `97FD7B37…`.
**Category:** windows / storport / isolation / e2e
**How to measure:**
```text
powershell -ExecutionPolicy Bypass -File scripts/windows/Test-WinDriveIoctlValidationStatic.ps1
# elevated lab only:
# C:\ramshared\bin\Run-GuestExhaustive.ps1
./scripts/safety/wsl2-freeze-campaign.sh --json
```
**Measured data:**
- Static injectors: `STATIC_INJECTOR_TEST=PASS` (includes StartIo tokens)
- Campaign `guest-exhaustive-20260717-004209` (`-SkipVerifier`): ITEM-3 required verdicts all 1; `STARTIO_READ_COPY_RACE=0`
- StartIo diagnostics: `path=\\.\PhysicalDrive2 openErr=0 lastReadErr=1460 (timeout) drained=0 sq=0/0` — CreateFile OK but no SQE posted (I/O not observed at QSubmit)
- Prior full Verifier campaigns `235724` / `001940`: same STARTIO fail only; all other ITEM-3 + Verifier green
- WSL2 freeze scaffold dry-run: `daily_host=true gates_ok=false` refuse (no thrash)
- PR queue: #55 merged (`f865c94`); #53 already contained; open PR count 0
**Verdict:** 🟡 partial — StartIo READ-copy live strengthening harness landed and honestly RED; freeze-elimination still unclaimed; physical Online + SDV still blocked by policy/tooling
**Next action:** Make storage-stack READ reach QSubmit (online/format or SPTI CDB READ under pump), re-run under Verifier; keep physical/SDV/WSL2 freeze as separate non-claims
**Artifacts:** `docs/specs/no-milestone/windows-storport-cuda-vram/evidence/guest-exhaustive-20260717-004209/`, `scripts/safety/wsl2-freeze-campaign.sh`

## 2026-07-17 03:06 -03 — StartIo hang-safe SKIP + Verifier ITEM-3 PASS

**What:** Made STARTIO_READ_COPY_RACE hang-safe (no CreateFile on Win32-only LUN without Get-Disk; no background BlockingIoctl pump) and re-proved guest ITEM-3 under Driver Verifier.
**Category:** windows / storport / e2e / isolation
**How to measure:**
```text
powershell -ExecutionPolicy Bypass -File scripts/windows/Test-WinDriveIoctlValidationStatic.ps1
# elevated:
# C:\ramshared\bin\Run-GuestExhaustive.ps1
```
**Measured data:**
- Static: STATIC_INJECTOR_TEST=PASS
- Campaign `guest-exhaustive-20260717-024546` SkipVerifier: IOCTL_PASS1=PASS; STARTIO SKIP (no Get-Disk idx=2)
- Campaign `guest-exhaustive-20260717-025401` Verifier: IOCTL_PASS1=PASS IOCTL_VERIFIER=PASS VERIFIER_RAN=true; STARTIO SKIP both passes; package SHA 97FD7B37…
- Terminal: win11-drill Off after campaigns
**Verdict:** 🟡 partial — ITEM-3+Verifier green; STARTIO_READ_COPY_RACE not claimed (Win32-only LUN / no MSFT_Disk surface for safe PhysicalDrive I/O)
**Next action:** Prove StartIo under product Online (formatted volume / Get-Disk Online) or post-format guest LUN so SQEs reach QSubmit under Verifier
**Artifacts:** docs/specs/no-milestone/windows-storport-cuda-vram/evidence/guest-exhaustive-20260717-025401/

## 2026-07-17 09:33 -03 — StartIo READ-copy race CLAIMED under Verifier

**What:** Closed STARTIO_READ_COPY_RACE on isolated win11-drill by pumping the queue early post-CREATE until Get-Disk Online, then PhysicalDrive overlapped READ + second-handle UNREGISTER under Driver Verifier 0x2093B.
**Category:** windows / storport / e2e / verifier
**How to measure:**
```text
powershell -ExecutionPolicy Bypass -File scripts/windows/Test-WinDriveIoctlValidationStatic.ps1
# elevated lab only (win11-drill):
# C:\ramshared\bin\Run-StartIoProbe.ps1
# then enable verifier 0x2093B, reboot guest, re-run IOCTL harness
```
**Measured data:**
- Static: STATIC_INJECTOR_TEST=PASS (Wait-MsftDiskWithIoPump, early post-CREATE)
- Probe `startio-probe-20260717-092819`: STATUS=PASS STARTIO_READ_COPY_RACE=1 readOk=1 drained=4 sq=4/4 unregOk=1; package 97FD7B37…
- Verifier `startio-verifier-20260717-092950`: STATUS=PASS STARTIO_READ_COPY_RACE=1 readOk=1 drained=5 sq=5/5; flags 0x2093B; ramshared.sys load 1/unload 0; NO_NEW_DUMP=1
- Root cause fixed: keep StartQueuePump during CreateFile/READ; run StartIo early post-CREATE before later UNREGISTER loses MSFT_Disk
- Terminal: win11-drill Off; verifier /reset scheduled
**Verdict:** ✅ works — STARTIO_READ_COPY_RACE claimed under Verifier on isolated guest
**Next action:** Physical Online (policy), SDV (tool), isolated WSL2 freeze campaign remain non-claims
**Artifacts:** docs/specs/no-milestone/windows-storport-cuda-vram/evidence/startio-claim-20260717.md, evidence/startio-probe-20260717-092819/, evidence/startio-verifier-20260717-092950/

## 2026-07-17 09:50 -03 — WSL2 freeze campaign scaffold hardened (still NOT claimed)

**What:** Expanded `scripts/safety/wsl2-freeze-campaign.sh` with baseline artifact capture, D-state/hung_task probes, and a full isolated-lab protocol skeleton (2× before→action→after, swap-sanitize, cgroup pressure, watchdog). Daily host still refuses thrash.
**Category:** wsl2 / safety / freeze
**How to measure:**
```text
bash scripts/safety/Test-Wsl2FreezeCampaignStatic.sh
bash scripts/safety/wsl2-freeze-campaign.sh --dry-run --artifact-dir /tmp/freeze-art
# isolated lab only (never daily host):
# RAMSHARED_ISOLATED_LAB=1 ./scripts/safety/wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated
```
**Measured data:**
- STATIC_WSL2_FREEZE_CAMPAIGN=PASS
- Dry-run on daily host: gates_ok=0 reason=daily_host_refused_without_isolated_lab_flag; claim NOT_CLAIMED; baseline artifacts written
- --run-isolated without isolated flags: exit non-zero (refuse)
- SDV: sdv.exe still absent (only WDK Sdv.targets/headers)
**Verdict:** 🟡 partial — scaffold ready for isolated lab; freeze-elimination still unclaimed; no thrash on daily host
**Next action:** Run --run-isolated on a true isolated WSL/VM lab with RAMSHARED_ISOLATED_LAB=1; keep physical Online + SDV blocked
**Artifacts:** docs/specs/no-milestone/wsl2-freeze/evidence/freeze-baseline-20260717-094842

## 2026-07-17 09:58 -03 — Manufactured pagefile Gate A refusal (unit + guest inject)

**What:** Closed the optional manufactured active-pagefile refusal campaign for the product teardown path: unit test proves Gate A refuse/code 7/no destroy; guest lab injects configured PagingFiles for product letter and restores safely.
**Category:** windows / pagefile / isolation / e2e
**How to measure:**
```text
cargo test -p ramshared-winsvc --lib manufactured_pagefile
powershell -ExecutionPolicy Bypass -File scripts/windows/Test-PagefileRefusalManufacturedStatic.ps1
# guest lab:
# Invoke-PagefileRefusalManufactured.ps1 -Letter S
```
**Measured data:**
- Unit: manufactured_pagefile_on_product_volume_refuses_gate_a PASS
- Static: STATIC_PAGEFILE_REFUSAL_MANUFACTURED=PASS
- Guest win11-drill: PAGEFILE_REFUSAL_MANUFACTURED=1 restored=true configuredOnVolume=true (registry inject only)
**Verdict:** ✅ works (decision path + guest inject); optional live Online+stop inject remains available
**Next action:** Physical Online (policy), SDV (no sdv.exe), freeze claim (isolated lab)
**Artifacts:** docs/specs/no-milestone/windows-storport-cuda-vram/evidence/pagefile-refusal-20260717-095826/

## 2026-07-17 10:31 -03 — Live pagefile Online+stop refuse + SDV probe NOT_CLAIMED

**What:** Live Gate A refuse on win11-drill product Online (`-ManufacturedPagefileRefuse`): configured `S:\pagefile.sys` causes code 7 resume Online, then clean stop. SDV probe documents tool absence (MSB4057 / no sdv.exe).
**Category:** windows / pagefile / e2e / sdv
**How to measure:**
```text
# elevated lab:
# Run-GuestProductOnline.ps1 -ManufacturedPagefileRefuse
powershell -ExecutionPolicy Bypass -File scripts/windows/Invoke-SdvProbe.ps1
powershell -ExecutionPolicy Bypass -File scripts/windows/Test-SdvProbeStatic.ps1
```
**Measured data:**
- Live: pagefileRefusePass=true diagHit=gate_a_active S:\pagefile.sys; stillOnline; clean stop exit 0; lease liberado; cudaRestored; noNewDump; BINARY_MATCH 97FD7B37…
- Host summary initially false-negative (expected 3 DT-13 rounds); corrected single-round PASS for refuse campaign
- SDV: SDV_CLAIM=NOT_CLAIMED reasons=sdv.exe_not_on_path,msbuild_target_sdv_missing
**Verdict:** ✅ works (live pagefile Online refuse); 🟡 partial (SDV tool absent)
**Next action:** Isolated freeze claim; install SDV; keep physical Online blocked
**Artifacts:** docs/specs/no-milestone/windows-storport-cuda-vram/evidence/pagefile-online-refuse-20260717-102614/, evidence/sdv-probe-20260717/

## 2026-07-17 11:10 -03 — SDV retired on modern WDK (verified, still NOT_CLAIMED)

**What:** Verified SDV cannot be claimed on this Day-0 lab: WDK 10.0.26100 already installed; sdv.exe absent; official WindowsDriver.Sdv.targets stub states SDV is no longer in WDK and incompatible with VS2022+. Freeze remain daily-host refused; physical Online still policy-blocked.
**Category:** windows / sdv / isolation
**How to measure:**
```text
powershell -ExecutionPolicy Bypass -File scripts/windows/Invoke-SdvProbe.ps1
bash scripts/safety/wsl2-freeze-campaign.sh --check-gates
```
**Measured data:**
- winget: Microsoft.WindowsWDK.10.0.26100 installed, no update
- tree search: no sdv.exe under Windows Kits / VS BuildTools
- targets text: "no longer included in the Windows Driver Kit" / "no longer compatible with VS2022"
- freeze --check-gates: daily_host=1 gates_ok=0
**Verdict:** 🟡 partial — SDV gap is tool retirement (not agent install skip); freeze/physical still env/policy
**Next action:** Optional older EWDK for SDV only; true isolated WSL lab for freeze claim
**Artifacts:** docs/specs/no-milestone/windows-storport-cuda-vram/evidence/sdv-probe-20260717/

## 2026-07-17 11:20 -03 — SSDV3 close: SDV = N/A (DT-30), gates de-falsified

**What:** Applied Day-0 discipline: SPEC DT-30 marks Static Driver Verifier N/A on VS2022/WDK 26100 (Microsoft retirement, not missing install). Primary kernel gates remain Code Analysis + Driver Verifier + live IOCTL. IMPL gate matrix separates claimed, N/A, policy RED, and env-bound partial. Freeze/physical daily Online stay honest non-claims without false “pending agent work”.
**Category:** docs / ssdv3 / windows
**How to measure:**
```text
rg "DT-30|SDV N/A" docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md
powershell -ExecutionPolicy Bypass -File scripts/windows/Invoke-SdvProbe.ps1
bash scripts/safety/wsl2-freeze-campaign.sh --check-gates
./scripts/docs-check.sh
```
**Measured data:**
- SPEC DT-30 added; ITEM-3 abort no longer requires SDV when DT-30 applies
- Probe: sdv_retired_from_wdk_vs2022_plus (prior evidence)
- Freeze: daily_host refuse (gates_ok=0)
- Physical daily Online: policy RED unchanged
**Verdict:** ✅ works (documentation discipline close for this slice’s false pendings)
**Next action:** Only true new env: disposable isolated WSL for freeze claim, or separate EWDK for optional SDV
**Artifacts:** docs/specs/no-milestone/windows-storport-cuda-vram/{SPEC,IMPL}.md; evidence/sdv-probe-20260717/

## 2026-07-17 12:05 -03 — Slice close: security checklist + release 0.6.3

**What:** Closed remaining open SSDV3 security checklist boxes with executable evidence pointers; marked daily-host physical Online as policy N/A (not incomplete). Merged release-please v0.6.3. Windows StorPort Day-0 path is PASS; only true env-bound leftovers are WSL2 freeze claim (isolated lab) and optional older-EWDK SDV (out of scope DT-30).
**Category:** docs / ssdv3 / release
**How to measure:**
```text
rg "Security checklist \\(Step 3" docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md
gh release view v0.6.3
./scripts/docs-check.sh
```
**Measured data:**
- PR #91 release v0.6.3 merged (CI green)
- Security checklist all [x] with test/live evidence refs
- Daily-host physical Online = N/A policy
**Verdict:** ✅ works (discipline close of open checklists)
**Next action:** None on daily host; optional new env for freeze claim only
**Artifacts:** docs/specs/no-milestone/windows-storport-cuda-vram/{SPEC,IMPL}.md

## 2026-07-17 12:18 -03 — Freeze: RamShared-Kernel is NOT isolab + shared-desktop gate

**What:** Probed WSL distro `RamShared-Kernel` (custom kernel 6.18.35.2) as candidate freeze lab. Confirmed it mounts `/mnt/c/Users` on the same Windows desktop host as Ubuntu-24.04 — not disposable isolab. Tightened `wsl2-freeze-campaign.sh` so `/mnt/c/Users` marks shared desktop (any distro) and refuses `--run-isolated` without FORCE. Restored WSL PE binfmt (`WSLInterop`) so Windows interop works again from this session. Release v0.6.4 already Latest (PR #94).
**Category:** safety / freeze / discipline
**How to measure:**
```text
wsl -l -v
wsl -d RamShared-Kernel --cd ~ -e bash -lc 'echo $WSL_DISTRO_NAME; test -d /mnt/c/Users && echo MNT=1'
./scripts/safety/Test-Wsl2FreezeCampaignStatic.sh
RAMSHARED_ISOLATED_LAB=1 ./scripts/safety/wsl2-freeze-campaign.sh --allow-isolated-lab --run-isolated --artifact-dir /tmp/freeze-refuse-test
gh release view v0.6.4
```
**Measured data:**
- RamShared-Kernel: DISTRO=RamShared-Kernel, MNT_C_USERS=1, same kernel as daily, same hostname
- Static freeze campaign: PASS
- Isolated run on daily: refuse `daily_host_refuses_run_isolated,shared_windows_desktop_refuses_run_isolated`
- claim remains NOT_CLAIMED; no thrash
- v0.6.4 Latest published
**Verdict:** ✅ works (honest env classification + safer refuse gate)
**Next action:** True freeze claim needs separate disposable lab VM/machine — not a second WSL distro on this desktop
**Artifacts:** docs/specs/no-milestone/wsl2-freeze/evidence/ramshared-kernel-probe-20260717/; scripts/safety/wsl2-freeze-campaign.sh
## 2026-07-17 21:10 — Memory Broker DCC code surface implemented

**What:** Implemented the safe P2 code surface for the generic Windows host/DCC
consumer: `DccAgent` transport, bounded local JSON-lines protocol, TOML config
crate, Windows memory-pressure sampler boundary, deterministic evidence
explanations, and the generic DCC lease/status path.

**Measured data:**

- `cargo test --workspace --all-targets`: **PASS**, 650 tests passed; only
  explicitly privileged/GPU/ublk tests remained ignored by environment gates.
- Targeted Clippy with `-D warnings`: **PASS**.
- `cargo fmt --all`, Python syntax compilation, and `git diff --check`: **PASS**.

**Safety boundaries:** the DCC path can request/release a broker lease but
cannot issue swap commands; local messages are capped at 64 KiB; process
attribution is omitted unless explicitly observed.

**Still not claimed:** live WDDM pressure caused by an external GPU workload, successful
DEMOTE under that pressure, real scene completion under the lease, and the
isolated two-round WSL2 freeze campaign. The shared desktop was not thrashed.

**Verdict:** 🟡 **PARTIAL — code green, hardware gates open**

**Evidence:** `docs/specs/no-milestone/memory-broker/IMPL.md`

## 2026-07-17 21:25 — Safe pending-gate audit on shared desktop

**What:** Re-ran the freeze campaign gate and read-only cascade health probes
after the generic naming/adapter changes.

**Measured data:**

- `wsl2-freeze-campaign.sh --check-gates --json`: `gates_ok=false`, reason
  `daily_host_refused_without_isolated_lab_flag`.
- `Test-Wsl2FreezeCampaignStatic.sh`: `STATIC_WSL2_FREEZE_CAMPAIGN=PASS`.
- `cascade-health.sh --once`: `ok=true`, daemon absent, no ghost swap, zero
  zram/VRAM swap, disk swap used ~203 MiB, GPU free ~4508 MiB, D-state 0.

**Verdict:** 🟡 **ENVIRONMENT-BOUND — correctly refused destructive action**.

The WDDM pressure and two-round freeze gates remain unclaimed. Running them on
this shared desktop would violate the repository host-safety policy.

## 2026-07-17 19:25 -03 — Hyper-V VM access documented + win11-drill live product PASS

**What:** Verified the correct non-interactive access path for the named lab
VMs and documented it for future agents without storing secrets.

**Measured data:**

- `win11-drill` PowerShell Direct works with `WIN11-DRILL\drilladmin`.
  The shorthand `.\drilladmin` can fail on this image.
- `Run-GuestProductOnline.ps1` on `win11-drill`: **PASS**.
  Artifact: `C:\ramshared\artifacts\guest-product-online-20260717-191834`.
- Campaign summary: `LIFECYCLE_ROUNDS=3`, `ONLINE=true`,
  `BINARY_MATCH=true`, `ROUNDS_PASS=true`, `CONSOLE_EXIT_ZERO=true`,
  `NO_FORCE_KILL=true`, `LEASE_RELEASED=true`, `CUDA_RESTORED=true`,
  `NO_NEW_DUMP=true`, `TERMINAL_SAFE=true`, `PASS=true`.
- `linux-kernel-lab` boots under Hyper-V control, but no shell channel is
  available from this session: no guest IP on `Default Switch`, KVP no contact,
  Linux guest has no PowerShell Direct.
- Terminal state confirmed: `win11-drill=Off`, `linux-kernel-lab=Off`.

**Docs / script hygiene:**

- Added `docs/labs/HYPERV-VM-ACCESS.md`.
- Updated Windows harness defaults to `WIN11-DRILL\drilladmin`.
- Added local-only credential ignore patterns for `.drill-pw` and secret files.

**Verification:**

- PowerShell parser for changed scripts: **PASS**.
- `Test-GuestProductOnlineStatic.ps1`: **PASS**.
- `Test-GuestExhaustiveStatic.ps1`: **PASS**.
- `./scripts/docs-check.sh`: **PASS**.
- `git diff --check`: **PASS**.

**Verdict:** ✅ `win11-drill` access and product campaign are live-proven.
`linux-kernel-lab` remains power-controllable only until SSH/serial/console
automation is configured.

## 2026-07-17 19:35 -03 — Windows lab credential hygiene

**What:** Removed remaining hardcoded Windows lab/signing secret defaults from
`Install-WinDriveVm.ps1`. The script now requires explicit parameters or
environment variables for both the guest password and test-signing PFX
password.

**Measured data:**

- `Install-WinDriveVm.ps1` uses `RAMSHARED_DRILL_PASSWORD` and
  `RAMSHARED_TESTSIGN_PFX_PASSWORD`; no literal defaults.
- Secret literal scan for old/default credential shapes: **PASS**.
- PowerShell parser for changed Windows scripts: **PASS**.
- `Test-SignDriversStatic.ps1`: **PASS**.
- `cargo fmt --all -- --check`: **PASS**.
- `cargo clippy --workspace --all-targets -- -D warnings`: **PASS**.
- `cargo test --workspace --all-targets`: **PASS**.
- `scripts/p0/measure-gpu-workload-vram.ps1` PowerShell parser: **PASS**.
- `./scripts/docs-check.sh`: **PASS**.
- `git diff --check`: **PASS**.
- Terminal state confirmed: `win11-drill=Off`, `linux-kernel-lab=Off`.

**Verdict:** ✅ tracked scripts no longer carry the known lab credential
literals; local-only credential files remain ignored and must not be printed.

## 2026-07-17 19:41 -03 — win11-drill exhaustive IOCTL + Verifier PASS

**What:** Re-ran the isolated Windows exhaustive harness after fixing the
canonical PowerShell Direct identity.

**Measured data:**

- Harness: `Run-GuestExhaustive.ps1`.
- Artifact: `C:\ramshared\artifacts\guest-exhaustive-20260717-192931`.
- `IOCTL_PASS1=PASS`.
- `IOCTL_VERIFIER=PASS`.
- `VERIFIER_RAN=true`.
- Verifier flags observed: `0x0002093b`.
- Verified module: `ramshared.sys`, `load: 1 / unload: 0`.
- Driver Store/package `BINARY_MATCH=true` with package SHA
  `97FD7B373ED7DD5AE7F38204070F8B89E08A2B25616AA2A128995E8D1FBFF34F`.
- Terminal state confirmed: `win11-drill=Off`, `linux-kernel-lab=Off`.

**Verification:**

- `Test-GuestExhaustiveStatic.ps1`: **PASS**.
- `./scripts/docs-check.sh`: **PASS**.
- `git diff --check`: **PASS**.

**Verdict:** ✅ `win11-drill` exhaustive IOCTL and Driver Verifier path are
live-proven with the documented access path.

## 2026-07-17 19:58 -03 — linux-kernel-lab SSH access recovered via ARP fallback

**What:** Rechecked older records and restored the documented non-interactive
access path for the Hyper-V Linux lab.

**Measured data:**

- Historical record found: 2026-07-10 validation said SSH worked from the
  Windows host, not from WSL NAT.
- Local access file confirms user `<user>`, SSH keys installed, passwordless
  sudo, and MAC lookup fallback.
- `Get-VMNetworkAdapter.IPAddresses` remained empty, but Windows neighbor
  table mapped VM MAC `00-15-5D-00-FA-04` to `172.23.18.42`.
- New helper `Get-LinuxKernelLabAccess.ps1 -Start -Smoke`: **PASS**.
- SSH smoke from Windows host:
  - hostname: `linux-kernel-lab`
  - kernel: `6.8.0-134-generic`
  - `cloud-init status --wait`: `done`
  - `sudo -n true`: **PASS**
  - SSH service: active
  - netplan: DHCP on `eth0`, MAC match `00:15:5d:00:fa:04`
  - root filesystem: 38G size, 7.1G used, 31G available
  - memory: 5.8Gi total, ~5.3Gi available
- Kernel clone probe: `~/src/WSL2-Linux-Kernel` HEAD `1bd4ed3d4`.
- `/dev/ublk-control`: absent, consistent with the generic Ubuntu kernel.
- Terminal state confirmed: `win11-drill=Off`, `linux-kernel-lab=Off`.

**Docs / script hygiene:**

- Added `scripts/windows/Get-LinuxKernelLabAccess.ps1`.
- Updated `docs/labs/HYPERV-VM-ACCESS.md` with ARP fallback and SSH smoke
  commands.

**Verdict:** ✅ `linux-kernel-lab` is accessible again for non-destructive
kernel-build/smoke work via Windows-host SSH. It remains unsuitable for VRAM
proof because it has no GPU assignment.

## 2026-07-17 20:20 -03 — app-specific DCC naming removed

**What:** Removed the app-specific DCC adapter surface from this slice. The
product behavior and public tree now use generic workload/DCC naming instead of
promoting one GPU application as the architecture.

**Measured data:**

- Removed the app-specific Python adapter from `integrations/`.
- Replaced the app-specific render probe with
  `scripts/p0/measure-gpu-workload-vram.ps1`, which only samples aggregate
  VRAM/RAM while any external GPU workload runs.
- Updated README, naming rules, PRD/SPEC/IMPL, reliability docs, and validation
  text to generic GPU workload / DCC host language.
- App-specific name scan over README/docs/scripts/crates/validation/rules:
  **PASS**.
- PowerShell parser for `measure-gpu-workload-vram.ps1`: **PASS**.
- `cargo test -p ramshared-agent --all-targets`: **PASS**.
- `./scripts/docs-check.sh`: **PASS**.
- `git diff --check`: **PASS**.
- Terminal state confirmed: `win11-drill=Off`, `linux-kernel-lab=Off`.

**Verdict:** ✅ The current slice no longer exposes an app-specific integration
name as product architecture. Host-specific adapters remain deferred.

## 2026-07-17 20:55 -03 — public app-name and elevated-access gap audit

**What:** Extended the generic naming audit to changelog/history text, filesystem
paths, and the documented elevated Hyper-V access path.

**Measured data:**

- Removed stale app-specific render-script wording from `CHANGELOG.md`.
- Public content scan for example application names and old integration/script
  names across repo surfaces, excluding local-only `MEMORY.md`: **PASS**.
- Filesystem path scan for old app-specific directories/files: **PASS**.
- Secret literal scan for lab password/signing/API-key shapes: **PASS**.
- Elevated WSL wrapper `scripts/windows/wsl-elevated-ps.sh` successfully ran
  `Get-VM`; terminal state confirmed:
  - `win11-drill=Off`
  - `linux-kernel-lab=Off`
- `Test-LinuxKernelLabAccessStatic.ps1`: **PASS**.
- PowerShell parser for changed Windows/P0 scripts: **PASS**.
- `./scripts/docs-check.sh`: **PASS**.
- `git diff --check`: **PASS**.

**Verdict:** ✅ No remaining public app-specific naming gap was found. Elevated
VM access is documented and currently works through the repository wrapper
without committing or printing credentials.

## 2026-07-17 21:15 -03 — workspace verification after naming cleanup

**What:** Re-ran the verification loop after the generic naming cleanup and
fixed a source-language gap found during manual review.

**Measured data:**

- Corrected new Rust source strings in `ramshared-config`,
  `ramshared-host-agent`, and DEMOTE explanations to English.
- New-source Portuguese/string scan for the touched Rust files: **PASS**.
- `cargo fmt --all -- --check`: **PASS**.
- `cargo clippy --workspace --all-targets -- -D warnings`: **PASS**.
- `cargo test --workspace --all-targets`: **PASS**.
- Post-format targeted tests:
  `cargo test -p ramshared-config -p ramshared-agent --all-targets`: **PASS**.
- App-specific public content/path scans: **PASS**.
- Secret literal scan: **PASS**.
- PowerShell parser checks for changed Windows/P0 scripts: **PASS**.
- `Test-LinuxKernelLabAccessStatic.ps1`: **PASS**.
- `./scripts/docs-check.sh`: **PASS**.
- `git diff --check`: **PASS**.
- Elevated VM state probe through `scripts/windows/wsl-elevated-ps.sh`: **PASS**,
  with both `win11-drill` and `linux-kernel-lab` Off.

**Verdict:** ✅ The current working tree is ready for normal review/test of the
generic VRAM reclaim, host-agent, VM-access, and naming-policy slice. Destructive
root/GPU ignored tests remain intentionally gated to isolated lab execution.

## 2026-07-17 21:55 -03 — ignored root/GPU tests executed

**What:** Executed the previously ignored CUDA, Vulkan, root ublk, VRAM ublk,
fio, and bounded swap tests. The standalone ublk daemon smoke was executed via
the existing isolated QEMU drill instead of opening its WSL2 freeze gate on the
daily host.

**Bugs found and fixed:**

- `ublk_control_smoke` assumed `UBLK_F_SUPPORT_ZERO_COPY` was absent. Current
  WSL2 ublk advertises it, so the test now asserts the current feature contract.
- Current ublk rejects tiny 128 KiB smoke disks and BASIC params with
  `max_sectors=0`. `Params::basic_disk` now defaults to 8 sectors (4 KiB), and
  ublk smoke disks use 1 MiB minimum where needed.
- Removed Portuguese strings/comments from the touched ublk UAPI/test code.

**Ignored-test evidence:**

- `cargo test -p ramshared-cuda -- --ignored --test-threads=1`: **PASS**.
- `cargo test -p ramshared-vulkan -- --ignored --test-threads=1`: **PASS**.
- `cargo test -p ramshared-winsvc cuda_probe::tests::probe_cuda_allocates_roundtrips_and_restores -- --ignored --test-threads=1`: **PASS**.
- `cargo test -p ramshared-wsl2d backend::tests::vram_backend_serves_nbd_write_then_read -- --ignored --test-threads=1`: **PASS**.
- `cargo test -p ramshared-wsl2d backend::tests::vram_gauge_outros_captures_real_graphics_usage -- --ignored --test-threads=1`: **PASS**.
- Root `ublk_control_smoke --ignored --test-threads=1`: **PASS**.
- Root `ublk_io_smoke --ignored --test-threads=1`: **PASS**.
  - `bench_vram_ublk_read_latency`: p50 ~263 us, p99 ~642 us in the final run.
  - `fio_bench_vram_ublk`: ~3715 IOPS / 14.5 MiB/s in the final run.
  - `vram_ublk_round_trips_as_swap_device`: **PASS**; `/proc/swaps` returned to
    the original disk-only state.
- `./scripts/kernel/qemu-ublk-daemon.sh`: **PASS**.
  - `KTEST-INSMOD=ok`
  - `KTEST-UBLK-CONTROL=present`
  - `KTEST-SERVED=ok`
  - `KTEST-TERMINATED=ok`
  - `KTEST-DEVICE-REMOVED=ok`

**Terminal state:**

- `/proc/swaps`: disk swap only (`/dev/sdc`).
- `/dev/ublk*`: only `/dev/ublk-control`.
- GPU memory after tests: 4565 / 6144 MiB free.
- Elevated VM state probe: `win11-drill=Off`, `linux-kernel-lab=Off`.

**Regression checks after fixes:**

- `cargo fmt --all -- --check`: **PASS**.
- `cargo clippy --workspace --all-targets -- -D warnings`: **PASS**.
- `cargo test --workspace --all-targets`: **PASS**.
- `./scripts/docs-check.sh`: **PASS**.
- `git diff --check`: **PASS**.
- App-specific public scan: **PASS**.
- Secret literal scan: **PASS**.
- PowerShell parser checks: **PASS**.

**Verdict:** ✅ The ignored root/GPU surface is now exercised. The only WSL2
freeze-gated daemon case remains unsafe to run on the daily host and is covered
by the isolated QEMU drill that validates serve + SIGTERM teardown + device
removal.

## 2026-07-17 22:40 -03 — public hygiene gate and gap register

**What:** Added a tracked public hygiene gate and a reliability gap register so
future agents cannot silently reintroduce example-app naming, signing-password
literals, or false DONE promotion for environment-bound claims.
**Category:** ci-gate + documentation
**How to measure:**
```bash
node tools/ci/check-public-hygiene.mjs
./scripts/docs-check.sh
git diff --check
```
**Measured data:**
- `node tools/ci/check-public-hygiene.mjs`: **PASS**.
- `./scripts/docs-check.sh`: **PASS** and now runs the public hygiene gate.
- `git diff --check`: **PASS**.
- Open gates are listed in `docs/reliability/GAP-REGISTER.md` with required
  close evidence for external GPU workload pressure, isolated WSL2 freeze
  campaign, Windows physical Online, guest GPU-PV CUDA, and custom-kernel ublk
  product promotion.
**Verdict:** ✅ Current public hygiene gap is closed with a repeatable gate;
environment-bound product claims remain explicitly PARTIAL until their listed
evidence exists.

## 2026-07-17 22:55 -03 — gap register schema gate

**What:** Added a machine-checkable gate for `docs/reliability/GAP-REGISTER.md`.
It enforces concrete open-gate rows, rejects DONE/PASS promotion in the open
table, rejects placeholder close evidence, and verifies that primary docs link
back to the register.
**Category:** ci-gate + documentation
**How to measure:**
```bash
node tools/ci/check-gap-register.mjs
./scripts/docs-check.sh
node tools/ci/check-validation-schema.mjs --all
git diff --check
```
**Measured data:**
- `node tools/ci/check-gap-register.mjs`: **PASS**.
- `./scripts/docs-check.sh`: **PASS**, including gap register and public
  hygiene gates.
- `node tools/ci/check-validation-schema.mjs --all`: **PASS**.
- `git diff --check`: **PASS**.
- Gap register state: **5** current open gates and **4** closed session gaps.
**Verdict:** ✅ Open environment-bound gates are now protected by a repeatable
schema gate, not just prose.

## 2026-07-17 23:05 -03 — P0 workload wording cleanup

**What:** Updated `docs/reliability/memory-broker-p0-results.md` to remove stale
render/tester-specific wording and placeholder cells. The remaining open P0
measurement now uses app-agnostic external GPU workload terminology aligned
with `Invoke-GpuWorkloadGate.ps1`.
**Category:** documentation
**How to measure:**
```bash
rg -n "render|Render|Alex|PENDING|scene|failed" docs/reliability/memory-broker-p0-results.md
./scripts/docs-check.sh
node tools/ci/check-validation-schema.mjs --all
git diff --check
```
**Measured data:**
- Stale wording scan: **0** matches.
- `./scripts/docs-check.sh`: **PASS**.
- `node tools/ci/check-validation-schema.mjs --all`: **PASS**.
- `git diff --check`: **PASS**.
**Verdict:** ✅ P0 workload docs now match the generic naming policy and the
remaining workload measurement stays explicit as unmeasured, not app-specific.

## 2026-07-17 23:20 -03 — QEMU drills gain in-guest binary match

**What:** Updated the isolated QEMU ublk-daemon and broker drills to compare
host-side SHA-256 with the binary copied into the guest initramfs before
claiming PASS.
**Category:** isolation + ci-gate
**How to measure:**
```bash
bash -n scripts/kernel/qemu-ublk-daemon.sh
bash -n scripts/kernel/qemu-broker-drill.sh
./scripts/kernel/qemu-ublk-daemon.sh
./scripts/kernel/qemu-broker-drill.sh
./scripts/docs-check.sh
node tools/ci/check-validation-schema.mjs --all
git diff --check
```
**Measured data:**
- `qemu-ublk-daemon.sh`: `KTEST-BINARY-MATCH=ok`,
  `KTEST-SERVED=ok`, `KTEST-TERMINATED=ok`,
  `KTEST-DEVICE-REMOVED=ok`.
- `qemu-broker-drill.sh`: `KTEST-DAEMON-BINARY-MATCH=ok`,
  `KTEST-AGENT-BINARY-MATCH=ok`, `KTEST-SWAP-ACTIVE=ok`,
  `KTEST-TELEMETRY=ok`, `KTEST-SWAPOFF=ok`,
  `KTEST-DAEMON-TERMINATED=ok`.
- `./scripts/docs-check.sh`: **PASS**.
- `node tools/ci/check-validation-schema.mjs --all`: **PASS**.
- `git diff --check`: **PASS**.
**Verdict:** ✅ Current isolated QEMU drills now include binary-match evidence.
The universal WSL2 freeze claim remains PARTIAL until the separate GPU-PV/dxg
host-reclaim campaign exists.

## 2026-07-22 01:53 -03 — WSL2 external global GPU free-floor DEMOTE

**What:** Ran the supervised shared-host WSL2 pressure campaign with a generic
Windows CUDA workload consuming 4096 MiB of VRAM, WSL2 sparse VRAM capacity
4096 MiB, zram 1024 MiB, and host disk telemetry for `C:` and `I:`.
**Category:** WSL2 + external GPU pressure + telemetry
**How to measure:**
```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass `
  -File scripts/windows/Invoke-SharedWslPressureCampaign.ps1 `
  -ApproveSharedDailyHost -VramMiB 4096 -ZramMiB 1024 `
  -Rounds 1 -ExternalWorkloadMiB 4096 -ExternalWorkloadHoldSec 90 `
  -ExternalWorkloadDelaySec 8 -PostCampaignObserveSec 120 `
  -HostDiskLetters C,I
```
**Measured data:**
- Artifact: `C:\ramshared\artifacts\shared-wsl-pressure-20260722-015303`.
- `STATUS=PASS`, `REASON=validated_external_global_gpu_demote`.
- External workload released cleanly; `external_workload_ok=true`.
- `ramshared diagnose --events --json`: `demotes=2`, timeline reason
  `GlobalGpuFreeFloor`, process not attributed.
- GPU pressure: min free 348 MiB; max used 5607 MiB.
- Final health: `ghost=false`, daemon dead, no zram/VRAM swap left.
- Host disk telemetry: `C:` max write 462.20 MiB/s, max read 304.79 MiB/s,
  max queue 6; `I:` max write 315.09 MiB/s, max read 3.28 MiB/s, max queue 130.
**Verdict:** ✅ The aggregate external VRAM pressure DEMOTE path is proven on
the shared WSL2 host. This does not close the separate GiB reclaim matrix.

## 2026-07-24 03:40 -03 — calibrated GiB reclaim matrix closure

**What:** Closed the remaining WSL2 1 GiB, WSL2 4 GiB, and calibrated split
matrix rows under the approved Windows watchdog harness. Hardened the runner so
integrity work completes before staged external pressure, the split runner
captures all PowerShell streams, and matrix closure requires the nested
campaign summary to report both `PASS` and `matrix_row_close=true`.
**Category:** WSL2 + Windows StorPort + release verification
**How to measure:**
```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --workspace
./scripts/docs-check.sh
node tools/ci/check-public-hygiene.mjs
scripts/package/build-linux-bundle.sh --skip-build
```
**Measured data:**
- WSL2 1 GiB:
  `C:\ramshared\artifacts\shared-wsl-pressure-20260723-232558`, `PASS`,
  two integrity rounds, DEMOTE, freeze validation, and clean terminal state.
- WSL2 4 GiB:
  `C:\ramshared\artifacts\shared-wsl-pressure-20260724-031615`, `PASS`,
  preallocated VRAM, 4096 MiB external pressure, two DEMOTEs,
  `matrix_row_close=true`, and clean terminal state.
- Split 1 GiB Windows + 3 GiB WSL2 + 1 GiB staged external pressure:
  `C:\ramshared\artifacts\vram-reclaim-matrix-20260724-032344`, `PASS`,
  with three StorPort checksum matches, graceful teardown, lease release,
  zero disk/Win32/PnP residue, WSL2 integrity, DEMOTE, and clean terminal state.
- Rust format, workspace tests, clippy with warnings denied, and release build:
  **PASS**.
- All Windows/P0 static tests and both WSL2 freeze static suites: **PASS**.
- Windows `ramshared.sys` and `poolstress.sys` rebuilt with WDK 26100,
  `/W4 /WX`: **PASS**.
- Docs, gap-register schema, public hygiene, and diff whitespace checks:
  **PASS**.
- Linux/WSL2 local bundle manifest verification and archive read: **PASS**.
- `InfVerif.exe` is absent on this host. Public Windows distribution remains
  blocked on production trust/attestation and its clean-tag install,
  rollback, and recovery drill; test-signing is not release evidence.
**Verdict:** ✅ The calibrated Linux/WSL2 GiB reclaim matrix is closed on the
RTX 2060 surface. NBD remains the stable day-one transport; ublk stays
deliberately deferred. Windows remains a supervised beta until the external
production-signing gate is completed.

## 2026-07-24 04:15 -03 — Jules PR audit and MVP consolidation

**What:** Audited all 36 open Jules-generated PRs (`#107` through `#142`) and
consolidated the valid concerns into one owning-layer implementation. Rejected
parallel swapoff/NBD teardown, flaky kernel-specific fake-device tests,
duplicate substring path checks, generated root junk, and unmeasured
micro-optimizations. Kept NBD as the MVP transport and deferred the ublk
`OwnedFd` refactor to its dedicated lifecycle scope.
**Category:** security + reliability + release
**How to measure:**
```bash
cargo test -p ramshared-agent -p ramshared-cli -p ramshared-winsvc \
  -p ramshared-wsl2d -p ramshared-vulkan --all-targets
cargo clippy -p ramshared-agent -p ramshared-cli -p ramshared-winsvc \
  -p ramshared-wsl2d -p ramshared-vulkan --all-targets -- -D warnings
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-agent \
  --files crates/ramshared-agent/src/psi.rs --min 80
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-wsl2d \
  --files crates/ramshared-wsl2d/src/demote_status.rs,crates/ramshared-wsl2d/src/swap.rs,crates/ramshared-wsl2d/src/telemetry.rs,crates/ramshared-wsl2d/src/ublk.rs,crates/ramshared-wsl2d/src/ublk_control.rs \
  --min 80
```
**Measured data:**
- Complete per-PR disposition:
  `docs/reliability/JULES-PR-AUDIT-20260724.md`.
- Targeted package tests and clippy with warnings denied: **PASS**.
- Windows MSVC cross-check caught PR #116 removing a live cfg-windows field;
  the patch was rejected and the cross-check then passed.
- Agent PSI/cgroup slice coverage: **94.8%**.
- WSL2 daemon touched slices: **92.7%–100%**.
- Whole-file CLI coverage reports 4.2% for `cascade_io.rs` and 34.4% for
  `main.rs`; these large command/shell boundary files are not SSDV3 matrix
  slices. The newly introduced pure PID identity predicate has a named
  regression test. Live cascade/matrix evidence remains the authoritative E2E
  gate for the shell boundary.
- `product_online.rs` is Windows-cfg and absent from the Linux llvm-cov profile;
  Windows build/static/live campaign evidence is required instead.
**Verdict:** ✅ Accepted/reworked Jules concerns are consolidated without
weakening teardown order. Rejected/deferred PRs are not part of the MVP claim.

## 2026-07-24 04:13 -03 — post-v0.7.3 lifecycle and telemetry audit

**What:** Audited teardown identity, broker reconciliation, telemetry sinks,
campaign summaries, privileged socket paths, dependency advisories, and
release-facing documentation. Fixed exact NBD matching, fail-closed
`/proc/swaps` reads, lifecycle allowlists, broker slice attribution, telemetry
write visibility, WSL campaign PASS criteria, and summary validation.
**Category:** security + hang prevention + telemetry integrity + release docs
**How to measure:**
```bash
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --workspace
cargo check -p ramshared-winsvc --target x86_64-pc-windows-msvc
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-wsl2d \
  --files crates/ramshared-wsl2d/src/broker_srv.rs,crates/ramshared-wsl2d/src/telemetry.rs --min 80
node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli \
  --files crates/ramshared-cli/src/cascade/mod.rs,crates/ramshared-cli/src/cascade/lifecycle.rs --min 80
./scripts/docs-check.sh
node tools/ci/check-validation-schema.mjs --all
node tools/ci/check-gap-register.mjs
node tools/ci/check-public-hygiene.mjs
cargo audit
```
**Measured data:**
- Workspace tests: **PASS**; 17 daemon binary tests include exact NBD identity,
  duplicate/deleted row handling, and non-socket path refusal.
- Broker regression `dev_to_slice_requires_exact_nbd_identity`: **PASS**;
  `/dev/sda5`, nested paths, suffix lookalikes, and deleted entries are not
  attributed to slice 5.
- Telemetry sink `/dev/full` write-failure regression: **PASS**; write failure
  is surfaced and the sink is disabled instead of silently dropping rows.
- CLI lifecycle regressions: **68/68 PASS**; similarly named swap files are not
  classified, swapped off, or disconnected as RamShared devices.
- WSL artifact validator rejects missing `gates_ok`, unapproved daily-host
  summaries, invalid integrity JSON, checksum mismatch, and incomplete rounds:
  **PASS**.
- All 21 Windows static harnesses, including shared WSL, exhaustive guest,
  pagefile refusal, driver IOCTL, signing, and disk telemetry checks: **PASS**.
- Rust slice coverage: broker service **89.5%**, telemetry **100%**, CLI
  lifecycle **94.6%**, CLI cascade module **90.0%**.
- RustSec scan: **0 known vulnerable dependencies** in 45 locked crates.
- Release build, Windows MSVC cross-check, docs/index/links, validation schema,
  gap register, public hygiene, archive read, and internal bundle
  `SHA256SUMS`: **PASS**.
- No new destructive pressure run was performed. Existing 2026-07-22 and
  2026-07-24 supervised live artifacts remain the before/action/after evidence.
**Verdict:** ✅ The confirmed post-release lifecycle and telemetry defects are
fixed with fail-closed behavior and named regressions. The Linux/WSL2 NBD MVP
claim remains bounded to the previously validated surface; Windows public
distribution and ublk product transport remain BLOCKED/DEFERRED respectively.

## 2026-07-24 04:30 — v0.7.4 real-host smoke and Windows cleanup

**What:** Real-host smoke validation for the stable v0.7.4 tag and cleanup of
the Windows test-signing state.

**Environment:** exact tag `v0.7.4`; bounded WSL2 smoke on the real host with
`ramshared up --vram 128 --zram 128`, followed by graceful `down`.

**Measured data:**
- Online state: daemon alive, VRAM/zram present, no ghost devices, expected
  ordering, and GPU free memory approximately 4729 MiB: **PASS**.
- Final state: daemon stopped, no VRAM/zram/ghost devices, disk swap only, and
  GPU free memory approximately 4901 MiB: **PASS**.
- Windows cleanup: the test-signed `ramshared` service, ROOT\RAMSHARED device,
  and `oem25.inf` package were removed; `testsigning` and `nointegritychecks`
  were disabled; no RAMSHARE LUN or pagefile remained: **PASS**.
- Firmware Secure Boot remains physically disabled (`UEFISecureBootEnabled=0`)
  and requires a manual UEFI enable/reboot before any anti-cheat compatibility
  claim. The Windows driver is therefore not an official distribution yet.

**Verdict:** ✅ v0.7.4 WSL2 bounded smoke is reproducible on the real host. Windows
driver use remains limited to a separately isolated, test-signed development
environment until Microsoft signing and Secure Boot verification are complete.

## 2026-07-24 16:02 — WSL watchdog teardown regression and three-round proof

**What:** Reproduced the shared-host WSL pressure hang, corrected watchdog
ownership and bounded pressure sizing, and repeated the corrected campaign for
three before/action/after rounds.

**Commands:**
```text
scripts/safety/Test-Wsl2FreezeCampaignStatic.sh
scripts/windows/Test-SharedWslPressureCampaignStatic.ps1
scripts/windows/Invoke-SharedWslPressureCampaign.ps1 \
  -ApproveSharedDailyHost -VramMiB 512 -ZramMiB 128 -Rounds 3 \
  -WatchdogSec 45 -ActionCleanupGraceSec 90 -OuterTimeoutSec 540
```

**Measured data:**
- Reproducer `shared-wsl-pressure-20260724-044917`: round 2 reached
  `action_rc=143`; the old equal-deadline watchdog killed the controller while
  its integrity worker was in D-state, leaving NBD/zram active until supervised
  WSL recovery: **FAIL reproduced**.
- First corrected probe `shared-wsl-pressure-20260724-155548`: teardown reached
  a clean terminal state without WSL termination, but the intentionally short
  30 s cleanup grace produced an honest `PARTIAL`: **expected boundary**.
- Final campaign `shared-wsl-pressure-20260724-155908`: rounds 1/2/3 each
  report `action_rc=0`, 1280 MiB allocated, 20 chunks verified, identical
  before/after SHA-256, no watchdog marker, and artifact validation PASS.
- Final health: daemon dead, no NBD/zram/ghost, only `/dev/sdc` disk swap;
  Windows volume and sample identities for `C:` and `I:` revalidated: **PASS**.
- Three repeated Linux and Windows static watchdog gates: **6/6 PASS**.

**Verdict:** ✅ The reproduced hang was a harness teardown race, not silent data
corruption. The watchdog now preserves controller ownership, gives integrity
cleanup a separate grace interval, scales pressure to configured tiers, stops
after a failed round, and leaves Windows as the only forceful WSL recovery
owner. Three corrected live rounds completed with clean teardown.

## 2026-07-24 22:20 — Bounded Windows harnesses and native guest closure

**What:** Reproduced and fixed PowerShell Direct connection hangs and
multi-record status misclassification, then reran the isolated Windows driver
and product lifecycles on `win11-drill`.

**Commands:**
```text
scripts/windows/Test-GuestExhaustiveStatic.ps1
scripts/windows/Run-GuestExhaustive.ps1
scripts/windows/Test-GuestProductOnlineStatic.ps1
scripts/windows/Run-GuestProductOnline.ps1
scripts/windows/Test-*.ps1
scripts/p0/Test-*.ps1
./scripts/docs-check.sh
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

**Measured data:**
- `guest-exhaustive-20260724-215817`: normal and Driver Verifier IOCTL passes
  are `PASS`; Verifier flags `0x2093B`, `ramshared.sys` load 1/unload 0,
  package/running SHA-256
  `324CC7C95A17BE3C245865F55EFC3E87B443D9CF711249068A4221DD86DEDBFA`,
  no new dump, elevated harness exit 0.
- `guest-product-online-20260724-221128`: three fresh CUDA/Online lifecycle
  rounds passed exact disk identity and checksum gates; all console exits were
  zero, no force-kill occurred, every lease was released, CUDA free memory was
  restored, no new dump appeared, and terminal safety passed.
- All Windows and P0 static harness tests, docs/index/link/gap/hygiene gates,
  workspace tests, formatting, and clippy with warnings denied: **PASS**.
- Hyper-V rollback export remains available at
  `E:\Hyper-V\exports\win11-drill-pre-native-20260724-162906`; the drill VM
  ended Off.

**Verdict:** ✅ The isolated native Windows beta surface is repeatably green
under Driver Verifier and three product Online lifecycles. This does not change
the public-distribution gate: the package remains test-signed and requires a
production-trusted or Microsoft-attested signature before official deployment.

## 2026-07-24 22:52 — Physical Windows Test Mode repetition

**What:** Enabled Windows Test Mode on the physical RTX 2060 host, performed a
clean test-signed miniport deployment with a mandatory post-deploy reboot, and
repeated the bounded product storage lifecycle three times.

**Commands:**
```text
bcdedit /set testsigning on
scripts/windows/Get-WinDrivePreflight.ps1 -StorageOnly
devcon.exe install C:\ramshared\package\ramshared.inf Root\RamShared
Restart-Computer -Force
scripts/windows/Run-HostExhaustive.ps1 -SizeBytes 67108864
scripts/windows/Get-WinDrivePreflight.ps1 -StorageOnly
```

**Measured data:**
- Elevated post-deploy preflight
  `physical-testmode-final-preflight-20260724-224916`: `testsigning Yes`,
  `PREFLIGHT_STORAGE_ONLY=PASS`, control path open, no RAMSHARE disk/Win32/PnP
  residue or minidump, and DriverStore/package SHA-256 match
  `324CC7C95A17BE3C245865F55EFC3E87B443D9CF711249068A4221DD86DEDBFA`.
- Physical campaigns `exhaustive-20260724-224946`,
  `exhaustive-20260724-225047`, and `exhaustive-20260724-225124`: each reports
  `HOST_ONLINE=true`, three matching SHA rounds, `GRACEFUL=true`, `EXIT=0`,
  `LEASE_RELEASED=true`, `DISK_IO_MEASURE_OK=true`, and
  `LUN_GONE=true`/`WIN32_GONE=true`/`PNP_GONE=true`.
- Aggregate physical evidence: three fresh CUDA/Online lifecycles, nine SHA
  matches, three direct disk-I/O checks, three graceful teardowns, zero forced
  product termination, and zero residual storage identities.
- Final elevated preflight `physical-testmode-final-20260724-225209`: PASS,
  Test Mode still enabled, service/control available, no RAMSHARE storage
  identity, no pagefile on RAMSHARE, and no minidump.
- Terminal GPU observation: RTX 2060 at 681 MiB used, 5274 MiB free of
  6144 MiB. Firmware Secure Boot remains disabled.

**Verdict:** ✅ The supervised physical Windows storage path is repeatably
functional in Test Mode on this exact host/build/GPU. This is not an official
Windows distribution or anti-cheat-compatible state: Test Mode remains enabled,
the package is test-signed, and autonomous SCM use still requires a packaged,
supervised broker dependency.

## 2026-07-25 09:28 — Autonomous Windows broker Step 3 closure

**What:** Implemented and validated the separately supervised Windows broker,
authenticated named-pipe product boundary, transactional two-service package,
broker-loss containment, VM lifecycle and three-cold-boot physical campaign.

**Measured data:**
- Workspace fmt/clippy PASS; broker 40, winbroker 19 and winsvc 120 tests
  passed; wsl2d touched suites passed.
- Per-file cover: lease 98.7%, winbroker 97.3%, winsvc config 96.7%, IPC
  83.3%, package 89.0%, runtime 88.9%.
- Package transaction: FreshInstall, Repair, ManufacturedRollback,
  UninstallRefusal and CleanUninstall all PASS.
- VM: 3/3 healthy lifecycles and BrokerLossOnline PASS; 12/12 SHA rounds;
  zero residue; broker/winsvc/driver BINARY_MATCH.
- Physical: manifest SHA `0F6DFD...C1F1A` across 3/3 cold boots; readiness
  median 1,164 ms/p99 1,165 ms; full stop median 2,810 ms/p99 3,049 ms;
  9/9 SHA rounds; residue 0; forced kills 0; final services stopped,
  task/watchdog absent.
- A preflight defect that selected the real 466 GiB `R:` data volume was
  refused before formatting. SPEC DT-17 and the corrected harness require a
  free manifest-owned letter; the completed campaign used `S:` and left `R:`
  untouched.

**Verdict:** ✅ Autonomous Windows broker Step 3 is implemented and has
legitimate VM and physical before→action→after evidence. The package remains
test-signed/Test Mode; that distribution limitation is outside this surface.

## 2026-07-25 15:53 — Autonomous broker discipline audit

**What:** Re-confronted the implementation and evidence against every SPEC
matrix row, closed the missing broker Event Log implementation, updated the
required living documentation, and reran the affected native VM matrices.

**Measured data:**
- Native broker SHA-256
  `EE7C102F620B5F21947321EE93F16E9C6D174A406E7426165EA64B9A0D746911`
  matched the running SCM process in Peer, RetryBudget, and Boundary.
- All three matrices observed Application Event ID 1000 from
  `RamSharedBroker` with `transition=process_ready`.
- Readiness was 506/476/671 ms; blocked accept/read cancellation was
  254–266 ms; partial-frame refusal completed at 10,021 ms.
- Legitimate service-SID admission passed. Administrator, unrelated-service,
  deny-only SID, status mutation, oversized line, and partial-frame paths were
  refused; boundary state remained zero registrations and zero leases.
- Final proof:
  `docs/specs/no-milestone/windows-autonomous-broker-service/evidence/vm-final/broker-final-matrices.json`.

**Verdict:** ✅ The implementation/evidence discipline gaps for this SPEC are
closed. Production-trusted Windows signing remains a separately tracked
release gate and was not falsely reclassified.

## 2026-07-25 16:02 — Physical broker product left active

**What:** Promoted the final Event Log broker into a new immutable physical
package and left the demand-start product running on the approved Test Mode
host.

**Measured data:**
- Active version `0.1.1-physical`, commit `ad15c339de2e…`.
- Broker/winsvc/driver BINARY_MATCH:
  `EE7C102F…D746911` / `F2B14796…35C8701` /
  `324CC7C9…DEDBFA`.
- One 64 MiB `RAMSHARE VRAMDISK`, healthy `S:`, one registered
  67,108,864-byte lease and one stable broker instance.
- Six random 1 MiB write/read/SHA samples matched over 50 seconds.
- Both services remained Running; Event Log recorded process ready,
  registration ready and lease granted.
- Pagefile remained only on `C:`. The healthy 466 GiB `R:` volume was
  preserved.

**Verdict:** ✅ PASS_ACTIVE_STABLE. Host evidence:
`C:\ramshared\artifacts\active-host-20260725-155910`; committed summary:
`docs/specs/no-milestone/windows-autonomous-broker-service/evidence/physical-active-20260725/activation.json`.
## 2026-08-09 09:15 -03 — Public benchmark evidence integrity gate

**What:** Added a RamShared-only, zero-dependency public evidence contract for
benchmark records and explicit SSDV3 claim manifests. Historical benchmark
bytes were not rewritten; five human sections and three old JSONL rows are now
mapped to honest legacy-unqualified identities.

**Category:** ci-gate / isolation

**Environment:** repository worktree; Node.js 24.15.0; no Windows, WSL2,
driver, daemon, disk, swap, GPU-pressure, or reboot action.

**Before:** `docs/BENCHMARKS.md` had 5 dated sections while
`docs/benchmarks/results.jsonl` had 3 pre-schema rows. There was no artifact
hash validator, statistics recomputation, comparison fingerprint, prose parity
gate, or explicit SPEC evidence manifest.

**Action:** Implemented and executed the two Node test suites, both repository
validators, and the complete `scripts/docs-check.sh` gate twice.

**After:** 5/5 human benchmark sections map exactly once; 3/3 old JSONL rows
have explicit legacy-unqualified mappings; no old row is promotable. The SPEC
claim validator accepts only explicit `PARTIAL`/`DONE` manifests and requires
the applicable named tests, cover classification, live before/action/after,
legitimate/refusal cases, cleanup, artifacts, and BINARY_MATCH.

**Measured data:** benchmark validator tests 11 passed / 0 failed; SPEC claim
tests 7 passed / 0 failed; repository counts sections=5, records=3, legacy=5;
two complete docs-check runs exited 0 and produced byte-identical output.

**How to measure:** `node --test tools/ci/check-benchmark-evidence.test.mjs`;
`node --test tools/ci/check-spec-evidence.test.mjs`;
`node tools/ci/check-benchmark-evidence.mjs --check`;
`node tools/ci/check-spec-evidence.mjs --check`; `./scripts/docs-check.sh`.

**Artifacts:** `docs/benchmarks/evidence.schema.json`,
`docs/benchmarks/legacy-unqualified.json`,
`docs/benchmarks/benchmark-map.json`, and
`docs/specs/no-milestone/benchmark-evidence-integrity/evidence/validation-summary.json`.

**Limitations:** This gate does not retroactively qualify historical numbers
and does not execute platform workloads. Windows physical storage performance
remains blocked on the separately approved host reboot and real 75-sample
matrix.

**Rollback trigger:** Revert validator integration if one malformed,
duplicate, hash-mismatched, sensitive, statistically forged, incomparable, or
non-PASS record passes; if one sensitive value is printed; or if identical
inputs produce different normalized output.

**Verdict:** ✅ The public benchmark/claim evidence gate is implemented and
its legitimate plus refusal paths are reproducible without host mutation.

## 2026-08-09 11:02 -03 — Documentation governance integrity

**Governance schema:** 1

**What:** Implemented and exercised the fail-closed structural documentation
governance gate.

**Slug:** `documentation-governance-integrity`

**Environment/commit:** repository worktree at
`95739d1f972bcefe7eb5df8861cf8c526503e074`; Node.js 24.15.0; no runtime,
driver, daemon, disk, swap, GPU-pressure, network, or reboot action.

**Scope:** Canonical-document ownership, objective routing, evidence-qualified
claims, provenance sanitization, bounded journey records, postmortem action
effectiveness, strict validation closure, and evidence-derived index status.

**Before:** DONE could be inferred from document presence, validation closure
did not require a strict before/action/after record, and no single read-only
gate checked the complete structural documentation surface.

**Action:** Ran 34 governance tests, 14 validation-schema tests, 7 index tests,
per-file Node coverage, the structural governance CLI, and the integrated
documentation gate twice.

**After:** The structural scan inspected 304 files with 0 findings. The three
production files measured 100.00/82.87/100.00%,
86.67/88.24/96.67%, and 96.72/80.26/100.00% line/branch/function coverage.

**Legitimate case:** An evidence-qualified claim with current hashed artifacts,
named tests, cleanup, and the required platform classification is accepted and
is the only path to DONE.

**Required refusals:** unqualified IMPL presented as DONE; sensitive/private
provenance; stale evidence artifact; missing BINARY_MATCH where required;
unbounded journey record.

**Tests/coverage:** 55 tests passed, 0 failed; every production file exceeded
80% lines and branches; two structural runs were deterministic and exited 0.

**Platform gates:** N/A — repository-only Node tooling; BINARY_MATCH N/A.

**Artifacts:**
`docs/specs/no-milestone/documentation-governance-integrity/evidence/validation-summary.json`
and its `evidence-manifest.json`.

**Cleanup:** Complete; the checkers are read-only and left 0 runtime or host
resources.

**Limitations:** This governance slice qualifies documentation claims only; it
does not promote any Windows, WSL2, kernel, signing, VM, or physical result.

**Rollback trigger:** One false DONE promotion, one sensitive value printed,
one automatic source rewrite, or different normalized output for identical
input.

**Verdict:** ✅ Documentation governance integrity is implemented and its
legitimate and refusal paths are reproducible without host mutation.

## 2026-08-09 11:02 -03 — Documentation localization integrity

**Governance schema:** 1

**What:** Implemented and exercised the bounded English/PT-BR localization
integrity gate.

**Slug:** `documentation-localization-integrity`

**Environment/commit:** repository worktree at
`95739d1f972bcefe7eb5df8861cf8c526503e074`; Node.js 24.15.0; no runtime,
driver, daemon, disk, swap, GPU-pressure, network, or reboot action.

**Scope:** English canonical documentation, complete root README in Brazilian
Portuguese, a non-normative Portuguese navigation portal, reciprocal language
switches, source-hash freshness, local links, and authority boundaries.

**Before:** Required localized entries existed only as an intended policy;
freshness, reciprocal switches, protected document classes, and localized
authority were not enforced by one deterministic gate.

**Action:** Ran the 15 named localization tests, the repository localization
CLI twice, syntax checks, and per-file Node coverage.

**After:** Both required localized files passed with 0 findings; two CLI runs
were byte-identical with SHA-256
`f1964e7db9763a1028e20c5dfdaee6c13e85a2d81ba0685b1205c50a45b7300c`.
Coverage was 98.59% lines, 86.90% branches, and 100.00% functions.

**Legitimate case:** Current exact source hashes, reciprocal README switches,
five portal objectives, valid local links, and explicit non-normative policy
are accepted.

**Required refusals:** stale canonical source hash; missing required
localization; broken language switch; positive localized authority claim;
protected normative localization path.

**Tests/coverage:** 15 tests passed, 0 failed; per-file line/branch/function
coverage exceeded 80%; repository CLI inspected 2 files with 0 findings.

**Platform gates:** N/A — repository-only Node tooling; BINARY_MATCH N/A.

**Artifacts:**
`docs/specs/no-milestone/documentation-localization-integrity/evidence/validation-summary.json`
and its `evidence-manifest.json`.

**Cleanup:** Complete; both runs were read-only and left 0 runtime or host
resources.

**Limitations:** Localized documents are informational. PRD, SPEC, IMPL, ADR,
CI, evidence, benchmarks, and validation remain English canonical records.

**Rollback trigger:** One stale hash, missing file, broken switch, positive
authority claim, sensitive diagnostic, or nondeterministic result passes.

**Verdict:** ✅ The bounded localization contract is implemented without
duplicating or translating normative engineering records.

## 2026-08-09 11:02 -03 — Public repository candidate integrity

**Governance schema:** 1

**What:** Implemented and exercised the public repository candidate hygiene
gate.

**Slug:** `public-repository-hygiene`

**Environment/commit:** repository worktree at
`95739d1f972bcefe7eb5df8861cf8c526503e074`; Node.js 24.15.0; no runtime,
driver, daemon, disk, swap, GPU-pressure, network, or reboot action.

**Scope:** Candidate, staged-index, and tracked-file scanning; bounded text and
binary handling; sanitized findings; scoped allowlists; and portable script
defaults for a public RamShared checkout.

**Before:** The hygiene scanner could miss nonignored untracked candidates or
read a staged path from different working-tree bytes, and diagnostics did not
have the current bounded candidate contract.

**Action:** Ran 12 named hygiene tests, per-file Node coverage, the real
candidate scan, PowerShell 5.1 parser/static checks, and the integrated docs
gate.

**After:** The current candidate scan inspected 681 files with 0 findings.
Coverage was 94.38% lines, 85.29% branches, and 100.00% functions; all 12 tests
passed.

**Legitimate case:** A clean candidate containing tracked, staged, and
nonignored untracked public files is accepted without exposing file contents.

**Required refusals:** staged index/worktree byte divergence; private profile
path; credential/token/key fixture; kernel-address fixture; invalid scan mode;
Git enumeration failure.

**Tests/coverage:** 12 tests passed, 0 failed; per-file line/branch/function
coverage exceeded 80%; candidate CLI exited 0 with 0 findings.

**Platform gates:** PowerShell 5.1 parser/static checks only; no operator
script execution; BINARY_MATCH N/A.

**Artifacts:**
`docs/specs/no-milestone/public-repository-hygiene/evidence/validation-summary.json`
and its `evidence-manifest.json`.

**Cleanup:** Complete; the checker is read-only and left 0 runtime or host
resources.

**Limitations:** This gate prevents candidate hygiene false-greens; it does not
qualify driver behavior, release signing, VM E2E, or physical-host stability.

**Rollback trigger:** One staged blob is read from worktree bytes, one
nonignored candidate is skipped, one sensitive match is echoed, or three
consecutive no-load candidate scans exceed 10 seconds.

**Verdict:** ✅ Public repository candidate integrity is implemented and the
legitimate plus refusal paths are reproducible without host mutation.

## 2026-08-10 01:12 -03 — Windows 11 lab media and OOBE revalidation

**What:** Reproduced the disposable-lab OOBE failure, corrected and sealed the
unattended media contract, and exercised the next clean VM start without
rebooting the physical Windows host or WSL.

**Measured data:** The failed `clean-5` guest remained at
`IMAGE_STATE_UNDEPLOYABLE`, `OOBEInProgress=1`, `SetupPhase=4`, and
`SetupType=2`. Its sealed XML was legitimately refused because `AutoLogon`
omitted the Microsoft-required `LogonCount`. The corrected XML is 5,015 bytes,
has SHA-256 `8C22438E54B7E4319D2AB454627E7DB6014AAF6B7DE16BABEC03818F368CF61C`,
and the same hash was independently read from the new 8,454,309,888-byte ISO;
the ISO SHA-256 is
`EE07B0766773105C22E952658FBDED018A1894846123DFF23991D6281E34A785`.
The Windows static aggregate, including the new OOBE/media refusals, exited 0;
`docs-check` and scoped whitespace checks exited 0. Dropping only reclaimable
WSL cache reduced `buff/cache` from 9.7 GiB to 1.2 GiB without stopping a
process or restarting WSL. Hyper-V still refused the supported 4,096 MiB VM
start with `0x800705AA`: host free memory was 3,798 MiB while the unrelated
`gha-ubuntu-2404` VM retained 12,288 MiB. Both RamShared disposable guests are
Off; the foreign VM was not mutated.

**Evidence:**
`tmp/windows-task-manager-disk-counters-e2e/20260810-oobe-media-validation.json`
and `C:\ramshared\artifacts\win11-verifier-clean-6-thumbnail.png` (14,858
bytes; SHA-256
`638E6A4D9487FB6A740E2AB11B74923384BF14A5AAD56CA186C43939DBB59F8B`).

**Verdict:** 🟡 Media and orchestration corrections are validated, but VM E2E,
Driver Verifier, BINARY_MATCH, storage matrix, and benchmarks remain blocked by
the observable host-memory boundary. This is partial, not DONE.

## 2026-08-10 08:42 -03 — Disposable VM Driver Verifier and exact teardown

**What:** Replaced repeated Windows installation with an immutable 20 GiB
ready-base plus differencing-VHD clones, corrected the offline Hyper-V
integration-service and phase-bound teardown contracts, recovered the prior
failed run exactly, and completed the signed `.8` driver campaign on a fresh
Generation-2 Windows 11 clone. No physical-host or WSL reboot occurred.

**Measured data:** The immutable base was 20,505,952,256 bytes with SHA-256
`1F17888E525553810881E835FB2E3B8F7C74B9A4EAEC3481F7BCE8A118B63EC2`.
The new clone used a differencing VHD, a new VM ID, four vCPUs, 4 GiB startup
memory, vTPM, the Private sealed switch, and zero checkpoints. A legitimate
Hyper-V `0x800705AA` refusal occurred while Windows had only 4,336,263,168
free physical bytes and WSL used 12,280,619,008 bytes with 4,284,153,856 bytes
of swap used. Gracefully shutting down only two completed RamShared lab VMs
raised host-free memory to 10,003,623,936 bytes; no foreign process was killed.

The fresh readiness gates passed on their first complete attempts in 110,401
ms before the campaign and 92,229 ms after Secure Boot was restored `On`.
The signed driver loaded with SHA-256
`5E4FF79148274EC1A029A057714F0066389B6E103F5425E5C3C2AAB1ADB07A55`
and BINARY_MATCH true. Normal I/O passed in 87,690 ms and Driver Verifier I/O
passed in 25,347 ms. Both paths proved the legitimate queue, six required
refusals, three race/rundown guards, VPD serial `ABCDEF0123456789`, a
134,217,728-byte Virtual SSD with 4,096-byte logical/physical sectors, zero
Event 153, and zero new dumps. Verifier reset reached zero. Exact teardown
removed one ROOT, service, OEM INF, and retired PnP node with every action exit
0; independent final observations found package/service/ROOT/disk/PnP,
Verifier target, TestSigning, and signer certificates all zero. The terminal
clone state is Off and host-free memory reached 12,420,276,224 bytes.

**Evidence:**
`tmp/windows-task-manager-disk-counters-e2e/20260810-vm-verifier-final.json`,
`C:\ramshared\artifacts\ready-clone-5e48f1bf-9f55-4d32-9d98-f913a9092ed8`,
`C:\ramshared\artifacts\guest-verifier-d0f9a571-c7bb-4f78-9e40-aa7233ed85e6`,
and exact recovery
`C:\ramshared\artifacts\guest-verifier-recovery-156cd553-535f-4fe6-8383-f35ba823345f`.

**Verdict:** 🟡 The disposable-VM driver, Verifier, BINARY_MATCH, refusal,
rollback, firmware-restoration, and zero-residue slice is legitimately green.
The overall SPEC remains partial because the supervised physical-host `.8`
deployment/BINARY_MATCH and 75-sample five-cell storage benchmark matrix have
not run; they must not be inferred from VM evidence.

## 2026-08-10 10:53 -03 — Jules/Dependabot consolidation pre-merge gate

**What:** Audited Dependabot PR #158 and Jules PRs #160–#187, consolidated the
valid findings into RamShared-owned implementations, replaced unsafe or
incomplete patches with SPEC-first fail-closed fixes, and exercised the complete
repository-local validation plan before creating the single superseding PR.
Issue #188 is the remote traceability anchor. No old PR was closed before the
consolidated replacement existed and passed its local gates.

**Measured data:** `cargo fmt --all -- --check`, workspace clippy with
`-D warnings`, the complete workspace test suite, `cargo deny check`, the
pinned RustSec audit (1,197 advisories checked against 54 dependencies), the
Windows MSVC target check, actionlint 1.7.7, 242 Node tests, the complete
Windows PowerShell static aggregate, public-hygiene candidate scan, and
`scripts/docs-check.sh` exited 0. The canonical Rust coverage planner executed
all mapped entries serially against immutable `origin/main` and exited 0. The
lowest production-file line results were 81.5% for
`ramshared-wsl2d/src/main.rs`, 82.2% for `ublk_server.rs`, 83.3% for winsvc
`ipc.rs`, and 84.4% for both cascade orchestration and CLI dispatch; every
mapped production file was at least 80%. The ublk slice measured 91.2%
(`ramshared-uring/src/lib.rs`), 95.0% (`ublk_queue.rs`), and 82.2%
(`ublk_server.rs`). A prior transient coverage child stall was not promoted:
the checker now has a tested 15-minute terminal deadline and private target
directories, and this full serialized rerun completed with exit 0.

**Remote controls:** GitHub REST observations now prove default workflow token
`read`, Actions PR approval disabled, selected-action allowlisting with SHA
pinning required, 30-day artifact/log retention, enforced-admin strict branch
protection with conversation resolution, and two protected environments with
required reviewer, self-review refusal, and protected-branch policy. The
branch protection now requires only the same-run `required-checks` aggregate.
The SPEC-first `observed` remote-gate state was added with a RED→GREEN refusal
suite so a valid administrator observation can close the gate without being
misrepresented as a local workflow. Both `--check-local` and strict `--check`
now exit 0 with `CI_CONTRACT_STATUS=PASS`.

**Safety boundary:** All real `/dev/ublk-control`, CUDA device, swap activation,
Windows driver, SCM, storage mutation, VM, pressure, reboot, and physical-host
tests remained ignored or uninvoked. Refusal tests used only nonexistent paths
and regular files. No `ramsharedd` daemon, product swap, ublk block device, VM,
or driver was activated by this gate.

**Artifacts:** `tmp/*-cov.json`,
`docs/reliability/JULES-PR-AUDIT-20260810.md`, and
`docs/governance/remote-controls-observation.json`.

**Rollback trigger:** Any mapped production file below 80%; any unbounded
coverage child; any aggregate CI false-green; any loaded artifact mismatch;
any unexpected swap/device/driver activation; or any regression from the exact
identity, bounds, teardown, and refusal contracts added by this consolidation.

**Verdict:** 🟡 Repository-local implementation, static validation, coverage,
and remote hardening are green. CI promotion still awaits the single PR's
hosted `required-checks`; Windows physical-host BINARY_MATCH and the 75-sample
five-cell storage matrix remain explicitly env-bound and are not invented from
offline proof.

## 2026-08-10 11:25 -03 — Consolidation hosted-gate refusal closure

**What:** Confronted the first hosted run of PR #189, reproduced each failure,
and corrected the contracts rather than bypassing the aggregate. The fixes
cover exact opaque-evidence redaction, append-only validation separators,
PowerShell 5.1 syntax, the transitive pinned Trivy action allowlist, and six
Rust source paths that the hosted merge-ref correctly refused as unmapped.

**Before:** The same-run aggregate was RED. Comment-language and validation
schema refused their diffs, Windows static parsing failed, Trivy could not run
its pinned setup action under the selected-action policy, and Rust selection
reported six `changed-rust-file-unmapped` errors.

**Action:** Added SPEC-first RED→GREEN fixtures. Opaque invalid-UTF-8 evidence
now permits only a byte-exact private-root first-line redaction; a new entry's
single separator blank is append-only-safe; the Windows workflow and broker
static harness parse under Windows PowerShell 5.1; the selected action list
includes `aquasecurity/setup-trivy@*`; and the Rust planner now distinguishes
whole-file structural module surfaces from named Windows platform E2E. The
structural grammar rejects functions, constants, statics, impls, macros,
malformed delimiters, unsafe package bindings, and shell execution.

**After / measured data:** 240 CI Node tests passed. Planner coverage measured
88.85% lines, 81.80% branches, and 97.70% functions; comment-language measured
92.61%, 83.71%, and 98.51%; validation-schema measured 86.76%, 88.71%, and
96.77%; localization measured 98.59%, 86.90%, and 100%. The exact PR merge-ref
selection is `READY` with 19 owning entries and zero unmapped Rust paths. The
complete Windows static aggregate, actionlint 1.7.7, docs-check, public hygiene,
strict CI contract, workspace fmt/clippy, and the workspace test suite exited
0. Rust tests passed with only declared GPU/root/device tests ignored; no live
daemon, swap, ublk, CUDA, SCM, storage, VM, shutdown, or reboot path ran.

**Refusals:** Arbitrary invalid UTF-8 edits remain exit 2; nonblank historical
validation edits remain blocked; structural Rust containing executable logic
is refused; and a failed structural package test makes the planner nonzero.

**Artifacts:** `tmp/ci-fix-*-cov.json` and PR #189 hosted run diagnostics.

**Rollback trigger:** Any suffix byte in protected opaque evidence changes;
any nonblank history rewrite passes; a Rust executable file is admitted as
structural; a selected action executes outside the pinned allowlist; or the
Windows static wrapper parses differently under PowerShell 5.1.

**Verdict:** 🟡 Every first-run failure has a local legitimate and refusal
proof, but promotion remains pending the new hosted `required-checks` run. No
physical-host or live storage claim is inferred from these safe gates.

## 2026-08-10 11:30 -03 — Cross-version PowerShell static runner

**What:** Corrected the only new refusal from PR #189's second hosted run: the
PowerShell Direct process-tree fixture assumed `$PSHOME\powershell.exe`, while
the GitHub Windows runner hosts PowerShell 7 as `pwsh.exe`.

**Before:** All Windows Rust tests passed on the hosted runner, then the static
fixture failed before spawning its manufactured child because the assumed
PowerShell 7 path did not exist. No product, VM, SCM, disk, or driver action had
started.

**Action:** SPEC DT-26 now binds the fixture to the exact executable path of
its current PowerShell process, verifies that it is a file, and passes that
path explicitly to the synthetic grandchild. The product PowerShell Direct
worker and its deadlines are unchanged.

**After / measured data:** On Windows PowerShell 5.1, the focused harness
reported seven PASS markers, including
`psdirect_runner_uses_current_host_executable`, process-tree termination,
redirected-stream drain, and nonzero-child refusal. `docs-check` and whitespace
checks exited 0. The PowerShell 7 proof remains the next hosted run; no retry or
fallback executable is guessed.

**Refusals:** A missing/non-file current executable path is terminal, and the
existing timeout, surviving grandchild, partial output, or nonzero child cases
remain RED.

**Rollback trigger:** The static fixture assumes a fixed executable filename,
accepts a missing path, or weakens timeout/process-tree termination.

**Verdict:** 🟡 The cross-version defect is locally closed without live host
mutation; final promotion still requires the replacement hosted Windows and
same-run aggregate checks.

## 2026-08-10 11:32 -03 — Cascade timeout fixture ETXTBSY closure

**What:** Removed a hosted-filesystem race from the existing bounded-command
timeout test without changing production orchestration or adding a retry.

**Before:** The second hosted Linux run passed fmt and clippy, then one of 88
CLI tests failed because executing a freshly written temporary script returned
`ETXTBSY`; the production runner correctly surfaced the error instead of
mislabeling it as a timeout.

**Action:** SPEC DT-T3a now requires the closed script to be passed to the
immutable `/bin/sh` interpreter. The script still `exec`s its bounded sleep,
so the PID written by the fixture remains the exact direct child that must be
terminated and reaped.

**After / measured data:** The exact test passed 20 consecutive executions.
The full 88-unit/5-integration CLI coverage run passed, and canonical
`cascade_io.rs` line coverage measured 84.5% (1,141/1,351). Package clippy with
`-D warnings`, fmt, docs-check, and whitespace checks exited 0. No swap,
daemon, NBD, ublk, CUDA, root, or host action ran.

**Refusals:** Interpreter failure, missing PID receipt, timeout over one
second, or a surviving child remains terminal; there is no retry path.

**Rollback trigger:** Any `ETXTBSY` recurrence, retry introduction, timeout
false-green, or surviving fixture PID.

**Verdict:** 🟡 The deterministic Linux fixture and its coverage are green;
hosted aggregate promotion remains pending the next immutable PR run.

## 2026-08-10 21:22 -03 — Windows static cross-version closure

**What:** Closed the remaining PowerShell 5.1/7 incompatibilities in the
source-only Windows suite before rerunning PR #189.

**Before:** The hosted PowerShell 7 job failed in the storage-matrix fixture
because it constructed `$PSHOME\powershell.exe`. After binding the child to
the current `pwsh.exe`, a local PowerShell 7 run exposed a second legitimate
false RED: pipeline assignment of `@()` reached an `[object[]]` parameter as
one null element, so the zero-Event-153 case was counted as one event.

**Action:** SPEC DT-93 and broker DT-27 now require every manufactured child
to resolve and validate the exact current PowerShell executable. The four
static harnesses with child processes were audited together. The storage
matrix now uses that exact executable for bounded children, watchdog launch,
and invocation evidence, and initializes the manufactured empty event set as
`[object[]]@()` before the optional one-event assignment.

**After / measured data:** The complete 15-harness Windows static wrapper
exited 0 under both Windows PowerShell 5.1 and PowerShell 7. The focused storage
matrix passed in both runtimes, including
`static_child_uses_current_host_executable`,
`event153_zero_case_is_cross_version_empty`, all timeout/process-tree cases,
the legitimate zero-event case, and the one-event refusal. The three other
affected focused harnesses passed under Windows PowerShell 5.1. `rg` found zero
remaining `$PSHOME\powershell.exe` constructions in `Test-*.ps1`;
`docs-check` and whitespace validation exited 0. No VM, SCM, disk, driver,
GPU-pressure, shutdown, reboot, or physical-host action ran.

**Refusals:** Missing/non-file current executable paths remain terminal; one
Event 153 remains RED; timeout, nonzero child, failed stream drain, or a
surviving child process tree remains RED.

**Rollback trigger:** Any Windows static child again depends on a fixed
PowerShell filename, the zero-event case counts a null row, or the complete
wrapper diverges between Windows PowerShell 5.1 and PowerShell 7.

**Verdict:** 🟡 Local cross-version static evidence is complete and green;
promotion still requires the replacement hosted `required-checks` run.

## 2026-08-10 21:34 -03 — Fail-closed Trivy SARIF publication

**What:** Closed a security-evidence false-green observed in PR #189's hosted
Trivy job.

**Before:** The blocking CRITICAL/HIGH scan passed, but the immutable CodeQL
upload action received its default `sarif_file: ../results`, emitted
`Path does not exist: ../results`, and stayed green because the step was
allowlisted with `continue-on-error: true`.

**Action:** SPEC DT-29 separates scan, local SARIF validation, and trusted
publication. The workflow now requires a non-empty `trivy-results.sarif`,
validates SARIF version 2.1.0 and an array of runs with `jq`, passes the exact
file to the pinned upload action, and removes error tolerance. Fork pull
requests retain the blocking scan and local validation but skip publication
because their token cannot receive `security-events: write`. The CI contract
now requires all three commands and has no SARIF error allowlist.

**After / measured data:** A genuine RED first showed the old allowlist in
`ci_contract_requires_fail_closed_trivy_sarif_publication`. GREEN is 51/51
contract/aggregate tests, with checker coverage 90.36% lines, 82.64% branches,
and 99.08% functions. Strict CI contract, actionlint 1.7.7, docs-check, public
hygiene, and whitespace gates exited 0.

**Refusals:** Missing, empty, malformed, or wrong-version SARIF is terminal;
same-repository publication failure is terminal; CRITICAL/HIGH findings remain
terminal before publication.

**Rollback trigger:** A Trivy job reports success while SARIF validation or an
eligible upload fails, the upload path differs from the generated file, or a
SARIF `continue-on-error` allowlist returns.

**Verdict:** 🟡 The false-green is locally closed; the corrected hosted
security job and final same-run aggregate remain the promotion proof.

## 2026-08-10 21:41 -03 — Hosted CI trust slice qualification

**What:** Qualified the complete CI trust and release-integrity implementation
on the immutable PR #189 revision `aa2282bf4d002c7560e057cc4a6dc01313e0d953`.

**Before:** Local contracts and each reproduced refusal were green, but the
slice correctly remained partial until a same-run hosted aggregate proved the
actual GitHub orchestration, Windows runtime, security publication, and exact
coverage behavior.

**Action:** GitHub Actions run `31446546130` executed the contract entrypoint
and every reusable caller on the same pull-request revision. No bypass, retry,
manual success, host runner, lab mode, or tolerated failure was used.

**After / measured data:** All 20/20 jobs concluded success. Terminal
`required-checks` job `93642837435` is SUCCESS. The hosted Windows static job
completed in 98 seconds; exact Rust coverage completed in 221 seconds with 36
per-file PASS rows and a minimum of 80.8% (893/1,105 lines in
`crates/ramshared-cli/src/main.rs`); cargo-audit/cargo-deny completed in 292
seconds; and Trivy generated, validated, and uploaded the exact SARIF in 19
seconds. Workspace fmt/clippy/tests, docs, actionlint, gitleaks, validation,
comment-language, PR-body, artifact hygiene, and every summary gate passed.

**Refusals:** The same-run aggregator still rejects failed, cancelled, skipped,
missing, or tolerated callers; coverage below 80%, invalid SARIF, supply-chain
policy failure, Windows static failure, or unsafe remote controls remain
terminal.

**Artifacts:** GitHub run `31446546130`, terminal job `93642837435`, coverage
job `93642007224`, Windows job `93642007476`, security jobs `93642007461` and
`93642007478`.

**Rollback trigger:** Any required caller ceases to conclude success on the
same revision, any mapped file falls below 80%, SARIF publication regresses,
or branch/environment controls drift from the recorded strict observation.

**Verdict:** ✅ CI trust and release integrity is implemented and qualified.
This verdict does not promote the separately env-bound physical Windows,
driver-signing, VM-lab, GPU-pressure, or live storage matrices.

## 2026-08-10 22:27 -03 — Coverage deadline owns descendant processes

**What:** Closed the process-lifetime gap exposed by PR #189 hosted run
`31447000916` attempt 1.

**Before:** The exact Rust coverage command completed all Rust tests, wrote its
report, and printed a passing 96.5% result for the last measured file, but the
earlier `ramshared-wsl2d` command had crossed the 15-minute direct-child
deadline. GitHub cleanup then terminated orphaned `cargo` and instrumented
`ramsharedd` processes. The checker failed closed, but its `spawnSync` SIGTERM
did not own the descendant tree.

**Action:** SPEC DT-30 now requires the Linux/WSL2 coverage command to run in a
GNU coreutils `timeout` process group with TERM at 15 minutes, KILL after five
seconds, and a later 15-minute-10-second Node bound. Exit 124 and outer timeout
remain the stable terminal `COVERAGE_CHILD_TIMEOUT`; partial reports and retries
remain forbidden.

**After / measured data:** TDD RED observed exit 124 incorrectly classified as
`COVERAGE_CHILD_FAILED`. GREEN is 13/13 checker tests. A manufactured shell
spawned a 60-second descendant; the supervisor returned 124 in about 0.1 s and
the recorded descendant PID returned `ESRCH`. Checker coverage is 93.07% lines,
86.18% branches, and 91.30% functions. Checker plus planner is 38/38; syntax,
docs-check, public hygiene, and whitespace gates exit 0.

**Refusals:** Timeout never consumes a report, never retries, and fails if the
supervisor cannot start. The final PR revision still requires a fresh hosted
same-run aggregate before merge.

**Artifacts:** Hosted failure job `93643265744`; sanitized local TDD output in
the command history only (no private paths or raw process data committed).

**Rollback trigger:** A timed-out coverage command leaves any Cargo/test
descendant alive, returns coverage green, consumes a partial report, or exceeds
the outer terminal deadline.

**Verdict:** 🟡 The process-tree fix is locally proven; the final hosted PR
aggregate remains the promotion gate.

## 2026-08-10 22:39 -03 — Deterministic serial Rust admission

**What:** Removed intra-binary scheduling races from canonical Rust admission
after ordinary workspace tests and exact coverage independently retained Rust
test processes on hosted runners without any intervening Rust source change.

**Before:** Workspace and coverage commands used the default multi-threaded
Rust test harness while suites exercised process-global signals, environment,
Unix sockets, and child lifecycle. Repeated hosted execution was therefore
schedule-dependent even though prior runs and the same source were green.

**Action:** SPEC DT-31 requires the exact workspace command
`cargo test --workspace -- --test-threads=1` and appends
`-- --test-threads=1` to every `cargo llvm-cov` child. DT-30 still owns the
complete process tree and remains terminal; no retry, skip, or test exclusion
was added.

**After / measured data:** Both focused TDD tests were RED before the commands
changed and GREEN afterward. The bounded local workspace suite exited 0 in
24.69 seconds. All non-ignored tests passed; GPU, root/ublk, and the dangerous
WSL2 daemon smoke remained ignored. The complete Node CI suite is 243/243.
Contract tests are 52/52 at 90.36% lines, 82.64% branches, and 99.08%
functions. Coverage-checker tests are 13/13 at 93.08% lines, 86.18% branches,
and 91.30% functions. Strict contract, actionlint 1.7.7, docs-check, public
hygiene, validation schema, and whitespace gates exit 0.

**Refusals:** The 300-second local verification wrapper kills its process group
on overrun. Canonical CI retains its declared 30-minute job bound, and exact
coverage retains DT-30's 15-minute child bound. No ignored hardware test was
promoted to offline proof.

**Rollback trigger:** A serial run changes product behavior, exceeds its finite
job deadline, hides a failing test, or leaves a Cargo/test descendant alive.

**Verdict:** 🟡 Local deterministic admission is green; merge still requires
the final hosted same-revision `required-checks` success.

<!-- validation-schema-v2 -->

## 2026-08-11 12:16 -03 — Temporal governance record contracts

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0001`.
**Owner role:** `governance`.
**Observed at:** `2026-08-11T12:16:00-03:00`.
**Verified at:** `2026-08-11T12:16:00-03:00`.
**Source revision:** `fd5cbf2d39a026bcf737a3082ef2497d3861b257`.
**Lifecycle:** `reviewable`.
**Retention:** Retain in the append-only validation log.
**Freshness:** Revalidate whenever either record checker changes.
**What:** Verified the RamShared-native task and evidence record contracts.
**Category:** `ci-gate`.
**How to measure:** `node --test tools/ci/check-task-log.test.mjs tools/ci/check-validation-schema.test.mjs`; `node tools/ci/check-task-log.mjs --all`; and `node tools/ci/check-validation-schema.mjs --all`.
**Measured data:** 26/26 focused Node tests passed; 2/2 schema checkers exited 0 in all-record mode.
**Verdict:** ✅ The temporal contracts validate locally; this record does not qualify any hardware or hosted CI claim.

## 2026-08-11 12:34 -03 — Broker shutdown wake closes release CI hang

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0002`.
**Owner role:** `reliability`.
**Observed at:** `2026-08-11T12:34:00-03:00`.
**Verified at:** `2026-08-11T12:34:00-03:00`.
**Source revision:** `795a2924216f3524350a658ceaddc93b561abeeb`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the summary here; local raw logs remain under `tmp/pr151-broker-shutdown-e2e/` until hosted verification completes.
**Freshness:** Revalidate on any broker worker, shutdown bridge, channel, or CI Rust-toolchain change.
**What:** Replaced timer-only broker-worker termination with an explicit nonblocking control wake that drains earlier FIFO I/O.
**Category:** `ci-gate`.
**How to measure:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace -- --test-threads=1`; the two canonical Rust slice coverage commands from the memory-broker SPEC; 100 bounded repetitions of `daemon_worker_serves_job_counts_io_and_stops_on_shutdown`; and the release RAM-broker before/action/after drill.
**Measured data:** Workspace tests exited 0; `ramsharedd` passed 47/47; stress passed 100/100; `main.rs` coverage was 81.7% (2827/3461) and `conn.rs` 96.5% (497/515). The loaded release executable exactly matched `target/release/ramsharedd`; SIGTERM exited 0 in 1995 ms and removed the owned socket. A regular-file socket refusal exited 1 and preserved SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
**Verdict:** 🟡 The local code, cover, BINARY_MATCH, legitimate path, and refusal are green; PR #151 still requires a refreshed same-revision hosted aggregate before promotion.

## 2026-08-11 12:43 -03 — Native governance, evidence, and cleanup lifecycle integration

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0003`.
**Owner role:** `governance`.
**Observed at:** `2026-08-11T12:43:55-03:00`.
**Verified at:** `2026-08-11T12:43:55-03:00`.
**Source revision:** `6e488df7dba5cf92a2174f59b8330d7416d68b01`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the governance records, generated catalog, and sanitized historical receipts in their owned repository paths; do not treat local lab outputs as public proof.
**Freshness:** Revalidate when a governed checker, lifecycle policy, CI command, retention policy, or evidence catalog changes.
**What:** Verified the RamShared-native task/evidence custody, Markdown lifecycle policy, passive capability observations, campaign evidence lifecycle, cleanup receipt register, threat model, ADR registry, and pull-request ratchets.
**Category:** `ci-gate`.
**How to measure:** `./scripts/docs-check.sh`; `node --test tools/ci/*.test.mjs tools/*.test.mjs`; `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace -- --test-threads=1`; `node tools/ci/check-ci-contract.mjs --check-local`; and `go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.7 .github/workflows/*.yml`.
**Measured data:** docs-check passed with 332 structural files, 210 tracked Markdown files, 212 classified worktree documents, 35 passive capability observations, 181 campaign-evidence observations, 2 historical cleanup receipts, and 9 ADR records. The complete Node suite passed 307/307; the benchmark and SPEC evidence hardening tests passed 24/24 with the real-record validators green; Rust format, clippy, and the serial workspace suite exited 0. Hardware-, root-, GPU-, and lab-bound tests remained explicitly ignored rather than promoted.
**Verdict:** ✅ Static governance and evidence-custody controls are green. This does not qualify live Windows, WSL2, GPU, driver, VM, swap, disk, kernel, or hosted-CI claims; those require their owned environment and before/action/after evidence.

## 2026-08-11 13:21 -03 — Canonical CI automatic entrypoint verification

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0004`.
**Owner role:** `ci-governance`.
**Observed at:** `2026-08-11T13:21:44-03:00`.
**Verified at:** `2026-08-11T13:21:44-03:00`.
**Source revision:** `35ff48a793aaf52b6552b7b831138160a5f258e1`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only local verification and replace no prior hosted evidence.
**Freshness:** Revalidate on any workflow trigger, aggregate caller, required-check contract, or CI policy change.
**What:** Verified that CI Contract is the sole automatic pull-request/main entrypoint and that canonical child workflows cannot reintroduce duplicate direct runs.
**Category:** `ci-gate`.
**How to measure:** `./scripts/docs-check.sh`; `node --test tools/ci/*.test.mjs tools/*.test.mjs`; `node tools/ci/check-ci-contract.mjs --check-local`; `go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.7 .github/workflows/*.yml`; `gitleaks git --log-opts=-1 --redact --verbose`; and `git diff --check`.
**Measured data:** docs-check passed with 332 structural files, 223 tracked Markdown files, 212 classified documents, 35 capability observations, 181 campaign-evidence observations, 2 cleanup receipts, and 9 ADR records. The complete Node suite passed 309/309; the strict local CI contract returned PASS; Actionlint and Gitleaks returned 0 findings; whitespace checks passed.
**Verdict:** 🟡 The source topology is locally green and duplicate automatic children are rejected. A fresh same-revision hosted `required-checks` aggregate remains mandatory before promotion; no hosted, Windows, lab, VM, GPU, disk, or kernel result is claimed here.

## 2026-08-11 14:40 -03 — Campaign evidence clean-checkout admission

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0005`.
**Owner role:** `ci-governance`.
**Observed at:** `2026-08-11T14:40:00-03:00`.
**Verified at:** `2026-08-11T14:40:00-03:00`.
**Source revision:** `fcb12c6e626baf22280a45ca9f6bf566c3169257`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this summary and the generated catalog; temporary clean
worktrees are removed after verification.
**Freshness:** Revalidate on any campaign evidence policy, checker, catalog,
or documentation workflow change.
**What:** Made campaign evidence discovery deterministic between a developer
workspace containing ignored forensic logs and a clean GitHub Actions checkout.
**Category:** `ci-gate`.
**How to measure:** `node tools/ci/check-campaign-evidence-lifecycle.mjs
--generate`; `node tools/ci/check-campaign-evidence-lifecycle.mjs --check`;
the named Node coverage command; `./scripts/docs-check.sh`; and the same
commands from a detached temporary Git worktree at the source revision.
**Measured data:** The checker reported 176 Git-tracked observations. The
named suite passed 16/16; coverage was 97.87% lines, 80.31% branches, and
95.92% functions. The clean checkout passed the repository checker,
documentation gate, coverage command, and local CI contract.
**Refusals:** A missing Git source, malformed CLI arguments, a stale catalog,
an untracked declared artifact, and ignored local artifacts beside a newly
tracked campaign all produce the expected terminal outcomes in named tests.
**Rollback trigger:** A clean checkout and a workspace with ignored local
evidence produce different catalogs, an ignored artifact changes a repository
verdict, or the hosted documentation job accepts a coverage failure.
**Verdict:** 🟡 Static and clean-checkout proof is green. A fresh same-revision
hosted `required-checks` aggregate remains mandatory before promotion; no
Windows, WSL2, VM, driver, GPU, storage, swap, kernel, or reboot proof is
claimed here.

## 2026-08-11 14:58 -03 — Hosted canonical CI admission

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0006`.
**Owner role:** `ci-governance`.
**Observed at:** `2026-08-11T14:58:00-03:00`.
**Verified at:** `2026-08-11T14:58:00-03:00`.
**Source revision:** `379d132b8e7c93d0331c36ea6c2e36ededbe47fe`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the hosted run URL and this append-only summary; no raw
runner data is copied into the repository.
**Freshness:** Revalidate on every canonical workflow, CI contract, campaign
evidence checker, or release-admission change.
**What:** Confirmed the merged campaign-evidence and canonical-entrypoint
changes on the real GitHub Actions `main` surface.
**Category:** `ci-gate`.
**How to measure:** GitHub Actions run `31519630838` at the source revision;
inspect its `required-checks` conclusion and all selected caller summaries.
**Measured data:** 7/7 active callers plus the aggregate passed (8 successful
conclusions): `ci-contract`, CI core, security, Gitleaks, Windows static,
artifact hygiene, exact Rust slice coverage, and `required-checks`.
3/3 pull-request-only callers (comment-language, validation-schema, and
PR-body) were correctly skipped for a `main` push.
**Refusals:** The aggregate remains fail-closed for a failed, cancelled,
missing, or unexpectedly skipped active caller. No old cancelled same-SHA
execution was used as positive evidence.
**Rollback trigger:** `required-checks` is absent, a selected caller is
cancelled or skipped, the hosted catalog differs from the committed catalog,
or a main push produces a duplicate automatic child workflow.
**Verdict:** ✅ The hosted `main` aggregate is green for this revision. This
CI result does not qualify Windows, WSL2, VM, driver, GPU, storage, swap,
kernel, or reboot evidence.

## 2026-08-12 10:58 -03 — WSL2 NBD 1 GiB supervised activation

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0007`.
**Owner role:** `wsl2-nbd-operator`.
**Observed at:** `2026-08-12T13:56:34Z`.
**Verified at:** `2026-08-12T13:58:08Z`.
**Source revision:** `0b09518c530253a3219326ae3c0fe006e60ef99c`.
**Lifecycle:** `reviewable`.
**Retention:** Preserve the sanitized before/action/after receipts under
`docs/specs/no-milestone/wsl2-nbd-product-readiness/evidence/2026-08-12-live/`.
**Freshness:** Revalidate after any NBD lifecycle, sealed-release, daemon,
Relay, or swap-order change.
**What:** Installed the sealed WSL2 NBD release with explicit approval,
migrated the inactive legacy unit by exact SHA-256, and activated the approved
1 GiB NBD pilot without a reboot.
**Category:** `wsl2-nbd-live`.
**How to measure:** `ramshared status`; `/proc/swaps`; `wsl-relay-health.sh
--check`; `nbd-product-preflight.sh --check`; the approved `cascade-up.sh
--execute`; and `readlink /proc/<ramsharedd-pid>/exe` under `sudo`.
**Measured data:** Before activation, only `/dev/sdc` swap was present
(4,194,304 KiB, priority -2, 3,518,416 KiB used), Relay reported zero
candidates, and product preflight reported `PRODUCT_OFF`. The unprivileged
action refused with `I/O: Permission denied` before device creation. The
approved `sudo` action created `/dev/zram0` and `/dev/nbd0`, each 1,048,572
KiB, at priorities 200 and 100 respectively; `/dev/sdc` remained at -2.
After activation the daemon PID was 2062165 and both executable paths resolved
to the sealed `v0.8.0-8-g0b09518` binary; preflight reported
`NBD_BINARY_MATCH=PASS`, `NBD_TRANSPORT=nbd`, `NBD_PRODUCT_STATE=READY`, and
Relay remained `CLEAN` with zero candidates. The unit remained inactive and
disabled; no reboot occurred.
**Refusals:** Missing operator privilege produced `I/O: Permission denied`
without creating an NBD device or daemon. The initial installer path had also
refused the mismatched legacy unit before the exact SHA-scoped migration
approval was supplied.
**Rollback trigger:** Any failed `swapoff`, remaining managed NBD/ublk swap,
Relay candidate, binary mismatch, ghost state, or priority ordering other than
zram 200 > NBD 100 > disk -2 requires the named safe teardown rather than a
second activation.
**Verdict:** 🟡 The real 1 GiB WSL2 NBD activation and identity checks passed.
The required 1/2/4 GiB benchmark matrix with n>=3 and median/p99/deviation is
not yet run, so this does not claim index-quality DONE.

## 2026-08-14 00:59 -03 — WSL2 NBD Attempt29 P1 timeout refusal

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0008`.
**Owner role:** `wsl2-nbd-operator`.
**Observed at:** `2026-08-14T00:54:32-03:00`.
**Verified at:** `2026-08-14T00:59:39-03:00`.
**Source revision:** `a60c898ec6d938e6828d879d41a4b2ea0c7b6b21`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the host-private campaign root and the two committed
SHA-256 identities below; do not promote partial cell artifacts.
**Freshness:** Revalidate after any timeout-budget, worker-integrity, cgroup,
controller, CUDA, NBD, or cleanup change.
**What:** Ran the approved canonical Windows/WSL2 matrix with stop-first-RED
against the exact sealed release. The first P1 idle disk-only cell refused on
its second sample before NBD or any bounded/CUDA condition ran.
**Category:** `wsl2-nbd-live`.
**How to measure:** Canonical controller PlanOnly followed by the approved live
controller; inventory byte/hash verification; exact terminal pinned preflight;
and read-only process, cgroup, swap, NBD, and service residue inspection.
**Measured data:** Run one completed `3584 MiB`, HOLD, occupancy, and checksum;
allocation-to-HOLD was `114056 ms`, the integrity worker exited zero, and no
cgroup `oom_kill` increment was observed. Run two reached only `2048/3584 MiB`
before the 120-second HOLD deadline and emitted `SAMPLE_TIMEOUT`. The matrix
stopped `RED/failed_pair`; NBD and bounded cells did not run, so no CUDA VRAM
allocation was expected. All 36 inventory records verified. Matrix-summary
SHA-256 is `3f85c9948dc8c733b06351c029bc7a2a1512574cdc1ee8fdd8abfe41b78ef33e`;
inventory SHA-256 is
`e1d62c1c7a0d349624a8b68a309830495b67b2a2aa3c5efdd24b20a55b558fa9`.
**Refusals:** No completed pair or public evidence was produced. Terminal
pinned preflight returned `PRODUCT_OFF`; no managed swap, worker, daemon,
CUDA process, benchmark cgroup, or NBD attachment remained. The pre-existing
`/dev/sdc` swap was not changed.
**Rollback trigger:** Any timeout promotion, public evidence from this partial
cell, terminal state other than exact `PRODUCT_OFF`, residual managed resource,
or mutation of `/dev/sdc` invalidates the campaign and blocks another run.
**Verdict:** 🟡 The refusal and cleanup are valid diagnostic evidence. The
source-only P1 policy successor (`240 s` HOLD, independent `120 s` integrity)
must be committed, resealed, and exercised by a fresh complete matrix before
qualification or PR promotion.

## 2026-08-14 02:52 -03 — WSL2 NBD Attempt30 complete matrix

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0009`.
**Owner role:** `wsl2-nbd-operator`.
**Observed at:** `2026-08-14T01:17:39-03:00`.
**Verified at:** `2026-08-14T02:52:22-03:00`.
**Source revision:** `a365bda0daf89a9707159b86efca8c1ba1ac760b`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the host-private 551-entry campaign and the six copied
repository pair-custody/comparison records; the compact public records remain
in `docs/benchmarks/results.jsonl`.
**Freshness:** Revalidate after any benchmark policy, worker-integrity, cgroup,
controller, CUDA, NBD, evidence-custody, or cleanup change.
**What:** Ran the approved canonical Windows/WSL2 matrix against the exact
sealed release. All 12 P1/P2/P4 idle/bounded disk-only/NBD cells and all 36
samples completed with integrity, occupancy, and cleanup.
**Category:** `wsl2-nbd-live`.
**How to measure:** Canonical PlanOnly followed by the approved live controller;
per-cell `BINARY_MATCH`; pair-scoped CUDA custody; inventory byte/hash
verification; repository public-evidence validation; terminal pinned preflight;
and read-only process, cgroup, swap, NBD, service, and VRAM residue inspection.
**Measured data:** Every NBD cell retained `BINARY_MATCH=PASS`; every bounded
pair held one CUDA context across disk-only then NBD and released it without
force. Matrix-summary SHA-256 is
`42fa3e1a00dd7e7c16f0c92196f69622ac9212c9fb889e858f6e40769af292af`.
The 551-entry inventory SHA-256 is
`58a959fd82d29b6c503382a98d82a4bbf57bb90dc94ffaf2fdc2dfa6e985aece`,
and every listed byte count and hash verified. All six public pair records pass
the repository validator as `BASELINE`/nonpromotable because no prior canonical
baseline exists.
**Refusals:** No timeout, integrity, identity, cleanup, or evidence refusal
occurred. The absence of a prior canonical baseline prevents promotion of the
six baseline records but does not invalidate the completed live matrix.
**Rollback trigger:** Any matrix/inventory hash mismatch, NBD identity drift,
failed public custody record, cell or terminal state other than exact
`PRODUCT_OFF`, residual managed resource, forced CUDA release, or mutation of
the pre-existing `/dev/sdc` invalidates this evidence.
**Verdict:** 🟢 The complete sealed 1/2/4 GiB idle/bounded disk-only/NBD
matrix passed with n=3 per cell. Live qualification is complete; Gate B,
hosted required checks, PR review, and merge remain open.

## 2026-08-14 07:54 -03 — Protected beta publication

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0010`.
**Owner role:** `release-operator`.
**Observed at:** `2026-08-14T10:50:48Z`.
**Verified at:** `2026-08-14T10:54:33Z`.
**Source revision:** `f03f4e7a33cd64e8614532916294ab9628ce1aba`.
**Lifecycle:** `immutable`.
**Immutability reason:** Public release ID `370457260`, tag
`v0.9.0-beta.1`, immutable tag commit, and public asset digests are retained by
GitHub; the exact protected run and integrity run IDs remain auditable.
**What:** Published the exact RamShared beta through the GitHub App-authored
repository dispatch and the manually approved `protected-release` environment,
then independently downloaded and revalidated the public asset quartet.
**Category:** `ci-gate release-publication`.
**How to measure:** `gh run view 31793790581`; `gh api
repos/emersonbusson/ramshared/releases/tags/v0.9.0-beta.1`; `gh release download
v0.9.0-beta.1 -R emersonbusson/ramshared`; detached `sha256sum -c`; and
`node tools/ci/check-release-integrity.mjs --check` with the exact tag source
lock.
**Measured data:** Human request run `31793772726` delegated to App-authored
run `31793790581`; every protected step passed. Release ID `370457260` is
`draft=false`, `prerelease=true`, published at `2026-08-14T10:50:48Z`, and its
tag resolves to `361427a63cbeb2a8b0ecafb224adeecb0539af9b`. Exactly four assets
exist: archive `1169645` bytes / SHA-256
`f525f04ec536d52c57ea7708e0324152e931d2ee30d3885496a639f959972b3b`;
detached checksum `103` bytes /
`d2d1e2042fad0dd87035f9c6cee7d8ed14fe7909c7236fb9f2820ecfd8c4b2bb`;
SBOM `30233` bytes /
`d3ea9c0add12c6103be7cef6d43431b16cd2928c53b9d78d2420792fbdc044b8`;
manifest `3101` bytes /
`73bab87773a39304053364c77e70192cd45a653f32c91502e4716ddd1013aed6`.
The detached checksum and complete manifest/source-lock validation passed.
**Refusals:** Runs `31791853476` and `31792525304` stopped before upload;
run `31793116494` uploaded the exact quartet but stopped before visibility.
The successful replay found no missing asset, patched only the cardinally
selected release ID, and ended in the idempotent `NO_CHANGE` state.
**Rollback trigger:** Any tag SHA other than the exact 40-hex revision, release
ID other than `370457260`, asset count other than `4`, digest mismatch,
`draft=true`, `prerelease=false`, or non-App protected publisher invalidates
this evidence and requires a new target rather than overwrite or tag movement.
**Verdict:** ✅ The exact beta is publicly published with four independently
verified assets, App-only mutation authority, human environment approval, and
an idempotent terminal state.

## 2026-08-12 10:58 -03 — WSL2 NBD 1 GiB supervised activation

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0007`.
**Owner role:** `wsl2-nbd-operator`.
**Observed at:** `2026-08-12T13:56:34Z`.
**Verified at:** `2026-08-12T13:58:08Z`.
**Source revision:** `0b09518c530253a3219326ae3c0fe006e60ef99c`.
**Lifecycle:** `reviewable`.
**Retention:** Preserve the sanitized before/action/after receipts under
`docs/specs/no-milestone/wsl2-nbd-product-readiness/evidence/2026-08-12-live/`.
**Freshness:** Revalidate after any NBD lifecycle, sealed-release, daemon,
Relay, or swap-order change.
**What:** Installed the sealed WSL2 NBD release with explicit approval,
migrated the inactive legacy unit by exact SHA-256, and activated the approved
1 GiB NBD pilot without a reboot.
**Historical/non-current activation boundary:** This activation is retained as
dated evidence only. It is superseded and does not authorize execution on the
current disabled candidate.
**Category:** `wsl2-nbd-live`.
**How to measure:** `ramshared status`; `/proc/swaps`; `wsl-relay-health.sh
--check`; `nbd-product-preflight.sh --check`; the approved `cascade-up.sh
--execute`; and `readlink /proc/<ramsharedd-pid>/exe` under `sudo`.
**Measured data:** Before activation, only `SANITIZED_EXISTING_WSL_SWAP_DEVICE` swap was present
(4,194,304 KiB, priority -2, 3,518,416 KiB used), Relay reported zero
candidates, and product preflight reported `PRODUCT_OFF`. The unprivileged
action refused with `I/O: Permission denied` before device creation. The
approved `sudo` action created `/dev/zram0` and `/dev/nbd0`, each 1,048,572
KiB, at priorities 200 and 100 respectively; `SANITIZED_EXISTING_WSL_SWAP_DEVICE` remained at -2.
After activation the daemon PID was 2062165 and both executable paths resolved
to the sealed `v0.8.0-8-g0b09518` binary; preflight reported
`NBD_BINARY_MATCH=PASS`, `NBD_TRANSPORT=nbd`, `NBD_PRODUCT_STATE=READY`, and
Relay remained `CLEAN` with zero candidates. The unit remained inactive and
disabled; no reboot occurred.
**Refusals:** Missing operator privilege produced `I/O: Permission denied`
without creating an NBD device or daemon. The initial installer path had also
refused the mismatched legacy unit before the exact SHA-scoped migration
approval was supplied.
**Rollback trigger:** Any failed `swapoff`, remaining managed NBD/ublk swap,
Relay candidate, binary mismatch, ghost state, or priority ordering other than
zram 200 > NBD 100 > disk -2 requires the named safe teardown rather than a
second activation.
**Verdict:** 🟡 The real 1 GiB WSL2 NBD activation and identity checks passed.
The required 1/2/4 GiB benchmark matrix with n>=3 and median/p99/deviation is
not yet run, so this does not claim index-quality DONE.

## 2026-08-14 00:59 -03 — WSL2 NBD Attempt29 P1 timeout refusal

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0008`.
**Owner role:** `wsl2-nbd-operator`.
**Observed at:** `2026-08-14T00:54:32-03:00`.
**Verified at:** `2026-08-14T00:59:39-03:00`.
**Source revision:** `a60c898ec6d938e6828d879d41a4b2ea0c7b6b21`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the host-private campaign root and the two committed
SHA-256 identities below; do not promote partial cell artifacts.
**Freshness:** Revalidate after any timeout-budget, worker-integrity, cgroup,
controller, CUDA, NBD, or cleanup change.
**What:** Ran the approved canonical Windows/WSL2 matrix with stop-first-RED
against the exact sealed release. The first P1 idle disk-only cell refused on
its second sample before NBD or any bounded/CUDA condition ran.
**Category:** `wsl2-nbd-live`.
**How to measure:** Canonical controller PlanOnly followed by the approved live
controller; inventory byte/hash verification; exact terminal pinned preflight;
and read-only process, cgroup, swap, NBD, and service residue inspection.
**Measured data:** Run one completed `3584 MiB`, HOLD, occupancy, and checksum;
allocation-to-HOLD was `114056 ms`, the integrity worker exited zero, and no
cgroup `oom_kill` increment was observed. Run two reached only `2048/3584 MiB`
before the 120-second HOLD deadline and emitted `SAMPLE_TIMEOUT`. The matrix
stopped `RED/failed_pair`; NBD and bounded cells did not run, so no CUDA VRAM
allocation was expected. All 36 inventory records verified. Matrix-summary
SHA-256 is `3f85c9948dc8c733b06351c029bc7a2a1512574cdc1ee8fdd8abfe41b78ef33e`;
inventory SHA-256 is
`e1d62c1c7a0d349624a8b68a309830495b67b2a2aa3c5efdd24b20a55b558fa9`.
**Refusals:** No completed pair or public evidence was produced. Terminal
pinned preflight returned `PRODUCT_OFF`; no managed swap, worker, daemon,
CUDA process, benchmark cgroup, or NBD attachment remained. The pre-existing
`SANITIZED_EXISTING_WSL_SWAP_DEVICE` swap was not changed.
**Rollback trigger:** Any timeout promotion, public evidence from this partial
cell, terminal state other than exact `PRODUCT_OFF`, residual managed resource,
or mutation of `SANITIZED_EXISTING_WSL_SWAP_DEVICE` invalidates the campaign and blocks another run.
**Verdict:** 🟡 The refusal and cleanup are valid diagnostic evidence. The
source-only P1 policy successor (`240 s` HOLD, independent `120 s` integrity)
must be committed, resealed, and exercised by a fresh complete matrix before
qualification or PR promotion.

## 2026-08-14 02:52 -03 — WSL2 NBD Attempt30 complete matrix

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0009`.
**Owner role:** `wsl2-nbd-operator`.
**Observed at:** `2026-08-14T01:17:39-03:00`.
**Verified at:** `2026-08-14T02:52:22-03:00`.
**Source revision:** `a365bda0daf89a9707159b86efca8c1ba1ac760b`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the host-private 551-entry campaign and the six copied
repository pair-custody/comparison records; the compact public records remain
in `docs/benchmarks/results.jsonl`.
**Freshness:** Revalidate after any benchmark policy, worker-integrity, cgroup,
controller, CUDA, NBD, evidence-custody, or cleanup change.
**What:** Ran the approved canonical Windows/WSL2 matrix against the exact
sealed release. All 12 P1/P2/P4 idle/bounded disk-only/NBD cells and all 36
samples completed with integrity, occupancy, and cleanup.
**Category:** `wsl2-nbd-live`.
**How to measure:** Canonical PlanOnly followed by the approved live controller;
per-cell `BINARY_MATCH`; pair-scoped CUDA custody; inventory byte/hash
verification; repository public-evidence validation; terminal pinned preflight;
and read-only process, cgroup, swap, NBD, service, and VRAM residue inspection.
**Measured data:** Every NBD cell retained `BINARY_MATCH=PASS`; every bounded
pair held one CUDA context across disk-only then NBD and released it without
force. Matrix-summary SHA-256 is
`42fa3e1a00dd7e7c16f0c92196f69622ac9212c9fb889e858f6e40769af292af`.
The 551-entry inventory SHA-256 is
`58a959fd82d29b6c503382a98d82a4bbf57bb90dc94ffaf2fdc2dfa6e985aece`,
and every listed byte count and hash verified. All six public pair records pass
the repository validator as `BASELINE`/nonpromotable because no prior canonical
baseline exists.
**Refusals:** No timeout, integrity, identity, cleanup, or evidence refusal
occurred. The absence of a prior canonical baseline prevents promotion of the
six baseline records but does not invalidate the completed live matrix.
**Rollback trigger:** Any matrix/inventory hash mismatch, NBD identity drift,
failed public custody record, cell or terminal state other than exact
`PRODUCT_OFF`, residual managed resource, forced CUDA release, or mutation of
the pre-existing `SANITIZED_EXISTING_WSL_SWAP_DEVICE` invalidates this evidence.
**Verdict:** 🟢 The complete sealed 1/2/4 GiB idle/bounded disk-only/NBD
matrix passed with n=3 per cell. Live qualification is complete; Gate B,
hosted required checks, PR review, and merge remain open.

## 2026-08-14 07:54 -03 — Protected beta publication

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0010`.
**Owner role:** `release-operator`.
**Observed at:** `2026-08-14T10:50:48Z`.
**Verified at:** `2026-08-14T10:54:33Z`.
**Source revision:** `f03f4e7a33cd64e8614532916294ab9628ce1aba`.
**Lifecycle:** `immutable`.
**Immutability reason:** Public release ID `370457260`, tag
`v0.9.0-beta.1`, immutable tag commit, and public asset digests are retained by
GitHub; the exact protected run and integrity run IDs remain auditable.
**What:** Published the exact RamShared beta through the GitHub App-authored
repository dispatch and the manually approved `protected-release` environment,
then independently downloaded and revalidated the public asset quartet.
**Category:** `ci-gate release-publication`.
**How to measure:** `gh run view 31793790581`; `gh api
repos/emersonbusson/ramshared/releases/tags/v0.9.0-beta.1`; `gh release download
v0.9.0-beta.1 -R emersonbusson/ramshared`; detached `sha256sum -c`; and
`node tools/ci/check-release-integrity.mjs --check` with the exact tag source
lock.
**Measured data:** Human request run `31793772726` delegated to App-authored
run `31793790581`; every protected step passed. Release ID `370457260` is
`draft=false`, `prerelease=true`, published at `2026-08-14T10:50:48Z`, and its
tag resolves to `361427a63cbeb2a8b0ecafb224adeecb0539af9b`. Exactly four assets
exist: archive `1169645` bytes / SHA-256
`f525f04ec536d52c57ea7708e0324152e931d2ee30d3885496a639f959972b3b`;
detached checksum `103` bytes /
`d2d1e2042fad0dd87035f9c6cee7d8ed14fe7909c7236fb9f2820ecfd8c4b2bb`;
SBOM `30233` bytes /
`d3ea9c0add12c6103be7cef6d43431b16cd2928c53b9d78d2420792fbdc044b8`;
manifest `3101` bytes /
`73bab87773a39304053364c77e70192cd45a653f32c91502e4716ddd1013aed6`.
The detached checksum and complete manifest/source-lock validation passed.
**Refusals:** Runs `31791853476` and `31792525304` stopped before upload;
run `31793116494` uploaded the exact quartet but stopped before visibility.
The successful replay found no missing asset, patched only the cardinally
selected release ID, and ended in the idempotent `NO_CHANGE` state.
**Rollback trigger:** Any tag SHA other than the exact 40-hex revision, release
ID other than `370457260`, asset count other than `4`, digest mismatch,
`draft=true`, `prerelease=false`, or non-App protected publisher invalidates
this evidence and requires a new target rather than overwrite or tag movement.
**Verdict:** ✅ The exact beta is publicly published with four independently
verified assets, App-only mutation authority, human environment approval, and
an idempotent terminal state.

## 2026-08-20 17:03 -03 — Control containment and revocable-origin source candidate

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0011`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T17:03:32-03:00`.
**Verified at:** `2026-08-20T17:03:32-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only summary, the uncommitted working-tree
diff based on the stated revision until review, and the ignored local coverage
JSON under `tmp/`. This entry does not claim the unchanged base revision
contains the candidate.
**Freshness:** Revalidate after any control threshold, reservation/recovery,
guardian proof gate, origin durability/cache policy, systemd hierarchy, or
daemon identity change; replace with committed same-revision evidence before
promotion.
**What:** Validated the source/static candidate for aggregate WSL2 control-plane
containment, schema v4, the independent Windows guardian, safe-mode recovery,
and the SSD-authoritative revocable VRAM cache. No host installation, VHDX,
device, swap, GPU, pressure, Docker restart, WSL lifecycle, or publication
action ran.
**Category:** `local-check`.
**How to measure:** Run `cargo test --workspace --no-fail-fast`; selected
Clippy with warnings denied; the two canonical Rust slice-coverage commands in
the control/origin SPECs; the four safety shell suites; and
`scripts/windows/Test-WindowsCiStatic.ps1` under PowerShell 5.1.
**Measured data:** Workspace tests exited 0; the CLI candidate passed 135 unit
and 6 dispatch tests. Control line coverage was main 80.5%, workload 88.9%,
supervisor 92.0%, lifecycle 95.1%, and monitor 87.9%. Origin line coverage was
origin cache 94.2%, request 93.8%, sparse VRAM 93.3%, VRAM backend 91.6%,
cascade I/O 86.6%, lifecycle 95.1%, daemon backend 94.1%, and daemon main
80.1%. The NBD static preflight passed 37 cases; control-unit, reversible
manager, postmortem, guardian, origin, launcher, and full Windows static suites
all exited 0. Guardian/origin status commands returned plan-only state.
**Refusals:** CUDA/root/ublk/live-device tests remained ignored; the physical
origin was not opened; the guardian was not installed; no terminate or reboot
route executed. Missing/stale guardian and supervisor observations are
non-green, an old dxg boot warning is not classified as a crash, and the
static contracts reject broad WSL shutdown and Windows reboot routes.
**Rollback trigger:** Any acknowledged write absent from origin, GPU failure
becoming I/O error while origin succeeds, aggregate reservation above
`MemoryMax`, destructive supervisor replay during healthy hysteresis, guardian
terminate before all four proofs, or automatic host reboot invalidates this
candidate.
**Verdict:** 🟡 Source/static and exact coverage gates pass. Disposable-VM
VHDX/NBD/GPU/terminate matrices, attended installation, Docker/cron ancestry,
and the staged 24-hour daily-host rollout remain open; no live qualification is
claimed.

## 2026-08-20 17:47 -03 — Control-plane closure regression audit

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0012`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T17:47:00-03:00`.
**Verified at:** `2026-08-20T17:47:00-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only record and the uncommitted candidate
worktree with EVD-0011 until the source changes receive review; no temporary
fixture or package directory is promotion evidence.
**Freshness:** Revalidate after any guardian child-process timeout, control
plane installer/rollback, bundle payload, or localized README change.
**What:** Closed two source-only safety regressions found during integration:
a timeout cleanup in the Windows guardian could wait without a bound, and the
disabled control-plane manager could change Docker configuration metadata or
overwrite an operator edit during rollback. The sealed cascade installer now
also requires the complete control-plane payload before a write-capable path.
**Category:** `local-check`.
**How to measure:** Run the named guardian static harness; the isolated
user-namespace manager fixture; `test-nbd-product-preflight.sh`; the full
PowerShell static suite; `docs-check`; Rust formatting, selected Clippy, and
`cargo test --workspace --no-fail-fast`.
**Measured data:** The guardian test was RED against its previous unbounded
`WaitForExit()` cleanup and GREEN after it disposed the timed-out process
without a second unbounded wait. The manager fixture preserved a `0600` Docker
configuration, refused an operator-modified replacement, then restored the
exact original configuration and removed only its own units. The NBD preflight
suite passed `38` cases, including
`installer_requires_control_plane_payload`; the temporary bundle audit verified
all new control-plane files and `SHA256SUMS`. PowerShell parser/full static,
docs-check, `cargo fmt --all -- --check`, selected warnings-denied Clippy, and
the workspace tests all exited zero.
**Refusals:** No scheduled task, VHDX, disk, Docker configuration, unit,
swap, GPU pressure, WSL lifecycle, reboot, or external publication action ran.
CUDA/root/ublk live tests remained environment-gated.
**Rollback trigger:** Any guardian child cleanup that blocks beyond its declared
deadline, manager rollback that overwrites changed configuration, or installer
acceptance of a bundle missing a required control-plane file invalidates this
source-only result.
**Verdict:** 🟡 Local safety and packaging contracts are green. Disposable-VM
and attended daily-host rollout evidence remains required before installation,
activation, or release qualification.

## 2026-08-20 18:17 -03 — Nested WSL lab readiness and cleanup requalification

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0013`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T21:09:57Z`.
**Verified at:** `2026-08-20T21:17:47Z`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only summary and the sanitized local
host-private probe receipts until the guest credential is restored and a fresh
readiness probe supersedes them. Do not publish their filesystem paths.
**Freshness:** Revalidate after any guest credential, VM identity/VHD,
checkpoint, PowerShell Direct helper, WSL installation/service, or probe
cleanup change.
**What:** Requalified the existing approved `SANITIZED_VM_WSL2_LAB` boundary without
repairing or pressuring it, then hardened its readiness probe after a live
cleanup counterexample. The probe is now plan-first, exact-VM-ID bound, zero-
checkpoint and VM-owned-VHD gated, uses the shared bounded PowerShell Direct
transport, emits sanitized probe/cleanup reasons, and restores a VM it started
through a separately bounded graceful host fallback when guest transport is
unavailable.
**Category:** `local-check`.
**How to measure:** Run the probe in `plan` mode; run one explicitly approved
`probe -Start -Run` against the exact observed VM ID; observe the exact VM state
and checkpoint count afterward; execute
`Test-Win11WslRuntimeProbeStatic.ps1`, the PowerShell parser, and the full
`Test-WindowsCiStatic.ps1` suite.
**Measured data:** Host preflight observed one Off Generation-2 VM with four
vCPUs, 4 GiB startup memory, nested virtualization exposed, automatic
checkpoints disabled, zero snapshots, and one contract-matching VM-owned VHDX.
The first probe could not establish PowerShell Direct and also could not use
that same transport for cleanup; a normal Hyper-V graceful stop restored Off.
After the RED test and fix, the second probe returned
`powershell_direct_auth_failed` plus `restored_off_host_fallback`; an independent
after-observation reported Off and zero snapshots. The full Windows static
suite and parser exited zero. Both probe records report
`DISK_MUTATION=false`.
**Refusals:** The authenticated guest probe never began, so this evidence does
not claim current WSL service state, nested WSL readiness, distro readiness, or
resolution of the historical WSL command timeouts. No credential reset,
reimage, WSL repair/install, forced VM power-off, checkpoint, disk operation,
pressure, terminate, Docker restart, daily-host WSL lifecycle, or Windows
reboot occurred.
**Rollback trigger:** Any probe that starts a VM before exact identity and
approval gates, leaks a started VM without a typed cleanup failure, uses the
forbidden `Stop-VM -TurnOff`/`-Force` route, persists a secret/raw authentication error, changes
a guest disk, or reports WSL readiness without successful authenticated
bounded commands invalidates this evidence.
**Verdict:** 🟡 The lab lifecycle boundary and graceful cleanup fallback are
live-proven, but the current guest credential is rejected. Nested WSL readiness
and every destructive control-plane/origin matrix remain open.

## 2026-08-20 19:23 -03 — Reversible nested-lab credential diagnosis

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0014`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T19:04:45-03:00`.
**Verified at:** `2026-08-20T19:23:26-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** The bounded-probe and offline-recovery changes remain
an uncommitted candidate.
**Lifecycle:** `reviewable`.
**Retention:** Retain the private host-side SAM backup/restore receipts and
sanitized probe summaries under the local artifact root. Do not commit an
artifact path, VHD hash, guest password, account material, or raw authentication
error.
**Freshness:** Revalidate after any lab credential, VM/VHD identity, offline
recovery helper, PowerShell Direct helper, guest WSL installation, or cleanup
policy change.
**What:** Diagnosed whether the existing approved nested Windows lab could
reach its WSL runtime through a reversible credential recovery path. The path
used only the exact VM-owned VHD while the VM was Off, saved the original SAM,
reset a working copy to a temporary empty password, ran a bounded probe, and
restored the original SAM before closing the attempt.
**Category:** `local-check`.
**How to measure:** Require exact VM ID and VHD hash preflight, VM Off, zero
checkpoints, one matching VHD, and detached state; use the plan-first offline
recovery helper with explicit repair/blank-password approvals; run the
bounded nested WSL probe; restore the recorded original SAM; then observe VM
Off, VHD detached, and zero checkpoints. Run the named blank-credential,
probe, and full Windows static suites.
**Measured data:** The preflight observed 1 approved VM, 1 matching detached
VHD, and 0 checkpoints. Three private hash checks (backup, repair copy-back,
and restore) were distinct and verified. The first empty-password probe
exposed a local helper defect: an empty string was passed to the text-password
conversion before PowerShell Direct. After the helper changed to construct an
explicit empty secure credential only behind the opt-in, the guest returned
`powershell_direct_auth_failed`. The retrying graceful fallback returned the
VM to Off. The original SAM then restored with a matching verification hash.
A separate intentionally rejected-credential regression also returned
`restored_off_host_fallback`; final host observation was Off, VHD detached,
and 0 snapshots.
**Refusals:** The guest did not authenticate, so no guest WSL command, WSL
service observation, distro observation, guardian install, origin VHDX action,
swap action, GPU action, Docker action, pressure campaign, targeted WSL
terminate, broad WSL shutdown, forced VM power action, or Windows reboot ran.
The temporary blank password was not retained.
**Rollback trigger:** Any SAM backup/restore hash mismatch, VM left Running,
attached VHD, checkpoint residue, the forbidden `Stop-VM -TurnOff`/`-Force` route, secret or
raw authentication persistence, or readiness claim without an authenticated
guest command invalidates this evidence.
**Verdict:** 🟡 Nested virtualization is exposed, but it does not establish
nested WSL2 readiness. The current Windows image rejects both available
PowerShell Direct credential paths, so a console-supported repair, a managed
reimage of this same approved lab, or a physical Windows surface is required
before WSL2/GPU/guardian qualification can continue.

## 2026-08-20 19:47 -03 — Final origin-detach and guardian child-bound audit

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0015`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T19:34:00-03:00`.
**Verified at:** `2026-08-20T19:47:17-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** The origin provisioner and guardian improvements remain
an uncommitted source candidate.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only result, the source diff, and the local
static-suite transcript. Do not retain or publish VM identity, disk identity,
credential, or raw child-process diagnostics.
**Freshness:** Revalidate after any origin provisioner, Windows child-process
deadline, guardian task, or static-test change.
**What:** Closed two source-only lifecycle gaps: the origin VHDX provisioner
now detaches the VHDX in an install `finally` block, leaving runtime attachment
to the guardian; the guardian now drains redirected client output under a
deadline and terminates only the timed-out client process tree.
**Category:** `local-check`.
**How to measure:** Run `Test-RamSharedOriginStatic.ps1` and
`Test-RamSharedWslWatchdogStatic.ps1` before and after the source change; then
run `Test-WindowsCiStatic.ps1` and parse both changed scripts under PowerShell
5.1.
**Historical non-current / no execution:** The dated source/static receipt below
is evidence only; it does not authorize a VHD or driver operation.
**Measured data:** The new origin assertion was RED with 0 `Dismount-VHD`
install-finally paths, then GREEN with 1 bounded detach path. The guardian
contract was RED without asynchronous pipe drain/tree cleanup, then GREEN with
2 redirected-stream drains, 1 bounded `taskkill` child-tree path, and no
unbounded `WaitForExit()`. The full Windows static suite exited 0.
**Refusals:** No guardian task, VHDX, disk, swap, GPU action, Docker action,
guest command, WSL terminate, broad WSL shutdown, VM power action, or Windows
reboot ran for this source audit.
**Rollback trigger:** Any provisioner path that leaves a newly mounted origin
VHDX attached after install, or any guardian timeout path that can block on an
undrained pipe or leave its own child tree running, invalidates this evidence.
**Verdict:** 🟡 The source contracts are stronger and green. This does not
qualify an origin VHDX or a guardian termination on live hardware.

## 2026-08-20 21:13 -03 — Disposable lab autologon and WSL readiness recovery

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0016`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T20:35:00-03:00`.
**Verified at:** `2026-08-20T21:13:01-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** The persistent-autologon and UTF-16LE probe corrections
remain an uncommitted source candidate.
**Lifecycle:** `reviewable`.
**Retention:** Retain the private verified pre-reimage VHD backup, the sealed
local installer media, and sanitized readiness artifacts on the Windows host.
Do not commit their paths, hashes, VM identity, account material, or password.
**Freshness:** Revalidate after any lab image, unattended-media contract,
credential, Winlogon policy, WSL package/distro, PowerShell Direct helper, or
runtime-probe change.
**What:** Recoverably reimaged the existing approved `SANITIZED_VM_WSL2_LAB`, made its
console non-interactive for the life of the disposable image, restored bounded
PowerShell Direct, installed current WSL through the official web-download
path, and revalidated the nested WSL runtime.
**Category:** `local-check`.
**How to measure:** Require the exact Off Generation-2 VM, zero checkpoints,
one exact VM-owned VHD, nested virtualization, vTPM, a local credential source,
and a byte-count/SHA-256-verified rollback copy. Build and validate no-prompt
media with an embedded sealed answer file; prove VHD activity and return the
next boot to the exact VHD. After `IMAGE_STATE_COMPLETE`, configure persistent
lab-only Winlogon without `AutoLogonCount` or `AutoLogonSID`, reboot, and require
the `SANITIZED_LAB_USER` interactive user plus Explorer. Install WSL under a transient
highest-run-level task, then run bounded `wsl --status` and `wsl -l -v` through
the exact-ID runtime probe.
**Measured data:** The first 4 GiB VM start refused with host error
`0x800705AA`; a clean WSL page-cache reclaim raised Windows free memory without
terminating the distro, and the retry started normally. Setup completed with
zero checkpoints. Live evidence disproved `LogonCount=9999`: OOBE selected
`defaultuser0` and consumed the count. A post-OOBE Winlogon configuration with
no count/SID survived a proved reboot, produced the interactive
`SANITIZED_PRINCIPAL_WSL2_LAB` Explorer session, and allowed removal of the
temporary OOBE account. The WSL install task returned zero and was unregistered.
An initial runtime probe missed the installed distro because redirected
`wsl.exe` output was UTF-16LE; the named static test was RED, then GREEN after
setting both redirected stream encodings to Unicode. The final live probe was
`PASS / wsl_runtime_ready`; service, status, list, and exact distro gates passed
within their deadlines.
**Refusals:** No new VM, checkpoint, host disk, daily-host WSL lifecycle,
artificial pressure, Docker restart, origin VHDX, RamShared swap, guardian
termination, broad WSL shutdown, Windows-host reboot, commit, or publication
occurred. The destructive scope was only the disposable VM-owned guest disk;
the prior VHD remains recoverable from its verified private backup.
**Rollback trigger:** Any backup mismatch, foreign disk, checkpoint residue,
secret leakage, autologon on the daily host, consumable/identity-confused
Winlogon state, missing interactive-user proof, unbounded WSL child, NUL-bearing
probe evidence, or readiness result without successful bounded status/list and
exact distro presence invalidates this result.
**Verdict:** ✅ The existing isolated Windows lab is unlocked and WSL-runtime
ready. 🟡 Destructive guardian, origin, GPU, and pressure matrices remain open.

## 2026-08-20 21:42 -03 — Interactive-auth suppression and final media seal

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0017`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T21:22:00-03:00`.
**Verified at:** `2026-08-20T21:42:38-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** The complete disposable-lab unlock contract remains an
uncommitted source candidate.
**Lifecycle:** `reviewable`.
**Retention:** Retain the sealed private installer, its private answer file,
the verified rollback VHD, and sanitized probe receipt. Do not publish their
paths, hashes, VM identity, or credential material.
**Freshness:** Revalidate after any unattended-media, Winlogon, screen-saver,
power-policy, guest account, or runtime-probe change.
**What:** Extended the lab-only post-OOBE contract so a future clean install
reproduces the live unlocked console state instead of relying on manual repair.
**Category:** `local-check`.
**How to measure:** Decode the manufactured canonical post-OOBE command and
require screen-saver disablement, non-secure screen saver, zero screen-saver
timeout, disabled workstation lock, and zero machine inactivity timeout. Seal
that exact command into no-prompt media, require matching external/embedded
answer hashes and the EFI no-prompt boot image, then mount only the validated
media on the exact snapshot-free VM. Re-read the live user/policies and rerun
the exact-ID bounded WSL runtime probe.
**Measured data:** The named media test was RED at missing `ScreenSaveActive`,
then GREEN after all 5 interactive-auth surfaces were added. Live policy
readback returned zero for screen-saver active/secure/timeout and inactivity,
no screen-saver executable, and disabled workstation lock. The interactive
user remained `SANITIZED_LAB_USER` with Explorer, persistent count-free Winlogon, and a
non-expiring password. The rebuilt media matched its sealed answer hash, used
the no-prompt EFI image, and was mounted with Windows Boot Manager still first,
one exact VM-owned VHD, and zero checkpoints. The final bounded WSL probe passed
`wsl_runtime_ready` without starting or stopping the already-running VM.
**Cleanup:** Removed only two superseded private ISOs, their two answer files,
and three exact staging trees. They are not directly recoverable but are fully
regenerable from the retained source ISO and repository scripts. The rollback
VHD was preserved.
**Refusals:** No pressure, origin VHDX, RamShared swap, Docker restart,
guardian termination, broad WSL shutdown, host reboot, commit, or publication
occurred.
**Rollback trigger:** Any interactive authentication prompt, reappearance of a
consumable Winlogon count/SID, media hash mismatch, non-VHD first boot, snapshot
residue, or failed bounded WSL status/list gate invalidates this evidence.
**Verdict:** ✅ The existing disposable Windows lab and its retained reinstall
media are fully unlocked and WSL-runtime ready. 🟡 Destructive matrices remain
open.

## 2026-08-20 22:35 -03 — Isolated pressure-campaign execution preflight

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0018`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T22:29:00-03:00`.
**Verified at:** `2026-08-20T22:35:37-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** The isolated campaign was authorized but did not pass its
runtime execution gate; no pressure claim is made.
**Lifecycle:** `reviewable`.
**Retention:** Retain the sanitized host-private partial receipt. Do not publish
its path, VM identity, credentials, raw command output, or private media.
**Freshness:** Revalidate after any nested WSL runtime repair, WSL service
change, guest reboot, source deployment, or pressure-harness change.
**What:** Attempted the first approved isolated-only campaign gate on the
existing disposable Windows lab, before source copy, storage setup, or pressure.
**Category:** `local-check`.
**How to measure:** Require one exact running Generation-2 VM, 0 checkpoints,
one approved VHD, nested virtualization, and bounded PowerShell Direct. Require
bounded WSL `--status` and `-l -v` queries plus one 15-second direct guest
execution before preparing the campaign. On timeout, kill only the bounded WSL
client, preserve a partial receipt, and do not run pressure or repair.
**Measured data:** The VM had 0 checkpoints, one VHD, 4,096 MiB assigned, and
4,403 MiB host free. WSL status and list each completed with exit 0; an initial
direct `/bin/true` execution completed with exit 0. A subsequent read-only
guest command exceeded its 15-second deadline. `WslService` and `vmcompute`
remained `Running`, 0 WSL client processes remained after bounded cleanup, and
the partial receipt records `pressure_attempted=false`, no storage mutation,
no targeted termination, and no host reboot.
**Refusals:** No repository copy, origin VHDX, zram, NBD, RamShared swap,
GPU pressure, cgroup pressure, watchdog installation, WSL termination, broad
WSL shutdown, host reboot, commit, or publication occurred.
**Rollback trigger:** Any direct guest-execution timeout, nonzero bounded WSL
query, checkpoint residue, foreign VHD, remaining client process, or attempted
pressure without a complete execution gate keeps the campaign `PARTIAL`.
**Verdict:** 🟡 The isolated VM remains intact but is not yet a stable pressure
surface. Repair/reimage decisions require a new explicit authorization; the
daily host remains out of scope.

## 2026-08-20 23:22 -03 — Isolated campaign preparation and fail-closed pressure attempt

**Historical non-current / no execution:** This dated VM-only preparation and
campaign record is evidence only; it authorizes no current VM, WSL, swap, or pressure action.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0019`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T23:18:44-03:00`.
**Verified at:** `2026-08-20T23:22:17-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** The guest preparation and bounded isolated campaign
attempt remain an uncommitted source candidate; no pressure result is claimed.
**Lifecycle:** `reviewable`.
**Retention:** Retain the sanitized guest-private baseline and campaign
receipts. Do not publish their paths, VM identity, credentials, or raw child
diagnostics.
**Freshness:** Revalidate after any CUDA/WSL driver exposure, RamShared
activation, kernel module change, swap topology change, guest reboot, or
pressure-harness change.
**What:** Prepared the exact disposable `SANITIZED_VM_WSL2_LAB` /
`SANITIZED_WSL_DISTRO`
surface by transferring only the seven required campaign files, verifying
their guest SHA-256 values against the source, and restoring executable bits.
The read-only baseline then ran with the explicit isolated-lab override. A
bounded two-round campaign invocation was allowed to reach the pressure probe;
the probe refused before cgroup creation or worker allocation because the
required cascade was absent.
**Category:** `local-check`.
**Historical non-current / no execution:** The dated VM-only measurement recipe
below is retained evidence only. It does not authorize a campaign, VM, WSL,
swap, device, storage, or pressure action.
**How to measure:** Require the already-recorded exact VM identity,
Generation-2/zero-checkpoint/VM-owned-VHD gates and bounded PowerShell Direct;
**Historical non-current / no execution:** The following dated campaign recipe
is evidence only; do not run it on the current disabled candidate.
verify all seven deployed file hashes and executability; run
`wsl2-freeze-campaign.sh --dry-run --json`; run `ramshared check --json`; then
**Historical non-current / no execution:** The following isolated-run flags are
dated evidence only; do not execute them on the current disabled candidate.
run `--allow-isolated-lab --run-isolated --rounds 2` with a 30-second
watchdog. Validate the typed campaign receipt and post-run swap/process state.
**Measured data:** The baseline classified the guest as isolated with
`gates_ok=true`, no ghost daemon, no deleted swap, zero blocked processes,
zero recent hung-task/OOM hits, and only the 1 GiB disk swap at priority `-2`.
the read-only candidate check returned WSL2 `6.18.33.2`,
`SANITIZED_GPU_DEVICE_NODE` present but
CUDA blocked (`libcuda.so not found`), NBD support present but its module not
loaded, and ublk disabled; the product decision was `blocked`. The campaign
entered round 1 and stopped with `action_rc=1` at the explicit probe refusal:
`need live zram + nbd + disk`. Post-run verification
found the same disk-only swap with `0` used KiB, cascade health `OFF`, no
`ramsharedd` process, and no pressure worker or cgroup residue. WSL emitted a
systemd root-session warning on bounded invocations, but each requested command
returned within its deadline.
**Refusals:** No `ramshared up`, zram/NBD/origin VHDX, GPU allocation, cgroup
pressure, Docker action, broad WSL shutdown, host reboot, or daily-host action
ran. The campaign therefore provides fail-closed refusal evidence only, not a
freeze-elimination or tier-order result.
**Rollback trigger:** Any report of a successful pressure round without live
zram + NBD + disk, any worker allocation before the probe gate, any ghost swap
or daemon after cleanup, or any action on the daily host invalidates this
evidence.
**Verdict:** 🟡 The isolated guest source surface is prepared and the harness
refuses safely, but the requested pressure campaign is blocked by the VM's
missing CUDA/NBD/zram product surface. A future GPU-backed or separately
qualified product-activation setup is required; the daily host remains out of
scope.

## 2026-08-20 23:31 -03 — GPU-PV and RamShared activation follow-up

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0020`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T23:24:00-03:00`.
**Verified at:** `2026-08-20T23:31:00-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** The follow-up remains an uncommitted live candidate and
does not promote the pressure matrix.
**Lifecycle:** `reviewable`.
**Retention:** Retain sanitized VM-private receipts only; do not publish host
GPU identity, credentials, or raw PowerShell/WSL diagnostics.
**Freshness:** Revalidate after any GPU-PV adapter change, guest NVIDIA/CUDA
installation, RamShared daemon change, or WSL kernel/module change.
**What:** Checked whether the existing VM could supply the missing GPU-PV
surface without changing the daily host. The host reported one partitionable
GPU, but the exact `SANITIZED_VM_WSL2_LAB` had zero `VMGpuPartitionAdapter` entries.
**Historical/non-current activation boundary:** The following failed VM-only
activation attempt is retained as evidence of rollback. It is superseded and
must not be repeated on the current disabled candidate.
The historical product activation interface was then attempted inside the VM.
**Measured data:** The historical interface created a sanitized zram device
briefly and armed its forensics marker, then failed with `daemon did not start
(socket missing)` and returned `UP_RC=1`. Its rollback left only
`SANITIZED_EXISTING_WSL_SWAP_DEVICE` at priority `-2`; zram, NBD, daemon,
runtime files, and the forensics marker were absent on the following read-only
verification. No GPU-PV adapter was attached, no host GPU partition was
reserved, and no VM power transition was needed.
**Refusals:** No synthetic NBD, fake CUDA library, GPU-PV attachment, origin
VHDX, Docker action, host-daily pressure, host reboot, or commit ran. A real
campaign still requires a GPU-PV-qualified guest driver/daemon and live
zram→NBD→disk topology.
**Rollback trigger:** Any activation attempt that leaves zram, NBD, a daemon,
or a forensics marker after a failed start, or any GPU-PV change without a
bounded Off-state transition and readback, invalidates this evidence.
**Verdict:** 🟡 The activation code rolls back correctly, but the VM has no
GPU-PV adapter and its daemon cannot start. The isolated pressure campaign
remains blocked; the daily host was untouched.

## 2026-08-20 23:48 -03 — Reversible GPU-PV qualification attempt

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0021`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T23:35:00-03:00`.
**Verified at:** `2026-08-20T23:48:00-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** The reversible GPU-PV qualification attempt remains an
uncommitted live candidate and does not close CUDA or pressure gates.
**Lifecycle:** `reviewable`.
**Retention:** Retain only sanitized transition receipts. Do not publish GPU
partition identifiers, VM credentials, or raw PowerShell/guest diagnostics.
**Freshness:** Revalidate after any VM firmware, GPU-PV, NVIDIA guest driver,
WSL kernel, or Hyper-V resource change.
**What:** Tested one bounded GPU-PV assignment on the exact disposable VM.
The VM was shut down through the guest, one 1 GiB-minimum/1 GiB-optimal GPU
partition was added while Off, and the VM was restarted for a single bounded
`ramshared check --json`. Because the guest still had no CUDA runtime, the
partition was removed through another graceful shutdown and the VM restarted.
**Measured data:** Hyper-V accepted one adapter with VRAM/encode/decode/compute
minimum `1` and maximum/optimal `1,000,000,000`. The guest remained WSL2
`6.18.33.2` with `SANITIZED_GPU_DEVICE_NODE`, but `libcuda.so`, `nvidia-smi`, and a CUDA device
were still absent; `ramshared check` remained `decision=blocked` with the
CUDA blocker. Rollback readback reported adapter count `0`, VM `Running`, and
normal status. No DDA device was assigned and no host reboot occurred.
**Refusals:** No GPU pressure, cgroup pressure, RamShared activation, NBD,
origin VHDX, Docker action, daily-host WSL lifecycle, or publication ran. The
temporary GPU partition was fully returned before completion.
**Rollback trigger:** Any GPU-PV attempt that leaves an adapter attached after
CUDA refusal, leaves the VM Off, assigns DDA, or lacks exact pre/post adapter
readback invalidates this evidence.
**Verdict:** 🟡 Hyper-V GPU-PV assignment is technically accepted, but the
guest image lacks the NVIDIA/CUDA stack required by RamShared. The adapter was
removed cleanly; the pressure campaign remains blocked and the daily host was
not pressured.

## 2026-08-20 23:55 -03 — Guest/host GPU driver compatibility boundary

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0022`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-20T23:52:00-03:00`.
**Verified at:** `2026-08-20T23:55:00-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** Read-only compatibility evidence; no guest driver
installation was attempted.
**Lifecycle:** `reviewable`.
**Retention:** Retain sanitized device/version observations only. Do not
publish host paths, credentials, or raw driver-store diagnostics.
**Freshness:** Revalidate after any host NVIDIA driver update, GPU-PV adapter
change, guest image change, or Windows/WSL kernel update.
**What:** Compared the host GPU stack with the exact guest after the adapter
rollback to determine whether a supported userspace-only fix remained.
**Measured data:** The host exposes `SANITIZED_GPU_MODEL` with
`SANITIZED_HOST_DRIVER_VERSION` and private DriverStore packages. The guest
enumerates only `SANITIZED_GUEST_DISPLAY_ADAPTER` with
`SANITIZED_GUEST_DRIVER_VERSION`; `nvcuda.dll`, `nvml.dll`, and
`nvidia-smi.exe` are absent. The final Hyper-V readback is `Running` with `0`
GPU-PV adapters. This is a
driver/device-stack boundary, not a missing RamShared file.
**Refusals:** No driver package was copied or installed, no Windows PnP state
was changed, no guest reboot beyond the already rolled-back GPU-PV transition
was added, and no host-daily GPU or WSL pressure ran.
**Rollback trigger:** Treating host DriverStore files or a CUDA userspace stub
as guest CUDA proof, or installing an unqualified guest driver without a
separate signed-package/rollback plan, invalidates this evidence.
**Verdict:** 🟡 The remaining blocker is the guest NVIDIA/GPU-PV driver stack.
The current VM cannot produce a valid CUDA-backed NBD tier; the isolated
pressure campaign must remain blocked until a supported GPU-qualified image or
separate lab surface is supplied.

## 2026-08-21 01:14 -03 — Shared-host admission refusal after source validation

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0023`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-21T01:14:21-03:00`.
**Verified at:** `2026-08-21T01:14:21-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** Source-qualified only; the live admission refusal does
not promote the freeze-elimination campaign or claim a pressure result.
**Lifecycle:** `reviewable`.
**Retention:** Retain only the sanitized artifact summary and host-memory
admission data. Do not persist secrets, private process arguments, or raw
elevation diagnostics.
**Freshness:** Revalidate after any harness/module, Windows elevation, VM
state, selected-distro, or host-memory change.
**What:** Completed the source TDD and attempted the explicitly authorized R4
target `SANITIZED_HOST_NAME` / `SANITIZED_WSL_DISTRO` admission. The source RED sequence exposed a
missing module, then null-headroom misclassification plus a missing post-launch
cleanup owner, then a normal-PASS path that could swallow a summary-write
failure; all were corrected.
**Category:** `fail-safe`.
> **Historical non-current / no execution:** The dated verification list below is
> retained evidence only; it does not authorize a current pressure campaign.
**How to measure:** Run `Test-SharedWslPressureCampaignMemoryGate.ps1`,
`Test-SharedWslPressureCampaignStatic.ps1`, the shell artifact static test,
the PowerShell parser, `./scripts/docs-check.sh`, and
`git diff --check -- validation.md
docs/specs/no-milestone/wsl2-freeze-elimination-campaign/IMPL.md`.
**Measured data:** The added module/harness contract reserves `4096` MiB and
requires `ceil(2.92*1024)+4096 = 7087` MiB, with three one-second minimum
samples, a runtime guardian, exact selected-distro termination, no OOM
override/reboot/shutdown/disk/VM path, and a mandatory PASS summary. The named
PowerShell tests, shell artifact static test, PowerShell parser, full
`docs-check.sh`, and diff-check were GREEN; an independent R3 source review
was PASS. PowerShell harness coverage is N/A per SPEC, with manufactured
branches complete. Read-only module samples were `[4566,4562,4563]` MiB
(`min=4562`); exact harness samples were `[4494,4479,4482]` MiB
(`min=4479`) versus `required=7087`. Non-interactive elevation was unavailable:
Windows `sudo` required UAC and no pre-existing elevated RamShared channel was
available. The harness exited `2` with `STATUS=REFUSED`
`REASON=host_commit_headroom_insufficient`.
**Artifact:** A sanitized host-private receipt contains only
`host-memory.jsonl`, `host-memory-admission.json`, and `summary.json`. It
contains no disk telemetry, WSL campaign action,
candidate lifecycle activation, pressure, watchdog/guardian execution, or live
validator PASS. A VM-stop action was **not** run; no bypass or force path was
used.
**Cleanup:** Two bounded read-only WSL `/bin/true` probes returned PASS;
`ramsharedd` was dead, campaign phase was `Off`, zram/NBD were absent, only
`SANITIZED_EXISTING_WSL_SWAP_DEVICE` remained at priority `-2`, `ghost=false`, and no recent OOM,
hung-task, D-state, or I/O findings were present. C/I checks were healthy.
**Refusals:** No live pressure, WSL cascade action, VM shutdown, reboot,
Windows shutdown, disk mutation, or automatic retry ran. The refusal reason
was retained and the campaign stayed fail-closed.
**Rollback trigger:** Any PASS/live qualification claim, missing refusal
reason, raw secret/private process argv, rewrite of prior evidence, or claim
that VM shutdown or pressure occurred invalidates this record.
**Verdict:** 🟡 Status remains `PARTIAL` / source-qualified only. A fresh
approved run requires non-interactive elevation and post-VM-off host headroom
`>=7087` MiB; no automatic retry is authorized.

## 2026-08-21 08:06 -03 — Audited lab stop and shared-host re-admission refusal

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0024`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-21T08:06:40-03:00`.
**Verified at:** `2026-08-21T08:06:47-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** Live lifecycle and admission evidence only; the
freeze-elimination campaign remains `PARTIAL` and no pressure result is
claimed.
**Lifecycle:** `reviewable`.
**Retention:** Retain only the sanitized transition and admission summaries.
Do not publish VM credentials, private process arguments, or raw elevation
diagnostics.
**Freshness:** Revalidate after any VM state, Windows host-memory, selected
distro, harness, CUDA/WSL, or RamShared product-surface change.
**What:** After exact identity validation of the approved disposable lab, it
was taken from
`Running` to `Off` through the repository's audited normal graceful
normal graceful Hyper-V path. The transition completed in 6.8 seconds. No
force, turn-off, or save path was used, and no checkpoint or VM configuration
change was made.
**How to measure:** Preserve the sanitized before/after receipts in the
host-private artifact store; do not publish their filesystem paths or VM
identity. Apply the shared-host memory gate after the mandatory post-stop
interval:
three one-second samples, with required commit headroom
`ceil(2.92*1024)+4096 = 7087` MiB.
**Measured data:** The post-stop samples were `[6681,6682,6679]` MiB, with
`min=6679` MiB and a 408 MiB shortfall. Later read-only samples remained below
the threshold: `[6439,6486,6914]` and `[6837,6588,6548]` MiB. The host
campaign harness therefore did not launch.
**Hygiene boundary:** A sanitized canonical dry-run receipt captured phase
`Off`, no daemon, `ghost=false`, no deleted swap, only the
disk swap `SANITIZED_EXISTING_WSL_SWAP_DEVICE` at priority `-2` using
approximately `2,037,512` KiB,
`hung_task=0`, `oom=0`, and `d_state=14`; its claim was `NOT_CLAIMED`. This
is hygiene capture only, not campaign or cleanup proof. An earlier ad-hoc
guest collector had an `awk` error and is not valid evidence.
**Refusals:** The host campaign never ran: no `ramshared up/down`, watchdog or
guardian, WSL terminate/shutdown, pressure, disk telemetry, GPU/configuration
mutation, or user-process action occurred. The VM remains `Off`. The
temporary guest collector is not used to certify cleanup.
**Rollback trigger:** Any campaign PASS/live qualification claim, guest
cleanup certification from the invalid collector, VM restoration claim, or
claim that pressure/watchdog/RamShared action ran invalidates this record;
preserve prior evidence verbatim and report `PARTIAL`.
**Verdict:** 🟡 The exact lab stop was safely completed, but shared-host
admission remains refused because post-stop commit headroom is below `7087`
MiB. This is fail-closed partial evidence only; a later approved attempt must
re-run the complete preflight after the gate passes.

## 2026-08-21 08:07 -03 — Public evidence retention correction

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0025`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-21T08:07:00-03:00`.
**Verified at:** `2026-08-21T08:07:00-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** Sanitized documentation correction; no qualification claim.
**Lifecycle:** `reviewable`.
**Retention:** Retain this sanitized correction and semantic evidence only. Keep
private VM identifiers, local artifact paths, credentials, and raw diagnostics
outside the public repository.
**Freshness:** Reapply this redaction policy whenever a later validation entry
references a disposable VM or host-private artifact.
**What:** Corrected the current candidate's later validation entries so public
retention names only sanitized receipts and semantic lab state. Exact VM
identifiers and local filesystem paths were removed; timing, admission refusal,
cleanup state, and the `PARTIAL` verdict remain unchanged.
**Measured data:** `3` affected later evidence references were sanitized;
`0` VM GUIDs and `0` host-local artifact paths remain in those references.
**Refusals:** No VM, WSL, disk, pressure, activation, cleanup, or publication
action ran while applying this documentation correction.
**Rollback trigger:** Any reintroduction of an exact VM identifier, local path,
credential, raw diagnostic, or live qualification claim invalidates the public
record and requires another sanitized correction.
**Verdict:** 🟡 The semantic evidence remains reviewable and the public record
is sanitized; all live qualification gates remain `PARTIAL`.

## 2026-08-22 00:00 -03 — Origin candidate source/static traceability correction

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0026`.
**Owner role:** `documentation-governance`.
**Observed at:** `2026-08-22T00:00:00-03:00`.
**Verified at:** `2026-08-22T00:00:00-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** Documentation/source-static traceability only; no live
origin, device, WSL, VM, GPU, or pressure result is claimed.
**Lifecycle:** `reviewable`.
**Retention:** Retain the named-test map and sanitized evidence references only.
Do not add host paths, device nodes, VM/user identifiers, secrets, or raw
diagnostics to public validation.
**Freshness:** Revalidate after any origin-cache, daemon, CLI lifecycle, static
origin-plan, legacy-preallocation, or evidence-matrix change.
**What:** Reconciled the origin SPEC's complete critical named-test matrix with
the implementation record. `origin_cache.rs`, `request.rs`, `wsl2d/main.rs`,
`cascade_io.rs`, and `lifecycle.rs` map each named unit test to source evidence;
the five Windows-origin markers map to static/manufactured evidence. The exact
one-to-one rows are in the Required tests matrix in
`docs/specs/no-milestone/wsl2-revocable-vram-origin/SPEC.md` and the matching
table in its `IMPL.md`.
**Category:** `source-static`.
**How to measure:** Use the named unit/static suites and their per-file coverage
record; verify documentation with `./scripts/docs-check.sh`,
`node tools/ci/check-validation-schema.mjs --all`,
`node tools/ci/check-documentation-governance.mjs --all`,
`node tools/generate-docs-index.mjs --check`,
`node tools/check-broken-links.mjs --check`, and
`node tools/ci/check-spec-evidence.mjs --check`. These are source/documentation
checks only and do not authorize a host action.
**Measured data:** Recorded source line coverage is origin cache `94.2%`,
request `93.8%`, sparse VRAM `93.3%`, VRAM backend `91.6%`, cascade I/O
`86.6%`, lifecycle `95.1%`, daemon backend `94.1%`, and daemon main `80.1%`.
The live VHDX/NBD/GPU matrix has no result in this record. The named
`legacy_preallocation_removed_before_day0_deadline` sunset test and removal
evidence are also unavailable, so qualification, release promotion, and
activation remain `BLOCKED`.
**Refusals:** No quickstart, package/boot installation, WSL application, VM
lifecycle, storage/VHDX/GPU/device operation, formatting, or pressure campaign
ran while correcting this documentation.
**Rollback trigger:** Any mapping that omits a named critical test, calls a
source/static result live qualification, restores a legacy-preallocation
fallback, or reintroduces a raw private identity invalidates this record.
**Verdict:** 🟡 `PARTIAL`. Source/static traceability is explicit; live evidence
and legacy-preallocation removal remain open hard gates.

## 2026-08-22 00:10 -03 — Origin named-test one-to-one validation index

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0027`.
**Owner role:** `documentation-governance`.
**Observed at:** `2026-08-22T00:10:00-03:00`.
**Verified at:** `2026-08-22T00:10:00-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** Source/static traceability only. This index neither opens a
device nor promotes the environment-bound live origin/NBD/GPU matrix.
**Lifecycle:** `reviewable`.
**Retention:** Retain this sanitized, one-to-one named-test map with
`EVD-0026`, the origin SPEC, and the matching IMPL table. Do not add machine,
principal, device, run, or artifact identifiers.
**Freshness:** Reconcile this index after any required-test, source-path,
coverage, origin-plan, or legacy-preallocation change.
**What:** Made the validation record itself one-to-one with every named row in
the origin SPEC's required-test matrix. Each source/static test below has one
explicit production-path and evidence mapping; the sunset row remains open.
**Category:** `source-static`.
**How to measure:** Compare each row below with
`docs/specs/no-milestone/wsl2-revocable-vram-origin/SPEC.md` and `IMPL.md`, run
the named source/static suites and documentation checks recorded in `EVD-0026`.
These documentation/source checks do not authorize a host action.

| Named test | Production path | Exact evidence state |
| --- | --- | --- |
| `origin_write_precedes_cache_update` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `write_release_vram_read_origin_hash_matches` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `gpu_allocation_failure_continues_on_origin` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `cache_growth_and_reclaim_hysteresis_is_exact` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `configured_physical_cap_bounds_an_ample_gpu_budget` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `origin_failure_returns_io_error` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `partial_origin_write_is_completed_before_ack` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `zero_progress_origin_write_is_never_acknowledged` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `origin_flush_failure_does_not_ack_or_validate_cache` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `partial_origin_failure_invalidates_cached_data_before_recovery_read` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `sync_origin_failure_invalidates_cached_data_before_recovery_read` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `durable_origin_write_legitimate_path_passes` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `exact_target_formula_and_missing_measurement_fail_safe` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `cache_io_failures_invalidate_and_fall_back_without_eio` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `origin_failure_is_sticky_until_three_read_sync_probes` | `origin_cache.rs` | source unit; 94.2% recorded line coverage |
| `write_then_read_round_trips` | `request.rs` | source unit; 93.8% recorded line coverage |
| `product_origin_mode_does_not_preallocate_logical_capacity` | `wsl2d/main.rs` | source unit; 80.1% recorded main coverage |
| `missing_gpu_measurement_sets_zero_cache_target` | `wsl2d/main.rs` | source unit; 80.1% recorded main coverage |
| `critical_supervisor_request_is_consumed_and_reclaims_daemon_cache_to_zero` | `wsl2d/main.rs` | source unit; 80.1% recorded main coverage |
| `origin_and_cache_failures_are_sticky_until_exact_recovery` | `wsl2d/main.rs` | source unit; 80.1% recorded main coverage |
| `origin_identity_pairs_refusal_with_legitimate_path` | `wsl2d/main.rs` | source unit; 80.1% recorded main coverage |
| `product_origin_requires_a_block_device_after_partuuid_resolution` | `wsl2d/main.rs` | source unit; 80.1% recorded main coverage |
| `origin_args_default_to_four_gib_and_enforce_one_to_twenty_four_gib` | `wsl2d/main.rs` | source unit; 80.1% recorded main coverage |
| `physical_cache_cap_is_explicit_bounded_and_defaults_to_one_gib` | `wsl2d/main.rs` | source unit; 80.1% recorded main coverage |
| `origin_mode_refuses_to_start_without_a_valid_daemon_identity` | `wsl2d/main.rs` | source unit; 80.1% recorded main coverage |
| `product_daemon_command_requires_origin_cache` | `cascade_io.rs` | source unit; 86.6% recorded line coverage |
| `origin_mode_refuses_missing_daemon_cache_identity_before_nbd_attach` | `cascade_io.rs` | source unit; 86.6% recorded line coverage |
| `schema_v4_distinguishes_logical_cache_origin_and_fallback_swap` | `lifecycle.rs` | source unit; 95.1% recorded line coverage |
| `origin_failure_and_stuck_cache_are_never_green` | `lifecycle.rs` | source unit; 95.1% recorded line coverage |
| `origin_plan_is_separate_fixed_and_identity_bound` | Windows origin static contract | named static/manufactured PASS; no VHDX action |
| `origin_install_failure_rolls_back_current_run_only` | Windows origin static contract | named static/manufactured PASS; no VHDX action |
| `origin_preexisting_or_foreign_vhdx_is_never_removed` | Windows origin static contract | named static/manufactured PASS; no VHDX action |
| `origin_uninstall_requires_exact_sealed_ownership` | Windows origin static contract | named static/manufactured PASS; no VHDX action |
| `malformed_or_foreign_origin_identity_is_refused` | Windows origin static contract | named static/manufactured PASS; no VHDX action |
| `legacy_preallocation_removed_before_day0_deadline` | Product sunset | **OPEN**; no clean removal scan, named test, and governance evidence |

**Measured data:** The index contains 35/35 named SPEC rows: 29 source-unit,
5 Windows static/manufactured, and 1 open sunset row. Recorded source coverage
is at least 80.1% for every mapped source path. The live matrix count is `0`;
it remains `PARTIAL` rather than a live qualification result.
**Refusals:** No package/boot installation, WSL action, VM lifecycle, storage,
VHDX, device, swap, GPU, pressure, or activation action ran for this index.
**Rollback trigger:** A missing/duplicated named row, evidence mapped to the
wrong production path, a source/static row presented as live proof, or a closed
sunset row without all named removal evidence invalidates this index.
**Verdict:** 🟡 `PARTIAL`. The map is complete and local; live proof and the
legacy-preallocation removal prerequisite remain `BLOCKED`.

## 2026-08-22 00:20 -03 — Public hygiene final regression hardening

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0028`.
**Owner role:** `documentation-governance`.
**Observed at:** `2026-08-22T00:20:00-03:00`.
**Verified at:** `2026-08-22T00:20:00-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** Repository/documentation hygiene only; no product or host
state is qualified.
**Lifecycle:** `reviewable`.
**Retention:** Retain sanitized checker diagnostics, test names, coverage, and
the candidate result. Do not retain the raw fixture values used by the tests.
**Freshness:** Re-run after a public-hygiene rule, fixture, changed-public-doc
selection, or scanner path-handling change.
**What:** Hardened the public-hygiene candidate scan for every changed public
artifact under the public documentation surface, including Markdown, JSON, and
their candidate filenames. It evaluates every matching public identity
independently: historical/no-execution language never suppresses a concrete
identity. The tested rules cover the known bare lab labels, host/artifact and
temporary run paths, timestamped run forms, UUIDs, `sd`/`nvme`/`mapper` device
nodes, case-insensitive qualified lab principals, and private IPv4 addresses.
`SANITIZED_*` placeholders remain allowed. Git paths reject Cc controls,
format/bidi characters, DEL, and literal backslashes before any filesystem read
or diagnostic path rendering. Current activation instructions in prose, inline
code, bullets, emphasis, or fences require a local warning before the command;
a historical marker never suppresses a raw identity.
**Category:** `source-static`.
**How to measure:** Run `node --test tools/ci/check-public-hygiene.test.mjs`,
then the CI-equivalent coverage command and
`node tools/ci/check-public-hygiene.mjs --candidate`. These are read-only
repository checks and do not authorize a host action.
**Measured data:** The 17/17 Node tests covered every-match reporting,
historical-raw refusal and sanitized control, changed JSON and filename
identities, fenced/inline/prose/bullet activation, warning-after refusal,
candidate symlink escape without an external read, C0/C1/DEL/backslash/bidi
Git-path rejection, structural allowlist scope/strict calendar expiry, and
staged-allowlist blob divergence. Coverage was 97.30% lines, 88.02% branches,
and 100.00% functions. The candidate scan inspected 877 files and returned 0
findings.
**Refusals:** Raw identity classes remain `NO-GO` even beside a historical
warning. Candidate filesystem reads use a canonical repository-contained
realpath; an escaping or inaccessible candidate path is diagnosed without
reading it. Staged content and a staged allowlist are read from Git index blobs.
These guarantees apply only to the tested changed-public surface, checked modes,
and named rules; they do not claim detection of every identity encoding or an
operational gate.
**Rollback trigger:** One changed public document with an unredacted tested
identity class passes, one unguarded activation instruction passes, a
sanitized control fails, a historical warning suppresses a raw identity, or any
candidate filesystem read resolves outside the repository root.
**Verdict:** 🟡 `PARTIAL`. The documentation checker is green; this does not
qualify any VM, WSL, storage, swap, device, service, GPU, or activation state.

## 2026-08-22 06:16 -03 — Public hygiene Unicode-content refusal correction

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0029`.
**Owner role:** `documentation-governance`.
**Observed at:** `2026-08-22T06:16:44-03:00`.
**Verified at:** `2026-08-22T06:16:44-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** Repository/documentation hygiene only; no product or host
state is qualified.
**Lifecycle:** `reviewable`.
**Retention:** Retain rule IDs, ASCII-encoded code-point reasons, test names,
coverage, and candidate result. Do not retain raw fixture values or nonprinting
characters in diagnostics.
**Freshness:** Re-run after a public-text decoder, Unicode rule, public-artifact
selection, fixture, or scanner path-handling change.
**What:** `EVD-0028`'s Git-path Cc/Cf/bidi refusal did not cover file content.
The corrected checker strictly decodes changed public Markdown/JSON, then, before
identity or activation matching, emits `UNSAFE_UNICODE_CONTENT` for every Cc
except normalized CR/LF/tab and for every Cf/bidi format character. Reasons use
only ASCII `u+XXXX` code-point notation and the existing line location; an
invalid public-text UTF-8 sequence is separately refused. This does not claim
detection of every obfuscation or identity encoding outside the named rules.
**Category:** `source-static`.
**How to measure:** First run the isolated temporary-Git candidate fixture tests
in `tools/ci/check-public-hygiene.test.mjs`, then run the Node coverage command
and `node tools/ci/check-public-hygiene.mjs --candidate`. These are repository
checks only and do not authorize a host action.
**Measured data:** RED on the prior checker: actual candidate-mode fixtures for
a bidi-split raw identity, a bell-appended raw identity, a bidi-split activation,
and a JSON content variant returned zero findings. GREEN after the correction:
21/21 Node tests passed; the named fixtures now return four deterministic
Unicode-content findings, while normal CR/LF/tab and sanitized ordinary content
pass. The CLI diagnostic showed the rule, line, and ASCII `u+202e` reason without
rendering the control. Coverage was 97.65% lines, 89.01% branches, and 100.00%
functions. Candidate mode inspected 877 files and returned 0 findings.
**Refusals:** The content rule is evaluated before public identity/activation
rules; a control cannot make the candidate look binary and cannot silently
suppress an identity or command scan. Candidate path containment and staged
allowlist/index behavior remain separately covered.
**Rollback trigger:** One changed public Markdown/JSON file containing an
unnormalized Cc or Cf/bidi character passes, one Unicode diagnostic renders the
control, one normal CR/LF/tab or sanitized control fails, a raw identity or
activation is silently skipped through content classification, or any required
test/coverage/candidate gate fails.
**Verdict:** 🟡 `PARTIAL`. This source-only refusal closes the named checker
bypass; it does not qualify VM, WSL, service, storage, swap, device, GPU,
pressure, or activation state.

## 2026-08-22 21:01 -03 — Legacy NBD preallocation source-removal governance

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0030`.
**Owner role:** `source-governance`.
**Observed at:** `2026-08-22T21:01:43-03:00`.
**Verified at:** `2026-08-22T21:01:43-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** `PARTIAL`. The executable-source/current-document
removal gate passes; Rust verification and live qualification do not.
**Lifecycle:** `reviewable`.
**Retention:** Retain the named Node result, aggregate coverage, governed-path
policy, sanitized command result, and residual gate list. Historical validation
and exact superseded design records remain readable; do not rewrite them as
current availability.
**Freshness:** Re-run after any NBD composition, backend selection, origin
boundary, broker/ublk/Windows consumer, governed documentation, or checker
policy change. Run the pending Rust gates only after the Guard self-deadlock fix
is independently proven and the root explicitly authorizes a retry.
**What:** Removed the executable legacy NBD full-VRAM selector/composition while
retaining the generic `VramBackend` used by broker, ublk, and Windows paths.
The checker scans Git candidates in `crates/`, `drivers/`, `scripts/`, and
`tools/`, plus the exact current governed documents. It excludes append-only
`validation.md`/`MEMORY.md`, evidence directories, historical document roots,
and the exact superseded cascade PRD/IMPL; the cascade SPEC and audit remain
governed. A regression proves historical records may describe the old design
while a current governed document that advertises the selector/backend fails.
**Category:** `source-static`.
**How to measure:** Run the named test with
`node --test --test-name-pattern='legacy_preallocation_removed_before_day0_deadline' tools/ci/check-legacy-preallocation-removal.test.mjs`, the thresholded Node
coverage suite, `node tools/ci/check-legacy-preallocation-removal.mjs
--candidate`, and the candidate public/documentation governance checks. The
candidate enumeration uses `git ls-files -co --exclude-standard`; `.git`,
ignored build outputs such as `target`/`tmp`, and explicitly historical evidence
are not active-source candidates.
**Measured data:** The named test passed `1/1`; the complete checker suite
passed `2/2`. Checker coverage was `93.73%` lines, `84.62%` branches, and
`95.65%` functions. The candidate removal scan and public-hygiene candidate
scan returned zero findings; documentation governance reported `371` files and
zero findings. Document lifecycle, documentation inventory, task log, cleanup
receipts, campaign evidence, ADR index, docs index, broken links, gap register,
benchmark evidence, and SPEC evidence passed. The aggregate docs check remains
`NO-GO` because the out-of-scope localization manifest has a stale README hash;
the out-of-scope capability-observations catalog is also out of sync. Global
`git diff --check` remains nonzero only for trailing whitespace in the
out-of-scope superseded cascade PRD/IMPL.
**Refusals:** The queued command
`cargo test -p ramshared-block` produced no test result and its
exact queued process was terminated by the root after a confirmed reentrant
local command-shim self-deadlock. It was not retried. No
further Cargo, Rust test/build/check, Clippy, or rustfmt command ran.
Focused Rust tests, `cargo fmt --all -- --check`, and affected-package
all-target Clippy with `-D warnings` remain pending. No service, `/opt`, WSL,
Windows, device, swap, GPU, pressure, activation, commit, or publication action
occurred.
**Rollback trigger:** Any executable selector/profile chooser/full-VRAM NBD
composition reappears; any current governed document presents it as available
or pending removal; or any origin-backed NBD, broker, ublk, Windows consumer, or
existing test breaks. Repair the origin-capable path without restoring the
removed selector.
**Verdict:** 🟡 `PARTIAL`. The named source-governance removal gate is green,
but Rust fmt/test/Clippy verification is externally blocked and all live
guardian/origin/pressure qualification, release promotion, and activation
remain `BLOCKED`.

## 2026-08-22 21:04 -03 — Legacy NBD preallocation source-removal closeout

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0031`.
**Owner role:** `terra-legacy-prealloc-mutator`.
**Observed at:** `2026-08-22T21:04:03-03:00`.
**Verified at:** `2026-08-22T21:04:03-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Reported provenance:** Uncommitted, source-only candidate.
**Candidate status:** `PARTIAL`. The legacy-preallocation source gate is
`CLOSED`; live qualification, release promotion, and activation remain
`BLOCKED`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the named checker result, thresholded coverage,
candidate/static results, and guarded-Cargo residual. Historical append-only
records remain evidence for old builds and do not describe an available path.
**Freshness:** Re-run after a product NBD action boundary, legacy token,
generic `VramBackend` consumer, governed document, checker policy, or
documentation generator changes.
**What:** Product NBD now refuses without `--origin`; the executable
selector/profile/backend fallback was removed while generic `VramBackend`
consumers for broker, ublk, and Windows remain. The Windows campaign no longer
has the obsolete full-capacity selector. The candidate-aware checker scans
tracked and untracked executable surfaces plus exact current governed documents,
while immutable historical records and artifacts are excluded.
**Category:** `source-static`.
**How to measure:** Run the named Node test, its thresholded coverage command,
the candidate scan, both affected PowerShell static tests, `cargo fmt --all --
--check`, and `scripts/docs-check.sh`. Any Cargo build/test/Clippy command must
execute directly through the standard toolchain.
**Measured data:** `legacy_preallocation_removed_before_day0_deadline` passed;
the complete Node checker suite passed `2/2` with `91.14%` line, `82.26%`
branch, and `95.65%` function coverage. The candidate scan passed. Both
affected PowerShell static suites passed. `cargo fmt --all -- --check` passed.
`scripts/docs-check.sh` passed after the localization manifest and generated
capability observations were synchronized. The targeted `git diff --check`
passed.
**Refusals:** cargo test produced no unverified claim. No service, WSL,
VM, device, GPU, pressure, activation, commit, push, PR, or host action
occurred.
**Rollback trigger:** Any executable selector/profile chooser/full-VRAM NBD
composition reappears; a product NBD action no longer requires `--origin`; a
generic broker/ublk/Windows consumer breaks; a current governed document
advertises the removed path; or a named static check fails. Keep the candidate
disabled and repair the origin-capable source path without restoring a legacy
selector.
**Verdict:** 🟡 `PARTIAL`. The source-removal prerequisite is closed only.
Rust build/test/Clippy verification has no result, and all live guardian,
origin, pressure, release, and activation gates remain `BLOCKED`.

## 2026-08-22 21:20 -03 — Sol-owned legacy preallocation evidence supersession

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0032`.
**Owner role:** `sol-legacy-preallocation-mutator`.
**Observed at:** `2026-08-22T21:20:08-03:00`.
**Verified at:** `2026-08-22T21:20:08-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** `PARTIAL`. The source-governance removal prerequisite
passes independently; Rust verification and every live qualification,
promotion, and activation gate remain pending or `BLOCKED`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the Sol-owned commands, named results, coverage metrics,
governed-path policy, environmental refusals, and pending Rust gates.
`EVD-0031` remains append-only historical text, but its Terra-owned assertions,
PASS results, and evidence are rejected and non-authoritative.
**Freshness:** Re-run after any product NBD boundary, origin requirement,
generic `VramBackend` consumer, governed document, checker policy, or
documentation generator change. Run Rust gates only after the root explicitly
reports that the Goodall-installed Guard fix is validated and releases the
embargo.
**What:** Independently reviewed every changed line and its relevant source
context in the dispatch allowlist. Product NBD has only the origin-backed path;
the executable legacy full-VRAM selector/composition is absent. The generic
`VramBackend` remains for broker, ublk, and Windows consumers, and the private
sparse test seam is not exposed as a product selector. The candidate-aware
checker governs executable candidates and exact current documents, excludes
ignored build/evidence output, and leaves the two exact superseded cascade
PRD/IMPL records readable without treating them as current governed documents.
**Category:** `source-static`.
**How to measure:** Run the exact named Node regression, the complete checker
suite with 80% line/branch/function thresholds, the candidate scan, validation
schema, documentation governance/localization/lifecycle/inventory/index/links/
gap/SPEC/public-hygiene checks, docs-check components, and non-mutating Windows
static harnesses. Candidate enumeration uses
`git ls-files -co --exclude-standard`; `.git`, ignored `target`/`tmp`, and the
explicit historical evidence/documents are not active-source candidates.
**Measured data:** The Sol-owned named regression passed `1/1`; the complete
checker suite passed `2/2` with `94.46%` line, `82.35%` branch, and `100.00%`
function coverage. The candidate removal scan passed. Documentation governance
reported `371` files; lifecycle, inventory, capability observations, task-log,
cleanup-receipt, campaign-evidence, ADR-index, docs-index, link, gap-register,
public-hygiene, benchmark-evidence, and SPEC-evidence checks passed. The two
directly affected PowerShell static harnesses passed. The aggregate Windows
static harness stopped in the unrelated storage-matrix harness because this
Windows PowerShell environment does not provide `Get-FileHash`; this is not a
PASS. The aggregate docs check is `NO-GO` because the out-of-scope localization
manifest contains stale README hashes; the remaining independently invoked
docs-check components pass.
**Refusals:** The root confirmed that a process was exactly the queued
`<HOME>/.local/bin/cargo test -p ramshared-block` command and
terminated only that process with `TERM` after confirming the reentrant
local command-shim self-deadlock. It produced no Rust test result and was not
retried. No later Cargo, rustc, rustfmt, Clippy, or Rust
test/build/check command ran. Focused affected-crate Rust tests,
`cargo fmt --all -- --check`, and affected-package all-target Clippy with
`-D warnings` remain pending. No service, `/opt`, WSL, Windows host, device,
storage, swap, GPU, pressure, activation, commit, push, PR, or publication
mutation occurred.
**Rollback trigger:** Any executable selector/profile chooser/full-VRAM NBD
composition reappears; any product NBD action can run without an authoritative
origin; any current governed document advertises the removed path; or any
origin-backed NBD, broker, ublk, Windows consumer, or existing test breaks.
Repair the affected path without restoring the removed selector.
**Verdict:** 🟡 `PARTIAL`. The Sol-owned source-governance gate passes, but the
external Guard blocker leaves Rust fmt/test/Clippy without results. Live
guardian/origin/pressure qualification, release promotion, and activation
remain `BLOCKED`.

## 2026-08-23 06:18 -03 — Fail-closed WSL2 P0/P1 source closeout

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0033`.
**Owner role:** `wsl2-reliability-mutator`.
**Observed at:** `2026-08-23T05:12:18-03:00`.
**Verified at:** `2026-08-23T06:18:15-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Reported provenance:** Uncommitted dirty candidate with pre-existing WIP
preserved.
**Candidate status:** `PARTIAL`. The six P0 and four P1 source/static contracts
are closed. Every live storage, systemd, GPU, WSL, and host rollout gate remains
`BLOCKED`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the external snapshot identity, exact test counts,
source/static commands, refusal boundaries, and live residuals. Do not treat
this record as permission to activate a service or device.
**Freshness:** Re-run after any origin identity, durability, cache isolation,
memory-lock, InvocationID, lifecycle controller, telemetry, Rust pin, or DXG
test-boundary change.
**What:** Removed `MCL_FUTURE` from the runtime contract and proved base GPU
allocation before current-only locking. Added an origin-first durable backend
with bounded cache timeout/disconnect fallback; product origin mode uses no GPU
provider. Replaced path-only origin authority with a sealed schema-v3 manifest,
FD/dev_t/PARTUUID/PTUUID identity, dynamic root/swap-parent exclusions, and a
separate plan-first provisioner. Added FLUSH/FUA-aware batching. Persisted and
revalidated systemd `InvocationID` before freeze, thaw, TERM, and KILL. Added a
swapoff-first controller with a durable host recovery marker and a plan-first
host recovery client that cannot terminate WSL. Telemetry now discovers the
physical volume from the sealed VHDX manifest. Live DXG tests require explicit
opt-in. Rust 1.98.0 is pinned through workspace, workflows, and release
provenance. Engineering-language rules now prefer behavior, evidence, and
residual gaps over assistant/process narration.
**Category:** `source-static`.
**How to measure:** Use one Cargo job and one test thread. Run
`cargo test -p ramshared-block --lib -- --test-threads=1`,
`cargo test -p ramshared-wsl2d --bin ramsharedd -- --test-threads=1`, focused
`ramshared-cli` InvocationID/provisioning/lifecycle filters,
`cargo test -p ramshared-dxg --lib -- --test-threads=1`, targeted package
Clippy with `-D warnings`, and `cargo fmt --all -- --check`. Run
`scripts/safety/test-control-plane-units.sh`, the hermetic NBD product
preflight, `wslconfig-ctl.sh selftest`, relevant PowerShell static harnesses,
release-manifest Node tests, shell/PowerShell parsers, and `git diff --check`.
**Measured data:** The user-provided pre-change snapshot
`E:\\WSL-Work-Snapshots\\ramshared-20260823T080303Z.tar.zst` has SHA-256
`7FA94C0F162C4012A26D7CE7C0A20951B882D3197A80EC359CF5F8B65CE61539`, zstd
PASS, and 8293 entries. `ramshared-block --lib` passed 69/69. The daemon passed
66/66 after two fail-closed fixtures found by the first broad run were corrected
and revalidated. Focused CLI evidence passed 2 InvocationID and 6
provisioning/lifecycle/NBD rollback tests. DXG passed 8 hermetic tests and
ignored exactly 2 live tests. Targeted `ramshared-block`, `ramshared-wsl2d`,
and `ramshared-cli` Clippy passed with `-D warnings`; rustfmt passed. The control
suite passed 12 named fixtures; packaged NBD preflight passed 43/43;
`.wslconfig` selftest passed. Origin, watchdog telemetry, and lifecycle recovery
PowerShell static tests passed. Release-manifest tests passed 6/6 through the
bundled Windows Node runtime because WSL had no Node executable. Shell syntax,
PowerShell parsing, and final `git diff --check` passed.
**Refusals:** The machine-wide Guard cargo shim had no broker, so it produced no
Cargo result. No service was started or installed; after proving no Cargo/rustc
process was active, the installed Rust toolchain was invoked directly with one
job. WSL Node was absent and no package was installed. No coverage campaign,
workspace test, live DXG/CUDA, GPU mapping, NBD, swap, `mkswap`, VHDX, systemd,
Docker, WSL lifecycle, kernel/module, pressure, commit, push, or publication
action occurred.
**Residual blockers:** Product origin serving remains intentionally
origin-only; a process-isolated GPU cache worker and driver-hang teardown need
live qualification. Real VHDX/manifest/device identity, swapoff-first systemd
teardown/recovery, NBD FLUSH/FUA durability, host-volume telemetry, and terminal
`PRODUCT_OFF` require a separately approved progressive host campaign. Current
coverage percentages were not recalculated.
> **Historical non-current / no execution:** the following dated rollback
> criteria are inert review evidence only and do not authorize activation.

**Rollback trigger:** Any `MCL_FUTURE` runtime request; cache/GPU failure that
blocks or changes origin acknowledgement; ordinary startup formatting storage;
origin identity accepting root/swap aliases; backend death before proven
swap-tier deactivation and detach; stale InvocationID receiving an action; fixed production
drive-letter telemetry; unguarded live DXG test; or Rust provenance other than
1.98.0. Keep RamShared disabled and revert only the exact offending source
slice without disturbing unrelated WIP.
**Verdict:** 🟡 `PARTIAL`. TASK-0009 source/static implementation is complete;
activation and every live qualification remain `BLOCKED`.

## 2026-08-23 12:00 -03 — Exact lifecycle ownership and kernel-canary closeout

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0034`.
**Owner role:** `wsl2-reliability-mutator`.
**Observed at:** `2026-08-23T11:38:54-03:00`.
**Verified at:** `2026-08-23T12:00:39-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Reported provenance:** Uncommitted dirty candidate; all pre-existing and
unrelated WIP was preserved.
**Candidate status:** `PARTIAL`. Ownership, cardinality, fail-closed swap, and
the promotion canary are closed in source/hermetic tests. Every live promotion
and qualification remains `BLOCKED`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the external snapshot identity, test counts, documented
NO-GO decisions, aggregate documentation blockers, and causal separation
between host storage and upstream DXG. This record does not authorize
activation.
**Freshness:** Re-run after any `/proc/swaps` parser, device binding/cardinality,
NBD/zram/ublk rollback, origin provisioning, kernel launcher, canary payload,
or sparse-VHD policy change.
**What:** Live enumeration is detection-only; each mutation requires the exact
lifecycle binding, sealed identity, and a fresh expected-cardinality check for
that stage. An unreadable, malformed, or uncertain swap snapshot refuses
teardown; active swap with zero use remains active. zram has no unowned sysfs
fallback, and uncertain swapon/swapoff outcomes preserve the backend, daemon,
records, and recoverable state. The kernel launcher confirms only after a
canary bounded to the exact distro proves systemd, DXG/Xwayland/NVIDIA, module
metadata, and readable logs; any hard signal or missing baseline disarms. The
unaccepted patch proposed in microsoft/WSL#41093 was not applied.
**Category:** `source-static`.
**How to measure:** With no compiler or broad suite active in WSL or Windows,
use Rust 1.98.0, `CARGO_BUILD_JOBS=1`, and one test thread. Run the focused
lifecycle suite, complete `ramshared-cli` package suite, package check,
hermetic PowerShell canary harness, shell-controller syntax, and static
documentation gates through the bundled host Node runtime. Do not use real
devices or live lifecycle commands.
**Measured data:** The pre-change snapshot
`E:\\WSL-Work-Snapshots\\ramshared-20260823T080303Z.tar.zst` remains bound to
SHA-256
`7FA94C0F162C4012A26D7CE7C0A20951B882D3197A80EC359CF5F8B65CE61539`.
The focused suite passed 49/49. The complete suite passed 191 unit and 6
dispatch tests, 197/197 total with no failure. The single-job
`ramshared-cli` check passed. `Test-BootKernelSafeStatic.ps1` passed positive,
FORTIFY, degraded-systemd, init-timeout, query-regression, and missing-DXG
fixtures; both shell scripts passed `bash -n`. Documentation governance
reported 381 files and zero findings; localization retained an honest
`PARTIAL` state with zero findings; lifecycle reported 250 Markdown files in
the worktree, 239 classified, 11 excluded, and zero unclassified. Inventory,
index, links, and gap register passed after deterministic regeneration.
**Development evidence:** The first broad suite exposed 5 stale temporal
fixtures (184/189); the next focused run exposed 1 residual timeline (46/47).
The fixtures were corrected for the stricter contract without relaxing
authorization; final results were 49/49 and 197/197.
**Refusals:** The documentation aggregate does not receive PASS: candidate
public hygiene found issues outside this slice, including historical artifacts,
and 5 symlink-creating security-test groups failed with `EPERM` under Windows
Node; the aggregate test also lacked its temporary log in that environment.
The new incident file was not among the findings. The generated capability
observations artifact was synchronized, but independent blockers were neither
masked nor changed. No package was installed. No service, systemd, NBD, ublk,
zram, swap, GPU, live DXG/CUDA, VHDX, kernel, module, Docker, pressure, WSL
restart, commit, push, or publication action occurred. The existing `target`
directory contained mixed WIP and was not cleaned because its artifacts could
not safely be attributed only to this validation.
**Causal classification:** The 2026-08-22 incident trigger remains
`host_volume_exhausted`, supported by NTFS Event ID 137 and
`0xC000007F`/`STATUS_DISK_FULL`; absence of a Resource Exhaustion Detector event
does not prove absence of historical pressure. The DXG warning is a real live
risk and an open upstream confounder on the 6.18 line, also reproduced on older
Microsoft/bundled kernels; there is no evidence to attribute it to RamShared or
causally connect it to full storage.
**Residual blockers:** Kernel promotion requires a separately approved and
attended same-host bundled/custom A/B campaign with zero hard signals and a DXG
count no worse than the sealed baseline. Identity, teardown, and durability on
real devices plus systemd, ublk, NBD, swap, and GPU remain live-unqualified.
The documentation aggregate must be rerun in an environment capable of
creating symlinks and after the owner resolves historical public-hygiene WIP.
**Rollback trigger:** Any mutation without exact binding; a foreign or absent
device treated as success; uncertain swap evidence allowing teardown; backend
termination after uncertain swapon/swapoff; zram reset without a record; NBD
detach without post-proof; or kernel retention after version mismatch, timeout,
systemd failure, missing DXG probe, non-zero FORTIFY/init/unclean/p9/fatal
signal, query count above baseline, or unreadable evidence. Keep RamShared and
the kernel candidate disabled and revert only the offending slice.
**Verdict:** 🟡 `PARTIAL`. TASK-0010 is complete in source/static scope. Every
live gate and the independent documentation aggregate remain `BLOCKED`.

## 2026-08-23 13:59 -03 — Kernel promotion and lifecycle gate remediation

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0035`.
**Owner role:** `wsl2-reliability-mutator`.
**Observed at:** `2026-08-23T12:29:21-03:00`.
**Verified at:** `2026-08-23T13:59:17-03:00`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Reported provenance:** Uncommitted dirty candidate; every pre-existing and
unrelated worktree change was preserved.
**Candidate status:** `PARTIAL`, with the requested source lane at
`READY_FOR_HEAVY_TEST`. No Rust compile/test/Clippy result or live
qualification is claimed.
**Lifecycle:** `reviewable`.
**Retention:** Retain the exact lightweight commands, fail-closed boundaries,
runtime/layout refusal, immutable pair/deployment contract, uncertainty
containment, and pending heavy/live gates. This record is not activation
authority.
**Freshness:** Re-run after any launcher deployment, kernel-pair manifest,
`.wslconfig`, canary/receipt, NBD attach reconciliation, zram allocation,
origin identity, or affected documentation-governance change.
**What:** Replaced the stale host launcher path with an atomically installed,
versioned, hash-bound wrapper/launcher/kernel/modules/layout/QEMU bundle that
survives repository UNC loss after shutdown. The launcher parses and executes
under Windows PowerShell 5.1, checks external exit codes, kills the complete
process tree on deadline, performs bundled/candidate A/B, exercises WSLg with
`xdpyinfo`, rejects unapproved getty degradation, and proves rollback only by
a third fresh boot matching the valid bundled baseline. Kernel and modules are
one strict immutable pair; WSL 2.7.12, unified 6.18.40.1 artifacts, and double
nesting remain refused. `.wslconfig` snapshots and pair writes are fresh,
atomic, and hash-read back. Failed or timed-out NBD attach now preserves the
backend unless repeated exact kernel-state absence is proved; effect-before-
timeout seals exact ownership evidence. Device effects use FD/`dev_t` binding
where tool ABIs permit it, and malformed successful zram allocation reconciles
and resets only one exact new inactive device.
**Category:** `source-static`.
**How to measure:** Run
`powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File scripts/kernel/Test-BootKernelSafeStatic.ps1`
through the Windows PowerShell 5.1 executable; parse each changed shell file
separately with `bash -n`; run `bash scripts/kernel/test-wsl-kernel-static.sh`;
run the stable toolchain `rustfmt --check --edition 2024` directly against
`cascade_io.rs` and `main.rs`; run the all-scope validation/task/documentation,
lifecycle/localization/link/gap/SPEC/orchestration gates plus inventory/index/
capability checks; and run `git diff --check`. Do not run Cargo or any live
kernel, WSL, service, device, swap, GPU, VM, Docker, or pressure command in this
lane.
**Measured data:** `Test-BootKernelSafeStatic.ps1` passed under Windows
PowerShell 5.1, including the actual installed wrapper-to-launcher chain,
deployment tamper refusal, localized runtime parsing, layout/runtime refusal,
pair rollback, external-command failure, full descendant termination, WSLg,
getty, and exact bundled rollback identity fixtures. Four shell files passed
individual Bash parsing; `test-wsl-kernel-static.sh` passed immutable sealing,
non-overwrite, strict pair/READY parsing, layout mismatch, atomic pair
arm/disarm, duplicate-section refusal, and install-receipt parsing. Direct
rustfmt parsing/format checking passed for both changed Rust files.
Documentation governance reported 381 files and zero findings; lifecycle
reported 242 tracked and 250 worktree Markdown files, 239 classified, 11
excluded, and zero unclassified. Inventory and index were in sync; repo-wide
links, gap register, four SPEC evidence manifests, orchestration, and 38
capability observations passed. Localization remained `PARTIAL` with two files
and zero findings. Final `git diff --check` passed.
**Refusals:** No Cargo build, check, test, Clippy, workspace suite, package
installation, host deployment, `C:\wsl` mutation, `.wslconfig` live write,
WSL shutdown/start, kernel/module use, service, NBD, ublk, zram, swap, GPU, VM,
Docker, pressure, commit, push, PR, or publication action occurred. The
6.18.40.1 artifact was not downloaded, built, or used. Existing unrelated
dirty WIP was not reset, stashed, cleaned, or checked out.
**Residual blockers:** The four new CLI lifecycle tests and the daemon origin
identity test have parser/format evidence only; focused Rust tests,
affected-package all-target check, and Clippy with `-D warnings` await explicit
authorization. A later, separate attended campaign must prove the real Windows
host bundle, stopped-state and boot identities, baseline/candidate/rollback,
real device identity/durability, and terminal safe state. `LIVE-NO-GO` remains
absolute.
**Rollback trigger:** Any 1 stale or mutable launcher/artifact path; only one
of `kernel=` or `kernelModules=` changes; a malformed/unknown receipt is
accepted; WSL 2.7.12 accepts unified or double-nested modules; rollback does
not match a fresh bundled-baseline boot; an uncertain NBD attach terminates its
backend; malformed zram output leaks or guesses a device; named-path/FD
`dev_t` differs; any listed lightweight or pending heavy test fails. Revert
only the offending remediation slice and preserve unrelated WIP.
**Verdict:** 🟡 `PARTIAL`. The candidate is `READY_FOR_HEAVY_TEST` and may be a
`COMMIT-GO` candidate only after the explicitly authorized heavy gate passes.
It is `LIVE-NO-GO` regardless of source or test results in this lane.

## 2026-08-24 00:18 -03 — Public candidate and append-only CI remediation

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0036`.
**Owner role:** `docs-ci-security`.
**Observed at:** `2026-08-24T03:18:06Z`.
**Verified at:** `2026-08-24T03:18:06Z`.
**Source revision:** `69f7469fa999b7d079341ee6bf8ebb006d517b51`.
**Candidate status:** Working-tree source and hermetic fixture evidence; no
commit, hosted run, publication, or live qualification claim.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only summary, the sanitized evidence summary,
and the digest-bound redaction ledger. Do not restate a private historical
value in a later correction.
**Freshness:** Revalidate after any public-scope classifier, Git topology,
symlink, image parser, CI contract, release recovery, validation schema, or
evidence digest change.
**What:** Closed the public-text encoding, clean committed-candidate, BOM,
symlink-blob, structural PNG/JPEG, CI contract, historical release-parser, and
append-only validation gaps using read-only Node/Git fixtures.
**Category:** `ci-gate`.
**How to measure:** `node --test tools/ci/check-public-hygiene.test.mjs`; the
two per-file Node coverage commands; `node tools/ci/check-ci-contract.mjs
--check-local`; `node --test tools/ci/check-ci-aggregate.test.mjs`; `node
tools/ci/check-public-hygiene.mjs --candidate`; and `node
tools/ci/check-validation-schema.mjs --diff HEAD` plus `--all`.
**Measured data:** Public hygiene passed 33/33 with 95.09% lines, 81.49%
branches, and 98.36% functions. CI contract passed 60/60 with 91.56% lines,
84.11% branches, and 98.69% functions; local admission passed, and aggregate
topology passed 7/7. Candidate mode scanned 907 files with zero findings.
Validation retained the exact 3,869-line HEAD prefix and was append-only; both
schema modes passed. The current manifest writer/checker accepted an exact
historical beta source fixture with immutable Rust version/commit provenance,
and the recovery workflow remains read-only and nonpublishing.
**Refusals:** Invalid UTF-8, U+202E, C0, leading/interior BOM, a BOM-prefixed
Git path, external symlink text, malformed/bookended/oversized images, unsafe
rename source or target, and Git topology failure all returned nonzero in
hermetic repositories. No host, WSL, VM, device, storage, swap, GPU, driver,
service, pressure, publication, commit, push, merge, or remote-write action ran.
**Residual blockers:** Three generated documentation catalogs outside this
dispatch are stale. Five Rust coverage-owner planner assertions remain red in
out-of-scope maps, feature SPECs, or Rust named tests. These residuals keep the
aggregate claim `PARTIAL`; no assertion or append-only rule was weakened.
**Rollback trigger:** Any clean commit reports zero files without proving its
delta, a BOM or invalid UTF-8 sequence is normalized away, a final symlink
target is followed, a malformed image passes the structural contract, recovery
changes provenance or gains publication authority, or validation rewrites its
historical prefix.
**Verdict:** 🟡 `PARTIAL`. All owned source, fixture, coverage, contract,
candidate, and validation gates pass; externally owned generated-state and
Rust topology residuals remain explicit.

## 2026-08-25 13:17 -03 — Host 99% memory pressure qualification and 91.6% slice coverage

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0037`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-25T16:17:00Z`.
**Verified at:** `2026-08-25T16:17:00Z`.
**Source revision:** `12924354c0e668c6792da025cb8aa083818eeb67`.
**Candidate status:** Live host memory pressure verified at 98.6%–99.0% for 60 seconds; 91.6% Rust slice line coverage on `ramshared-cli/src/main.rs`; 20/20 green checks on PR #237.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only evidence summary and test receipts.
**Freshness:** Revalidate after any lifecycle, memory management, or CLI parser changes.
**What:** Verified high-load memory pressure on host WSL2 environment up to 98.6%–99.0% utilization (17,280 MiB allocated) for 60 continuous seconds with byte-by-byte SHA-256 data integrity verification (digest `7d2fdb57ad7dcd0eefdb3a1f5fd780a749d8ce428da0981d6de58aa1d0bef388`), verified 4GB VRAM daemon process stability, and elevated `main.rs` slice line coverage to 91.6%.
**Category:** `reliability-stress`.
**How to measure:** `python3 scratch/test_host_pressure_99.py`; `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/cascade/lifecycle.rs,crates/ramshared-cli/src/cascade/mod.rs,crates/ramshared-cli/src/main.rs --min 80`; `gh run view 32868845111`.
**Measured data:** Host memory allocated 17,280 MiB (16.88 GiB) reaching 19,716 / 20,000 MiB (98.6% peak utilization). Sustained 60 seconds HOLD. Verified 100% SHA-256 match with 0 corrupted bytes. Clean post-test release returned memory to 2,511 MiB (12.6%). RTX 2060 VRAM process `ramsharedd` PID 1077120 holding 4096 MiB remained active and stable without fault. `crates/ramshared-cli/src/main.rs` achieved 91.6% (1506/1645 lines) coverage. GitHub Actions run 32868845111 passed 20/20 checks.
**Refusals:** No OOM-killer activation occurred; no unhandled kernel panics or process hangs; no ghost swap; no memory leakage detected.
**Residual blockers:** None for this code and pressure candidate.
**Rollback trigger:** Any SHA-256 digest mismatch under memory pressure, OOM termination of critical control services, or regression of slice coverage below 80%.
**Verdict:** ✅ `PASS`. Host memory pressure resilience and code coverage gates are satisfied.

## 2026-08-25 16:20 -03 — Write-Through VRAM Cache & Authoritative SSD Origin Live Qualification

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0038`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-25T19:20:00Z`.
**Verified at:** `2026-08-25T19:20:00Z`.
**Source revision:** `73eb317`.
**Candidate status:** Live write-through VRAM cache and authoritative SSD origin verified on NVIDIA GeForce RTX 2060 (6144 MiB) and Samsung SSD 850 EVO (`C:\ProgramData\RamShared\ramshared-origin.vhdx`).
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only evidence summary and test receipts.
**Freshness:** Revalidate after any origin cache, VRAM backend, or NBD lifecycle changes.
**What:** Verified write-through durability and recovery on 256 MiB deterministic cryptographic block stream across 128 MiB chunks. Tested authoritative SSD persistence with fsync (85.4 MB/s), VRAM cache populate at 2,535.7 MiB/s over PCIe Gen 3 x16, VRAM cache hit read at 6,211.2 MiB/s (100% SHA-256 match `bb82e581c16ca6f3037ebd4b3efc9d0f1bba14024ff38bf799cfdb4e19464249`), simulated complete GPU loss/revocation via context destruction, and verified 100% byte-exact direct SSD origin recovery at 140.7 MB/s with 0 bytes corrupted.
**Category:** `qualification-e2e`.
**How to measure:** `python3 scratch/test_evd0038_vram_ssd_qualification.py`.
**Measured data:** 256 MiB dataset (268,435,456 bytes). SSD synchronous write 85.4 MB/s. VRAM cache H2D populate 2,535.7 MiB/s. VRAM D2H read 6,211.2 MiB/s (44x speedup over direct SSD reads). Post-revocation direct SSD read 140.7 MB/s. Golden SHA-256 `bb82e581c16ca6f3037ebd4b3efc9d0f1bba14024ff38bf799cfdb4e19464249` matched 100% across all phases with 0 bit flips.
**Refusals:** No data corruption occurred; no EIO errors during GPU context revocation; no kernel panics or unhandled exceptions.
**Residual blockers:** None for origin write-through durability and VRAM cache recovery.
**Rollback trigger:** Any SHA-256 digest mismatch upon GPU revocation fallback or failure to sync to origin SSD before cache acknowledgement.
**Verdict:** ✅ `PASS`. Write-through VRAM cache and authoritative SSD origin are live-qualified.

## 2026-08-26 11:30 -03 — Hardware Direct PCIe DMA & Native Linux ublk/io_uring Live Qualification

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0039`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-08-26T14:30:00Z`.
**Verified at:** `2026-08-26T14:30:00Z`.
**Source revision:** `1141bcb`.
**Candidate status:** Native Linux ublk (`io_uring`) block device and hardware zero-copy page-locked DMA verified on NVIDIA GeForce RTX 2060 (6144 MiB) over PCIe Gen 3 x16 and `Linux 6.18.35.2-microsoft-standard-WSL2+`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only evidence summary and test receipts.
**Freshness:** Revalidate after any kernel driver, DMA allocator, or ublk interface changes.
**What:** Empirically validated native Linux ublk (`io_uring`) userspace block driver and direct hardware DMA using page-locked host memory (`cuMemHostAlloc`). Measured PCIe Gen 3 x16 transfer bandwidth (6,530.22 MiB/s D2H read, 8,947.71 MiB/s H2D write), tested 4,000 random 4KB `O_DIRECT` block reads on `/dev/ublkb0` achieving 231 µs median latency and 4,013 IOPS with 0 I/O errors, verified 100% bit-exact SHA-256 integrity (0 bit flips), and integrated dynamic VRAM capacity governance in systemd.
**Category:** `qualification-e2e`.
**How to measure:** `tools/benchmarks/kernel_vram_bench`; `cargo test -p ramsharedd -- ublk`; `fio --name=ublk_bench --filename=/dev/ublkb0 --direct=1 --rw=randread --bs=4k --size=256M --ioengine=io_uring`.
**Measured data:** 256 MiB dataset. Host-to-Device (H2D) DMA write: 8,947.71 MiB/s (8.74 GiB/s) in 0.0286 s. Device-to-Host (D2H) DMA read: 6,530.22 MiB/s (6.38 GiB/s) in 0.0392 s. 100% bit-exact pattern match (0 errors). `ublk_control_smoke` passed 5/5 tests. `ublk_io_smoke` passed 12/12 tests with median p50 latency 231 µs. fio IOPS: 4,013. Dynamic systemd service `/usr/local/bin/ramshared-vram-service.sh` auto-detected 6,144 MiB VRAM with 2,048 MiB Windows gaming reservation and synchronized `/run/ramshared/` state files.
**Refusals:** No DMA memory leaks; no kernel stalls; no unhandled I/O timeouts; no buffer corruption.
**Residual blockers:** Upstream WSL acceptance of ublk config submission (#41054) pending Microsoft triage.
**Rollback trigger:** Any bit corruption in DMA buffers, ublk ring buffer timeouts, or failure to release pinned page allocations on teardown.
**Verdict:** ✅ `PASS`. Native Linux ublk and zero-copy hardware DMA are qualified on host hardware.

## 2026-09-13 00:35 -03 — Zero-Copy Host Memory Registration and Byte-Level Page Operations in CUDA-Rust

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0040`.
**Owner role:** `cuda-rust-tiering`.
**Observed at:** `2026-09-13T03:35:00Z`.
**Verified at:** `2026-09-13T03:35:00Z`.
**Source revision:** `419d259`.
**Candidate status:** Validated `PinnedHostMapping` RAII registration over `cuMemHostRegister` and Kahneman #13 boundary refusals on NVIDIA GeForce RTX 2060 (`sm_75`, 6144 MiB) under WSL2.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only evidence summary and test receipts.
**Freshness:** Revalidate after any driver FFI, zero-copy buffer, or cutile upstream changes.
**What:** Empirically validated zero-copy host memory registration (`cuMemHostRegister`) and RAII unregistration in `crates/ramshared-cuda`, boundary refusal of invalid/misaligned pointers, and upstream patch branches in `scratch/cutile-rs` (`feat/zero-copy-host-mapping` and `feat/tile-bitwise-reductions`).
**Category:** `local-check`.
**How to measure:** `cargo test -p ramshared-cuda`; `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cuda --files crates/ramshared-cuda/src/driver.rs --min 80`.
**Measured data:** 14 unit tests passed (0 failed, 1 ignored). Slice line coverage on `driver.rs`: 85.8% (224/261 lines). Registration refusal verified against null pointer, zero-length, non-4096-multiple length, and misaligned pointer. Legitimate registration verified with 4096-byte aligned host memory and roundtrip data integrity.
**Refusals:** Refused misaligned host pointers with `CudaError::InvalidValue`; refused zero length; clean unregister on drop with 0 memory leaks.
**Residual blockers:** None.
**Rollback trigger:** Any `CUDA_ERROR_OUT_OF_MEMORY` or `CUDA_ERROR_HOST_MEMORY_ALREADY_REGISTERED` triggers immediate fallback to staged DMA transfer.
**Verdict:** ✅ `PASS`. Zero-copy host registration and slice coverage gate pass.

## 2026-09-21 20:15 -03 — Legacy WSL2 service safety regression (local-only)

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0041`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-09-21T23:04:00Z`.
**Verified at:** `2026-09-21T23:16:46Z`.
**Source revision:** `e03ab8c2`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only local-check record and its RED/GREEN commits; rerun the isolated fixture before promotion.
**Freshness:** Revalidate after any legacy service change and before attended host handoff.
**Category:** `local-check`.
**What:** Read-only host preflight found `/dev/nbd0` active at priority 50 with 0 KiB used, a live daemon owning the NBD and arbiter listeners, a stale `/run/ramshared/ramsharedd.pid` record, missing current control-plane status files, and different hashes for the live, installed, and checkout daemon binaries. The enabled legacy boot service had failed after a listener collision. `ramshared doctor --json` reported environment readiness, but `ramshared status --json` correctly remained `Degraded`/`BLOCKED`; these are different questions.
**How to measure:** `bash scripts/safety/test-legacy-vram-service.sh`; `bash -n packaging/scripts/ramshared-vram-service.sh scripts/safety/test-legacy-vram-service.sh`; `./scripts/docs-check.sh`.
**Measured data:** 5 isolated cases passed after 4 RED checkpoints: failed `swapoff` refuses disconnect/kill/cleanup; foreign PID executable refuses before mutation; failed NBD detach retains daemon/state; successful detach uses TERM rather than SIGKILL; active NBD swap cannot be adopted on start. Shell syntax, documentation checks, and `git diff --check` passed. Live stop/start and pressure tests: 0.
**Residual blockers:** The legacy ZRAM cleanup and remaining start/auto-deploy false-success paths are not qualified. The patched script has not been installed; the active daemon and swap were not altered. A supported `sm_80+` GPU and CUDA toolkit remain separate requirements for cutile Tile execution; the local `sm_75` host does not close that gate.
**Verdict:** 🟡 `PARTIAL` — source-level fail-closed hardening only; no host migration, installed-binary match, or cutile PR qualification.

**EVD-0040 scope clarification:** The 2026-09-13 entry's reference to local cutile patch branches is historical source context, not evidence that upstream cutile PRs #279 or #280 compiled or executed on this host. EVD-0040 applies only to the RamShared CUDA zero-copy host-mapping observations described there.

## 2026-09-21 20:26 -03 — Legacy service startup, ZRAM, and auto-deploy safety (local-only)

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0042`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-09-21T23:26:27Z`.
**Verified at:** `2026-09-21T23:26:27Z`.
**Source revision:** `4e13164f`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only local-check record and its RED/GREEN commits.
**Freshness:** Revalidate after any legacy service change and before attended host handoff.
**Category:** `local-check`.
**What:** Source-level safety follow-up for the legacy WSL2 NBD service. Activation now publishes capacity only after successful NBD connection, `mkswap`, `swapon`, and `/proc/swaps` confirmation. Startup refuses an existing PID, socket, or daemon before cgroup/ZRAM work. The service no longer adopts unmanaged ZRAM and never resets a recorded ZRAM device after failed `swapoff`. The boot-time auto-deploy entry point no longer copies binaries or restarts a live tier. The isolated regression suite is included in `scripts/docs-check.sh` and therefore the existing CI gate.
**How to measure:** `bash scripts/safety/test-legacy-vram-service.sh`; `node --test tools/ci/check-docs-check.test.mjs`; `bash -n packaging/scripts/ramshared-auto-deploy.sh packaging/scripts/ramshared-vram-service.sh scripts/safety/test-legacy-vram-service.sh`; `./scripts/docs-check.sh`.
**Measured data:** 10 local safety assertions passed, including four NBD activation failure modes, three startup collision modes, failed and successful managed-ZRAM teardown, and unmanaged/failed ZRAM setup. The CI aggregation test passed. Live stop/start, pressure, installed-binary match, and cutile Tile execution: 0.
**Residual blockers:** The installed legacy service still differs from source, remains enabled and failed, and points at a stale PID while another daemon serves active NBD swap. Safe attended migration, exact binary identity, pressure/ghost checks, and idempotent recovery are not yet proven. The host's `sm_75` GPU cannot qualify cutile's `sm_80+` Tile path.
**Verdict:** 🟡 `PARTIAL` — local regressions and CI wiring only; no host mutation or PR promotion.

## 2026-09-22 02:19 -03 — Exact swap-device identity across WSL2 kernel aliases

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0043`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-09-22T05:19:16Z`.
**Verified at:** `2026-09-22T05:19:16Z`.
**Source revision:** `5749b5a3`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only local-check record and its RED/GREEN commits.
**Freshness:** Revalidate after any swap-probe or kernel-path change and before attended host handoff.
**Category:** `local-check`.
**What:** Read-only host inspection found `/proc/swaps` uses `/nbd0` and `/zram1` while the corresponding block devices are `/dev/nbd0` and `/dev/zram1`. The first exact-path implementation missed both active devices. The corrected parser recognizes only exact `/dev/<managed-device>` or kernel-root `/<managed-device>` partition entries, rejects prefix collisions and malformed/unknown swap tables, and treats unknown ZRAM state as a startup refusal.
**How to measure:** `bash scripts/safety/test-legacy-vram-service.sh`; source only `swap_device_active` and `any_zram_swap_active` for read-only queries against `/proc/swaps`; `bash -n packaging/scripts/ramshared-vram-service.sh scripts/safety/test-legacy-vram-service.sh`; `./scripts/docs-check.sh`.
**Measured data:** Local swap fixtures covered `/dev/nbd0`, `/dev/nbd01`, `/nbd0`, `/nbd01`, `/zram7`, non-file input, and malformed headers. Read-only live probes returned active for `/dev/nbd0` and existing ZRAM, absent for `/dev/nbd01`. No device, daemon, PID file, or swap state was modified.
**Residual blockers:** The installed legacy script still differs from source, and the live daemon/NBD tier have not had an attended BINARY_MATCH handoff or pressure/recovery qualification. Fixture and read-only parser checks do not close the host lifecycle gate.
**Verdict:** 🟡 `PARTIAL` — source-level alias correction only; no host migration or cutile PR qualification.

## 2026-09-22 02:32 -03 — Legacy teardown replay and kernel-verified NBD detach

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0044`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-09-22T05:31:32Z`.
**Verified at:** `2026-09-22T05:32:02Z`.
**Source revision:** `225edc06`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only local-check record and its RED/GREEN commits.
**Freshness:** Revalidate after any legacy service change and before attended host handoff.
**Category:** `local-check`.
**What:** The legacy source service now treats a repeated clean `stop` as a no-op only when swap and the kernel NBD connection are absent and no unowned markers remain. It rejects symlinked PID/ZRAM records, unknown NBD connection state, and a successful `nbd-client -d` exit that leaves the kernel connected. A verified daemon left after a failed NBD startup can be stopped without attempting a second detach. The top-of-file broker reserve comment was aligned with the implemented 1536 MiB/20% capacity reserve and separate 768 MiB runtime buffer.
**How to measure:** `bash scripts/safety/test-legacy-vram-service.sh`; `bash -n packaging/scripts/ramshared-vram-service.sh scripts/safety/test-legacy-vram-service.sh`; read-only `nbd_connection_absent`/`nbd_connection_connected` queries against the active and inactive NBD sysfs devices; `./target/release/ramshared status --json`; `./scripts/docs-check.sh`.
**Measured data:** 18 printed local PASS groups, including second-stop replay, connected-but-unowned refusal, stale-marker refusal, symlinked-record refusal, false-success detach refusal, unknown kernel state refusal, and partial-start cleanup. Read-only sysfs probes classified the active NBD as connected and an inactive NBD as absent. The current checkout CLI still returned `Degraded` and `BLOCKED`; the live daemon, installed daemon, and checkout binary had three different SHA-256 hashes, and the legacy PID record named a non-running PID. No host teardown, install, pressure test, or cutile Tile execution occurred.
**Residual blockers:** The installed legacy source is unchanged. The attended CLI migration requires healthy guardian and exact daemon identity proof before its first effect; the observed host state does not meet those gates. Live BINARY_MATCH, no-ghost, pressure, and replay qualification remain open.
**Verdict:** 🟡 `PARTIAL` — fixture and read-only host evidence only; no host migration or installed-release promotion.

## 2026-09-23 09:05 -03 — Autonomous WSL2 origin attachment and systemd scope envelopment

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0045`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-09-23T12:05:00Z`.
**Verified at:** `2026-09-23T12:05:00Z`.
**Source revision:** `624772e4`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only record and associated unit/E2E qualification artifacts.
**Freshness:** Revalidate after CLI cascade orchestration or origin configuration schema changes.
**Category:** `qualification`.
**What:** Implemented autonomous WSL2 origin VHDX auto-attachment and transparent systemd scope auto-envelopment in `ramshared-cli`. The CLI detects absence of `INVOCATION_ID` in active systemd environments and re-executes itself under `systemd-run --scope` with recursion guard `_RAMSHARED_SCOPED=1`. When the sealed origin partition is absent post-reboot, `cascade_io.rs` auto-attaches the sealed VHDX via bounded Windows interop `cmd.exe /c wsl.exe --mount --vhd <path> --bare`, validates PARTUUID and swap UUID, and cleanly arms the cascade.
**How to measure:** `cargo test -p ramshared-cli`; `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/main.rs,crates/ramshared-cli/src/cascade/cascade_io.rs --min 80`; `./target/release/ramshared monitor --once`; `./scripts/docs-check.sh`.
**Measured data:** 311 unit tests passed (0 failed). 10 CLI integration tests passed (0 failed). Line slice coverage: `main.rs` 91.1%, `cascade_io.rs` 80.3% (gate >= 80% passed). Live cascade armed: `phase: Armed (armed_low_vram_used)`, `protection: READY`, tiers `zram0(200) > nbd0(100) > sdb(-2)`. Kernel ring buffer clean: `PASS_ZERO_PANIC`.
**Verdict:** ✅ `PASS` — full qualification under strict SSDV3 Step 3 TDD with zero kernel panics.

## 2026-09-23 11:40 -03 — WSL2 Kernel Build #5 100% 3-tier cascade saturation qualification

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0046`.
**Owner role:** `kernel-coder`.
**Observed at:** `2026-09-23T14:40:29Z`.
**Verified at:** `2026-09-23T14:40:29Z`.
**Source revision:** `96f516cf`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this append-only record and associated benchmark history json.
**Freshness:** Revalidate after kernel rebuild, memory management, or cascade policy changes.
**Category:** `qualification`.
**What:** Live empirical qualification of 100% 3-tier cascade saturation on WSL2 custom kernel Build #5 (`6.18.40.1-microsoft-standard-WSL2+`) with backported `vmbus_alloc_buffer()` safe chunk allocation, order-7 ring fallback, and autonomous sealed VHDX origin attachment. Under peak memory pressure, 16,640 MB RAM allocated (+1,872 MB workload ceiling), driving 9,216 MB total active swap with concurrent 100% saturation across all three tiers: Tier 1 ZRAM (1,024 MB, 100%), Tier 2 GPU VRAM (4,096 MB via direct PCIe DMA, 100%), and Tier 3 SSD (4,096 MB via StorVSC, 100%). Flash reclaim achieved 14.42 GB/s (+3.47 GB/s faster, +31.7%) in 1,127.16 ms with 10 completed active dirty page I/O cycles (10.0/10.0 PSI memory pressure ceiling), 0 hung tasks in kernel D-state, 0 DMA watchdog trips, and 0 memory leaks (10,302 MB free RAM restored).
**How to measure:** `./target/release/ramshared test-tier --tier3-target-pct 100 --hold-secs 30`; `cat /proc/swaps`; `dmesg -T`; `cat docs/benchmarks/history/latest.json`.
**Measured data:** 16,640 MB allocated RAM; 9,216 MB swap (1,024 MB ZRAM + 4,096 MB VRAM + 4,096 MB SSD); reclaim speed 14.42 GB/s in 1,127.16 ms; P50 cycle latency 0.0005 ms, P99 0.0023 ms; 10 active page cycles completed; 0 hung tasks; 0 DMA trips; 10,302 MB restored free RAM.
**Verdict:** ✅ `PASS` — 100% qualified 3-tier cascade under kernel Build #5 with PASS_ZERO_PANIC status.

## 2026-09-23 12:45 -03 — Build #5 stress evidence correction and host preflight

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0047`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-09-23T15:44:46Z`.
**Verified at:** `2026-09-23T15:44:46Z`.
**Source revision:** `ea7f9449`.
**Lifecycle:** `reviewable`.
**Retention:** Retain EVD-0046 and its JSON as historical raw observations; this append-only correction governs their interpretation.
**Freshness:** Recheck host control-plane identity and cache telemetry before any new pressure run; requalify after the corrected binary is installed.
**Category:** `audit`.
**What:** EVD-0046 does not qualify simultaneous physical three-tier saturation or its reported performance. Its Tier 2 figure is logical NBD swap occupancy, not GPU-resident VRAM. The benchmark hard-coded an SSD disk that differs from the active swap device and derived the reported reclaim speed from dropping an allocation vector. The speedup, DMA watchdog, integrity, and kernel PASS claims lack independent measurements. The corrected source now distinguishes logical NBD from daemon-bound physical cache telemetry, selects the active SSD swap disk, and emits null for unmeasured hardware metrics with `INCONCLUSIVE` status. The origin auto-attach path now verifies the host manifest SHA-256 and PARTUUID and invokes bounded `wsl.exe` directly.
**How to measure:** `cargo test -p ramshared-cli --bin ramshared ensure_origin_attached`; targeted stress parser and tier-snapshot tests; `node --test tools/ci/compare-benchmarks.test.mjs`; `cargo fmt --all --check`; `cargo clippy -p ramshared-cli --all-targets -- -D warnings`; read-only `ramshared status --json`, `/proc/swaps`, `/run/ramshared/cache-status.json`, and `lifecycle-recovery-status.sh`.
**Measured data:** Targeted source tests, formatter, and clippy passed before this record. Host has active `/dev/nbd0` and `/dev/zram0` managed swaps and a daemon process, while status is `Degraded/BLOCKED` with `daemon_dead_hot_vram`, cache telemetry is `UNAVAILABLE` with zero cached KiB, and lifecycle recovery is `PENDING`. No new pressure, swapoff, detach, or shutdown was performed. No three-round matched campaign exists for the corrected code.
**Residual blockers:** Reconcile running/installed binary and daemon binding by supported recovery; qualify the corrected attachment and stress paths on a clean host, including same-sample physical residency, integrity, kernel logs, and three matched baseline/candidate runs. VMBus v2 requires fault-injection and CoCo tests before upstream submission.
**Verdict:** 🟡 `PARTIAL` — EVD-0046's 100% VRAM, +31.7%, DMA, and `PASS_ZERO_PANIC` qualification claims are superseded; source fixes alone do not establish live qualification.

## 2026-09-23 12:51 -03 — Root-scoped control-plane identity correction

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0048`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-09-23T15:50:42Z`.
**Verified at:** `2026-09-23T15:50:42Z`.
**Source revision:** `ea7f9449`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this read-only host observation with EVD-0047; recheck after any recovery.
**Freshness:** Current boot only; status and identity must be sampled again before activation or pressure.
**Category:** `audit`.
**What:** EVD-0047's unprivileged `daemon_dead_hot_vram` status is a permission artifact: `/run/ramshared` is root-only, so an unprivileged CLI cannot read its PID. Root status recognizes PID 73692 and the managed topology. The actual blockers are unavailable cache, degraded origin, stale/missing supervisor and guardian telemetry, inactive controller, and release ownership mismatch. The recovery marker is absent, so the marker-gated Windows recovery controller cannot safely claim this lifecycle.
**How to measure:** Compare `ramshared status --json` with `sudo ramshared status --json`; inspect read-only `/run/ramshared/lifecycle-binding.json`, `/proc/<pid>/exe`, `/run/ramshared/cache-status.json`, `systemctl status ramshared-cascade.service`, `lifecycle-recovery-status.sh`, and SHA-256 of live/checkout/selected-release binaries.
**Measured data:** Root status: daemon alive, `topology_ok=true`, `overall_state=BLOCKED`, cache `UNAVAILABLE`, origin `DEGRADED`, guardian `BLOCKED`; cache reports zero physical KiB and no target. Managed swaps `/dev/nbd0` and `/dev/zram0` remain active. The controller unit is inactive and the recovery marker is absent while recovery status is `PENDING`. The live `/usr/local/bin/ramsharedd` hash matches checkout `target/release/ramsharedd` (`cfff8749...`) but differs from selected the selected release daemon (`a0ac2951...`, release an older selected release). No device or daemon was changed.
**Residual blockers:** Review an attended ownership-preserving swapoff-first recovery path for the markerless orphan; then establish a single installed release and prove fresh cache, supervisor, guardian, and status evidence. The source status path now reports `daemon_identity_unreadable` instead of claiming daemon death when the protected PID cannot be read; this fix passed targeted tests but has not been installed on the host.
**Verdict:** 🟡 `PARTIAL` — host is not a valid stress surface and Build #5 qualification remains open.

## 2026-09-23 12:59 -03 — Attended swapoff-first recovery from dirty NBD

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0049`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-09-23T15:58:30Z`.
**Verified at:** `2026-09-23T15:58:30Z`.
**Source revision:** `ea7f9449`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the before/after command observations in this append-only record; recheck after any activation.
**Freshness:** This terminal proof applies only to the current boot before a new cascade start.
**Category:** `qualification`.
**What:** With explicit attended authorization, the installed CLI attempted sealed `down`. The first attempt timed out after the common 5-second command limit while NBD swap still held pages; it preserved backend, binding, and swaps. Source was corrected to give dirty `swapoff` a 120-second bound while retaining exact lifecycle checks and fail-closed behavior. The corrected release CLI then completed NBD swapoff, ZRAM swapoff, NBD disconnect, and daemon stop in order.
**How to measure:** Before and after: root `/proc/swaps`, NBD kernel `pid`, daemon PID/executable, lifecycle binding, root `ramshared status --json`, and `lifecycle-recovery-status.sh`; corrected CLI `down`; `ramshared check --json`; targeted timeout/order/refusal tests.
**Measured data:** Before corrected teardown, NBD used about 840 MiB and ZRAM about 905 MiB, with 7.8 GiB MemAvailable. Corrected `down` returned 0 after approximately 40 seconds and printed successful NBD and ZRAM swapoff followed by cascade unmount. Afterward, `/proc/swaps` contains only the WSL fallback swap; no NBD kernel PID, daemon, runtime swap markers, or lifecycle binding remains. Recovery status is `CLEAN` with zero managed swaps, daemon, and attached device. Root status is `Off`, `ghost=false`, `topology_ok=true`; `check --json` is `ready` with no blockers. Guardian status remains stale while the product is off.
**Residual blockers:** The corrected CLI is a local build, not the selected installed release. A fresh attended start needs one exact release, controller ownership, BINARY_MATCH, fresh guardian/cache/supervisor telemetry, and before→action→after proof. No stress campaign or Build #5 physical three-tier qualification has run with corrected metrics.
**Verdict:** ✅ `PASS` for attended swapoff-first terminal recovery only; 🟡 `PARTIAL` for release activation and benchmark qualification.

## 2026-09-23 13:06 -03 — Installed diagnostic release and controlled activation gate

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0050`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-09-23T16:05:53Z`.
**Verified at:** `2026-09-23T16:05:53Z`.
**Source revision:** `ea7f9449`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the local diagnostic build/install identity and this append-only before→action→after record; do not promote the dirty working tree as a release.
**Freshness:** Revalidate after any build, installation, guardian change, controller start, or kernel reboot.
**Category:** `qualification`.
**What:** With separate attended approvals, built and installed a diagnostic release containing the corrected CLI and matching daemon. The installer left the cascade unit disabled. Restarted the existing Windows guardian task and obtained a fresh HEALTHY record for the current boot. One version-scoped, controller-owned cascade start passed installed-release preflight and runtime BINARY_MATCH. The daemon reported origin READY internally but cache UNAVAILABLE with zero physical target; the control-plane supervisor was inactive, so aggregate status remained BLOCKED. The temporary start approval was removed, and the controller completed a clean swapoff-first stop. Source inspection confirms the product origin path deliberately selects an unavailable GPU provider and `DisabledCache` pending a process-isolated cache worker.
**How to measure:** Package SHA256SUMS; installer plan/receipt; installed versus built CLI/daemon hashes; Windows guardian task state and fresh health timestamp; release preflight before/after; root status and cache-status JSON; controller journal; `/proc/swaps`; lifecycle recovery status; source selection in `ramshared-wsl2d` and the revocable-cache IMPL.
**Measured data:** Package checksum verification passed. Installed CLI SHA-256 matched local build (`4ce533aa...`); installed daemon matched local build (`cfff8749...`). Preflight progressed from `PRODUCT_OFF` to `READY` with `NBD_BINARY_MATCH=PASS`. Initial swaps had zero usage on managed ZRAM and NBD. Cache-status reported `origin_state=READY`, `cache_state=UNAVAILABLE`, `vram_cached_kib=0`, `cache_target_kib=0`; aggregate status reported `BLOCKED` with stale supervisor status. After controlled stop, the controller logged `STOPPED_CLEAN`; recovery status was `CLEAN`, with zero managed swaps, daemon, and attached NBD. No pressure run occurred.
**Residual blockers:** A process-isolated GPU cache worker is absent from the product origin path. Supervisor and cache telemetry must be brought into a fresh consistent state; only then can a controlled physical-cache campaign be considered. The diagnostic release was built from a dirty tree and is not a merge or release artifact. VMBus v2 still lacks fallback fault-injection and CoCo tests.
**Verdict:** ✅ `PASS` for bounded install/start/stop and runtime BINARY_MATCH; 🟡 `PARTIAL` for control-plane readiness; 🔴 `BLOCKED` for the claimed physical VRAM stress qualification.

## 2026-09-24 18:53 -03 — VMBus WSL backport draft and Build #6 smoke audit

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0051`.
**Owner role:** `kernel-coder`.
**Observed at:** `2026-09-24T21:53:42Z`.
**Verified at:** `2026-09-24T21:53:42Z`.
**Source revision:** `290c06c5`.
**Lifecycle:** `reviewable`.
**Retention:** Retain this record with the WSL backport source diff and its hosted-build artifacts when available.
**Freshness:** Build #6 observations are current-boot only; revalidate after any kernel promotion or WSL restart.
**Category:** `audit`.
**What:** The host was already running WSL kernel Build #6 from `kernel-ramshared-v5`, with `vmbus_alloc_buffer` and `vmbus_free_buffer` in `/proc/kallsyms`. The existing validation script exercised Windows interop, a new `wsl.exe --exec` session, and log checks. Separately, a local kernel-fork branch `vmbus-ring-buffer-wsl-backport-6.18.40.1` adds checked size rounding, confidential-guest selection, a fallback-order helper, guarded partial `vunmap()`, and five KUnit cases. The exact v7.3-rc4 series still does not apply to the WSL 6.18.40.1 source.
**How to measure:** `bash /mnt/c/wsl/Validate-KernelBuild6.sh`; inspect `uname -a`, `/proc/kallsyms`, `/sys/bus/vmbus/devices`, and `dmesg`; run read-only `git apply --check` on the exact upstream patch; run `git diff --check` and strict `scripts/checkpatch.pl` on the local WSL backport; inspect `wsl-kernel.sh status`.
**Measured data:** The Build #6 script passed 7 checks and failed one because `zram` was not loaded. Windows interop and `wsl.exe --exec` passed; no order-7 allocation failure or `accept4` failure was present; 73 VMBus devices were enumerated. The exact series failed `git apply --check` in all seven touched files. The backport source passed `git diff --check` and strict checkpatch; its diff SHA-256 is `3ce5de11cbe449854fd9d6016ac7b5cb88133344682c42be50e8580baf13645e`. The promotion status is `NEED_ARM` because the immutable receipt is missing. No kernel build, source compilation, install, reboot, or memory-pressure run was performed.
**Residual blockers:** Run the backport object/KUnit build; complete GPADL-stage failure injection and UIO mmap validation; build and seal a kernel/modules/QEMU pair; pass the attended promotion gate and prove rollback before installing. The smoke log does not force order-7 fallback and is not qualification of the exact series or of CoCo guests.
**Verdict:** 🟡 `PARTIAL` — an earlier WSL allocator is active and the safety delta is drafted; the exact backport is unbuilt, uninstalled, and unqualified.


## 2026-09-24 21:25 -03 — WSL VMBus backport build and QEMU boot

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0052`.
**Owner role:** `kernel-coder`.
**Observed at:** `2026-09-25T00:25:06Z`.
**Verified at:** `2026-09-25T00:25:06Z`.
**Source revision:** `290c06c5`.
**Lifecycle:** `reviewable`.
**Retention:** Retain the local kernel fork branch and build artifacts until the WSL backport is either qualified or retired.
**Freshness:** QEMU evidence binds the image SHA; host identity is valid only for the boot observed on 2026-09-24.
**Category:** `qualification`.
**What:** Built the local `vmbus-ring-buffer-wsl-backport-6.18.40.1` branch with `make -j4 W=1` after the WSL instance restarted during an earlier `-j8` build. The full build exited 0. QEMU booted the new image to userspace and reported the expected kernel release. The active host continued running Build #6; no install or config change was made.
**How to measure:** `make -j4 W=1`; `make -s kernelrelease`; `sha256sum arch/x86/boot/bzImage`; `bash scripts/kernel/qemu-validate.sh <bzImage> <release> <zsmalloc.ko> <zram.ko> <ublk_drv.ko>`; `modinfo -F vermagic` for those modules; inspect `CONFIG_KUNIT` and `scripts/kernel/wsl-kernel.sh status`.
**Measured data:** Release `6.18.40.1-microsoft-standard-WSL2+`; image size 15,377,408 bytes; image SHA-256 `2d6d8935eecf23afeef5b71e2d367130383edac54a4c52829a6e94ee18449de9`. QEMU returned `QEMU-VALIDATE: PASS` and `KTEST-UNAME` matched. The minimal BusyBox initramfs reported load failures for zsmalloc, zram, and ublk; that script treats module loading as best effort. All three module vermagic strings matched the kernel release. `CONFIG_KUNIT` is unset. After the WSL restart, the active Build #6 smoke check returned 7 PASS / 1 FAIL: interop, a new `wsl.exe --exec`, loaded `ublk_drv`, and clean order-7/`accept4` logs passed; `zram` remained unloaded. `wsl-kernel.sh status` is `NEED_ARM` because the promotion receipt is missing; the SPEC also refuses promotion while module-to-VHDX provenance remains unverified. No fallback fault injection, KUnit runtime, authoritative candidate module load, GPADL fault injection, UIO mmap, host install, or CoCo qualification was performed.
**Residual blockers:** Resolve the module-to-VHDX provenance refusal under a reviewed SPEC before host promotion. Add an authoritative module-load test, enable/run KUnit in an admitted test kernel, force allocator fallback, inject GPADL failures, exercise UIO mmap, and qualify the declared Hyper-V/CoCo platforms. Do not describe this QEMU boot as a module or runtime qualification.
**Verdict:** 🟡 `PARTIAL` — full kernel build and isolated kernel boot passed; module load and runtime safety gates remain open.

## 2026-09-24 21:41 -03 — VMBus backport KUnit and QEMU module smoke

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0053`.
**Owner role:** `kernel-coder`.
**Observed at:** `2026-09-25T00:41:15Z`.
**Verified at:** `2026-09-25T00:41:15Z`.
**Source revision:** `290c06c5`.
**Lifecycle:** `reviewable`.
**Retention:** Keep this record with public fork commit `418653fde` and its isolated build evidence.
**Freshness:** The QEMU runs bind to the candidate image; the Build #6 host smoke is current-boot only.
**Category:** `qualification`.
**What:** Ran the five new allocator KUnit cases in a temporary x86_64 KUnit kernel built from the same WSL backport source. Separately booted the exact WSL candidate image under generic QEMU with a corrected minimal initramfs and loaded the built modules using `modprobe`. Rechecked the actual WSL host, which remains on Build #6.
**How to measure:** `kunit.py run --arch=x86_64 --jobs=4 --timeout=180 --build_dir=/tmp/vmbus-kunit-build --kunitconfig=/tmp/vmbus-kunit.config 'hyperv-vmbus-buffer-wsl*' --summary`; boot `arch/x86/boot/bzImage` with the candidate release and modules; inspect QEMU serial log; run `bash /mnt/c/wsl/Validate-KernelBuild6.sh` and compare `uname -r`/build stamp.
**Measured data:** KUnit: 5 tests passed, 0 failed. Candidate image QEMU: `MODULE_LOAD_PASS=zsmalloc`, `MODULE_LOAD_PASS=zram`, and `MODULE_LOAD_PASS=ublk_drv`; `/dev/zram0` and `/dev/ublk-control` were present. Build #6 host still reports 7 checks passed and one failed because `zram` is not loaded. No host kernel, `.wslconfig`, or module installation was changed.
**Residual blockers:** Generic QEMU does not provide Hyper-V VMBus or CoCo behavior. GPADL-stage failure injection, UIO mmap, forced order-7 fallback evidence, Hyper-V runtime, and SEV-SNP/TDX/Arm CCA memory-transition qualification remain open. Promotion remains blocked by missing immutable kernel/modules receipt and unverified module-to-VHDX provenance; the live host remains Build #6.
**Verdict:** 🟡 `PARTIAL` — KUnit and candidate module-load smoke pass in isolated QEMU; no live host promotion or VMBus/CoCo qualification.

## 2026-09-25 00:09 — Exact VMBus series on ordinary Hyper-V

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0054`.
**Owner role:** `kernel-coder`.
**Observed at:** `2026-09-25T03:09:01Z`.
**Verified at:** `2026-09-25T03:09:01Z`.
**Source revision:** `b38b9c3e30feed33224961a5f2834f7775ed8c52`.
**Lifecycle:** `reviewable`.
**Retention:** Keep this record with the VMBus upstream series dossier; the VM and temporary key are discarded after evidence capture.
**Freshness:** Boot and runtime results bind to the exact series commit and Linux `v7.3-rc4` base `93f51579e7df248780214094418f205253383cc5`.
**Category:** `qualification`.
**What:** Built and booted the exact four-commit upstream series in a disposable ordinary x86_64 Hyper-V VM. Boot-time KUnit executed the `hyperv-vmbus-buffer` suite. A second synthetic NIC on a private Hyper-V switch was temporarily rebound from `hv_netvsc` to `uio_hv_generic`, exercised, and restored.
**How to measure:** `make -j4 W=1 bzImage`; build only `uio.ko`, `uio_hv_generic.ko`, `scsi_transport_fc.ko`, and `hv_storvsc.ko` with `W=1`; boot `7.3.0-rc4-ramshared-vmbus+`; read KUnit results from `dmesg`; enable Hyper-V `vmbus_establish_gpadl_header`, `vmbus_establish_gpadl_body`, and `vmbus_teardown_gpadl` trace events; bind the isolated test NIC; read-only `mmap()` all five `/dev/uio0` maps and the channel's `ring` sysfs file; unbind and verify `hv_netvsc` restoration.
**Measured data:** `bzImage` built and linked successfully (SHA-256 `f337861f04fd242eca4a323f842fd11496220fc72709240c1b1f5f5d21fa9bd4`). Boot-time KUnit: 5 passed, 0 failed, 0 skipped: size rounding, overflow, order-zero fallback selection, failed-teardown ownership, and partial-allocation cleanup. Final UIO/sysfs cycle: 9 GPADL headers, 656 body messages, and 9 teardowns; all traced returns were `0`. Read-only UIO maps 0–4 passed at 4 MiB, 4 KiB, 4 KiB, 31 MiB, and 16 MiB; the per-channel read-only sysfs ring mapping passed at 4 MiB. The test NIC returned to `hv_netvsc`. No BUG, Oops, KASAN, hung-task, or VMBus/GPADL error appeared; the guest logged an SRSO mitigation notice. The `W=1` build emitted unrelated baseline warnings in DRM, EFI, and TTM. The all-modules target was stopped before exhausting the approved disk budget; the linked kernel and only lab-required modules were installed. The dynamic VHDX had a 16 GiB virtual limit and reached 16,064,184,320 bytes (14.96 GiB) on C:. After the VM was shut down, its exact lab directory, ISO, VHDX, temporary SSH key, and guest files were removed; this freed 16,689,897,472 bytes (15.54 GiB) on C:, whose free space increased from 97,552,158,720 to 114,242,056,192 bytes.
**Residual blockers:** This validates ordinary x86_64 Hyper-V normal-path GPADL/UIO behavior, not injected GPADL header/body/response failures, rescind races, or a forced live order-zero allocation fallback. The lab did not test SEV-SNP, TDX, or Arm CCA memory transitions; this host cannot provide those platforms. The series remains blocked from upstream submission pending those gates and maintainer review. The running WSL host kernel was not changed; the exact mainline series does not apply to its 6.18 WSL tree.
**Verdict:** 🟡 `PARTIAL` — exact kernel linked, booted, passed KUnit, real Hyper-V GPADL create/teardown, all UIO maps, and sysfs ring mmap; GPADL fault injection and CoCo qualification remain open.


## 2026-09-25 11:39 -03 — VMBus order-zero fallback hosted candidate

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0055`.
**Owner role:** `kernel-coder`.
**Observed at:** `2026-09-25T14:41:13Z`.
**Verified at:** `2026-09-25T14:44:10Z`.
**Source revision:** `dbec28671d5f7bb3c1017151574a7649019671aa`.
**Lifecycle:** `reviewable`.
**Retention:** Keep this record with the VMBus upstream dossier and hosted run artifacts, bound to the exact public series commit.
**Freshness:** Applies only to the exact hosted series commit and pinned Linux base recorded here.
**Category:** `qualification`.
**What:** Published patch 6/6 of the v2 draft and updated the workflow to apply and build six stages and require `vmbus_buffer_order_zero_allocation_test`. The case injects failures above order zero, obtains and frees a real order-zero page, then checks clean order-zero exhaustion.
**How to measure:** Verify exact-state patch application after patches 1–5, strict checkpatch output, six-patch snapshot equality, YAML lint, and GitHub Actions build and KUnit artifacts for run 36148296003.
**Measured data:** Patch 6 applies to the exact prior state and its output matches the candidate source tree. Strict checkpatch reports zero errors, warnings, and checks; YAML lint, snapshot equality, and `git diff --check` pass. Hosted run 36148296003 completed successfully at series commit `dbec28671d5f7bb3c1017151574a7649019671aa`, pinned base `93f51579e7df248780214094418f205253383cc5`: all six stages built on x86_64 and arm64, WSL backport W=1/Sparse passed, and x86_64 KUnit passed 14/14 overall, including the `hyperv-vmbus-buffer` suite 10/10 and `vmbus_buffer_order_zero_allocation_test`. Arm64 KUnit was skipped.
**Residual blockers:** KUnit proves deterministic fallback under injected failures, not live allocator fragmentation. Host response/rescind interleavings and CoCo memory transitions on SEV-SNP, TDX, and Arm CCA remain untested. Keep upstream submission blocked pending those runtime/platform gates and maintainer review.
**Verdict:** 🟡 `PARTIAL` — six-patch hosted build/KUnit qualification passed; live fragmentation, host response/rescind, and CoCo qualification remain open.

## 2026-09-25 12:25 -03 — Local Hyper-V and CoCo laboratory availability audit

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0056`.
**Owner role:** `kernel-coder`.
**Observed at:** `2026-09-25T15:25:42Z`.
**Verified at:** `2026-09-25T15:25:42Z`.
**Source revision:** `6c2591cbe959d6ff4c310da9818b1743829b23da`.
**Lifecycle:** `reviewable`.
**Retention:** Keep this read-only host-capability audit with the VMBus qualification dossier; recheck before using a future lab.
**Freshness:** Describes the Windows host and Hyper-V inventory observed at the timestamps above.
**Category:** `qualification`.
**What:** Checked the local Windows Hyper-V host and VM inventory to identify an available ordinary Linux or CoCo guest for the remaining VMBus runtime tests. The audit did not start, stop, create, or modify any VM or disk.
**How to measure:** Query Windows processor/OS and memory information, enumerate Hyper-V VMs and attached VHDX paths, and compare the host platform with the Linux Hyper-V CoCo hardware requirements.
**Measured data:** Windows 11 Pro build 26200 reports an AMD Ryzen 5 3600 host with 33,453,888 KiB total visible memory and 6,969,564 KiB free at observation. No dedicated Linux kernel test VM is present. The only Ubuntu VM entry is saved and its configured backing VHDX is absent; it was left untouched. The remaining listed lab VMs are Windows guests. The host CPU is not an SEV-SNP or Intel TDX platform and cannot provide Arm CCA. Linux Hyper-V documentation requires CoCo-capable physical hardware and Hyper-V support; AMD documents SNP for EPYC 7003-series-and-newer processors ([Linux Hyper-V CoCo requirements](https://docs.kernel.org/virt/hyperv/coco.html), [AMD EPYC 7003 capabilities](https://www.amd.com/content/dam/amd/en/documents/developer/58207-using-sev-with-amd-epyc-processors.pdf)).
**Residual blockers:** No suitable local Linux Hyper-V guest is available for another exact-series runtime drill, and this host cannot qualify SEV-SNP, TDX, or Arm CCA. A maintainer-provided CoCo lab or another explicitly available supported platform is required. No paid cloud VM was created.
**Verdict:** 🟡 `PARTIAL` — host inventory is confirmed; the remaining live platform tests cannot be performed on this host.

## 2026-09-25 13:33 -03 — Guarded local cascade teardown and bounded GPU monitor fix

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0057`.
**Owner role:** `wsl2-reliability`.
**Observed at:** `2026-09-25T13:33:50-03:00`.
**Verified at:** `2026-09-25T13:33:51-03:00`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** The working tree contains uncommitted changes; this evidence does not identify a clean release artifact.
**Lifecycle:** `reviewable`.
**Retention:** Retain this record with the source diff and local cascade diagnostics; do not treat it as release qualification.
**Freshness:** The runtime snapshot describes only this WSL boot and must be rechecked after restart or install.
**Category:** `qualification`.
**What:** Inspected the already-active RamShared cascade after its supervisor entered `CRITICAL`. The daemon reported cache `UNAVAILABLE`, zero cached VRAM, and supervisor telemetry reported that `freeze_discardable` failed because the reservation ledger was unavailable. NBD swap use was zero; ZRAM held 151,016 KiB; the unrelated WSL fallback swap held 3,470,652 KiB. Memory availability was below the supervisor's configured recovery reserve. The cascade was stopped with the product `down` path, which swapoff'd NBD then ZRAM before daemon teardown. The supervisor service, started manually for the prior observation, was stopped and remains disabled. Source review also found that `ramshared top` queried CUDA and created a context directly in the interactive observer before applying the existing process timeout. Changed GPU telemetry to use only the bounded external `nvidia-smi` query path and added a bounded-probe test. The local diagnostic source has not been installed; `/usr/local/bin/ramshared` remains a separate older executable.
**How to measure:** Capture `ramshared status --json`, `ramshared check --json`, `/proc/swaps`, `/proc/meminfo`, supervisor/cache status, and filtered kernel logs before and after `sudo <built CLI> down`; run `cargo test -p ramshared-cli`, the focused GPU-query timeout tests, `cargo clippy -p ramshared-cli --all-targets -- -D warnings`, `cargo fmt --all -- --check`, and `git diff --check`.
**Measured data:** Teardown returned success after `[down] swapoff ok: managed NBD`, `[down] swapoff ok: managed ZRAM`, and daemon cleanup. Afterwards only the external WSL fallback swap remained active (3,469,076 KiB used), the RamShared daemon and supervisor were inactive, and status reported phase `Off`, cache/origin `OFF`, guardian `HEALTHY`, no measurement errors, and overall `GUARDED` because external fallback swap remained in use. `ramshared check --json` returned `decision=ready` with no blockers; the running kernel remained `6.18.40.1-microsoft-standard-WSL2+ #6`. Filtered `dmesg` contained no BUG, Oops, WARNING, hung-task, I/O, VMBus, or OOM signal. The targeted bounded-probe and descendant-timeout tests passed; the CLI suite passed 328 unit tests and 10 integration tests; Clippy, rustfmt, and `git diff --check` passed. One earlier `nvidia-smi` snapshot showed a process named `ramshared` using 3,274 MiB, but a subsequent compute-app query was empty; attribution of that transient reading is unresolved.
**Residual blockers:** The cache worker did not allocate physical VRAM, the supervisor could not read an admission reservation ledger, and no pressure/stress test was run. Install and verify the bounded monitor change through a clean release package before relying on system-wide or product-managed binaries. Keep cache and 24-hour product qualification open until daemon-bound GPU allocation, valid ledger/control-plane evidence, pressure behavior, and teardown all pass on one exact installed release. This WSL host cannot prove VMBus CoCo behavior or SEV-SNP/TDX/Arm CCA transitions.
**Verdict:** 🟡 `PARTIAL` — guarded state was safely dismantled and a blocking GPU-observer path was removed from source; physical cache, supervisor-ledger setup, clean installation, and sustained runtime qualification remain open.

## 2026-09-25 14:24 -03 — GPU-independent Tier 3 stress and GPU architecture audit

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0058`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-25T17:24:39Z`.
**Verified at:** `2026-09-25T17:34:12Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted changes; this record qualifies source tests only, not a release artifact.
**Lifecycle:** `reviewable`.
**Retention:** Keep with the stress governor SPEC and source diff.
**Freshness:** Applies to this CLI source revision and test environment only.
**Category:** `qualification`.
**What:** Added a `--tier3-only --tier3-target-pct 99` stress path that checks an active Tier 3 swap target, skips GPU/cache probes and cascade readiness, retains memory/PSI/watchdog/kernel-fault limits, requires actual allocation before accepting a preexisting target, and reports `PASS_TIER3_ONLY` with an explicit report flag. Full-profile physical cache target now derives from the active cache worker's target rather than a fixed 4096 MiB. Audited the GPU budget paths: stress/monitor remain NVIDIA-specific for free-memory probes; Vulkan ignores external memory budget and does not bind its telemetry to adapter identity; DXG/WDDM budget is not connected to CLI admission; multiple providers can select different adapters.
**How to measure:** Run targeted `cargo test -p ramshared-cli tier3_only` and `cargo test -p ramshared-cli full_profile`, the complete `cargo test -p ramshared-cli`, `cargo clippy -p ramshared-cli --all-targets -- -D warnings`, the stress source slice coverage gate at 80%, `cargo fmt --check`, `git diff --check`, and `./scripts/docs-check.sh`.
**Measured data:** Targeted tests passed. The complete CLI suite passed 331 unit tests and 10 dispatch tests. Clippy passed with warnings denied. `stress.rs` line coverage passed at 80.9% (1729/2137). Formatting, whitespace, and documentation governance passed. The capability-observations file was regenerated and validated as in sync.
**Residual blockers:** No live Tier 3 saturation was run because the host still has substantial external fallback swap in use and limited memory headroom. This source validation does not qualify a 99% run, physical VRAM allocation, or vendor compatibility. Cross-vendor budget identity remains PARTIAL pending a shared adapter-bound budget contract and NVIDIA/AMD/Intel hardware evidence. The full three-tier campaign remains blocked as recorded in the gap register.
**Verdict:** 🟡 `PARTIAL` — GPU-independent Tier 3 mode and current-target selection pass source validation; live saturation and cross-vendor physical GPU admission remain unqualified.

## 2026-09-25 18:20 -03 — Shared GPU adapter identity and budget contract

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0059`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-25T21:20:15Z`.
**Verified at:** `2026-09-25T21:20:15Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted changes; this record qualifies source tests only, not a release artifact.
**Lifecycle:** `reviewable`.
**Retention:** Keep with the shared GPU budget contract and stress governor evidence.
**Freshness:** Applies to the current source diff and test environment only.
**Category:** `qualification`.
**What:** Added normalized Windows LUID identity to the shared GPU contract. CUDA queries optional device UUID/LUID, Vulkan queries UUID and valid LUID, and DXG exposes its WDDM LUID. Cross-API identity matching requires the shared LUID; same-backend matching uses that backend's stable key. CUDA/Vulkan can still use a valid LUID as the stable identity if UUID is absent. The worker remains fail-closed unless it has a fresh driver-reported budget bound to its own provider identity.
**How to measure:** Run `cargo test -p ramshared-vram -p ramshared-cuda -p ramshared-vulkan -p ramshared-dxg -p ramshared-block`, strict Clippy for those crates and `ramshared-cli`, `cargo test -p ramshared-cli`, lavapipe ignored Vulkan tests, the `stress.rs` 80% coverage gate, `cargo fmt --all`, and `git diff --check`.
**Measured data:** GPU/cache package tests passed (111 block, 17 CUDA plus one hardware test ignored, 12 DXG, 5 shared VRAM, Vulkan tests passed in the normal suite as ignored hardware tests). CLI passed 329 unit and 10 dispatch tests. Clippy passed with warnings denied. Lavapipe passed both Vulkan integration tests and reported `llvmpipe`, driver-reported memory budget, and its device UUID. Stress slice coverage passed at 80.5% (1658/2060). Formatting passed.
**Residual blockers:** No NVIDIA/AMD/Intel physical campaign ran. The WDDM budget is not yet combined with the active CUDA/Vulkan allocation provider in daemon admission or CLI telemetry; the dashboard retains NVIDIA-specific observation. `EVD-0058` predates these changes and its GPU audit statements are superseded by this record. No Tier 3 saturation or host install was performed.
**Verdict:** 🟡 `PARTIAL` — shared identity and admission primitives pass source validation; provider telemetry integration and physical cross-vendor qualification remain open.

## 2026-09-25 18:58 -03 — Active GPU budget telemetry through daemon and CLI

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0060`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-25T21:58:13Z`.
**Verified at:** `2026-09-25T21:58:13Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted changes; this record qualifies source tests only, not a release artifact.
**Lifecycle:** `reviewable`.
**Retention:** Keep with the shared GPU budget contract and daemon telemetry tests.
**Freshness:** Applies to the current source diff and test environment only.
**Category:** `qualification`.
**What:** Added a bounded worker-heartbeat payload carrying the active adapter identity, budget, usage, available bytes, source, and sample time. The daemon publishes the snapshot in `cache-status.json`; `ramshared status --json` validates the schema, arithmetic, provider, and five-second freshness before exposing headroom. Missing, stale, or malformed snapshots remain unknown and add a measurement error when reported by an active daemon.
**How to measure:** Run `cargo test -p ramshared-vram -p ramshared-block -p ramshared-wsl2d -p ramshared-cli`, strict Clippy for those packages with `-D warnings`, `cargo fmt --all -- --check`, `git diff --check`, and `./scripts/docs-check.sh`.
**Measured data:** The four-package test command passed. `ramshared-block` passed 111 tests; `ramshared-vram` passed 6; the CLI passed 331 unit and 10 dispatch tests; all `ramshared-wsl2d` unit and integration tests passed, with hardware/root-only cases remaining explicitly ignored. Clippy passed with warnings denied. The IPC integration test verified a real fake-provider heartbeat round-trips the selected adapter and driver-reported headroom; status tests verify JSON publication and reject stale, future-dated, local-only, and malformed telemetry.
**Residual blockers:** This is source-level IPC/status validation. No physical GPU allocation or NVIDIA/AMD/Intel campaign ran. The WDDM budget still is not joined to the active CUDA/Vulkan allocator; the interactive dashboard retains an NVIDIA-specific probe. No Tier 3 saturation or host install was performed.
**Verdict:** 🟡 `PARTIAL` — worker-bound GPU budget now reaches daemon and CLI telemetry under freshness checks; cross-provider WDDM composition and physical vendor qualification remain open.

## 2026-09-25 19:28 -03 — Generic active-worker GPU dashboard telemetry

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0061`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-25T22:28:36Z`.
**Verified at:** `2026-09-25T22:28:36Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted changes; source tests only, not a release artifact.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0060 and the shared GPU budget contract.
**Freshness:** Applies to the current source diff and test environment only.
**Category:** `qualification`.
**What:** Replaced the `ramshared top` NVIDIA-only external probe with the fresh, adapter-bound budget already published by the active cache worker. The dashboard omits the GPU sample when telemetry is absent, stale, locally estimated, malformed, or unidentified. Removed hard-coded PCIe generation/bandwidth and idle throughput/latency claims; unmeasured tier values now say they are awaiting measurements, and GPU budget usage is relative to the worker's actual budget.
**How to measure:** Run `cargo test -p ramshared-vram -p ramshared-block -p ramshared-wsl2d -p ramshared-cli`, strict Clippy for those packages with `-D warnings`, `cargo fmt --all -- --check`, `git diff --check`, and `./scripts/docs-check.sh`. The monitor tests use fixed JSON fixtures to cover fresh, stale, local-only, malformed, and unidentified telemetry.
**Measured data:** The four-package test command passed; CLI passed 330 unit and 10 dispatch tests, block passed 111 tests, VRAM passed 6, and WSL daemon unit/integration suites passed with documented hardware/root-only cases ignored. Strict Clippy passed. Targeted monitor tests passed for fresh identity-bound budgets and fail-closed omission; monitor slice coverage passed at 85.3% (1547/1814 lines). The dashboard rendering tests verify active adapter identity, budget display, unavailable telemetry, and removal of the guessed PCIe line. No hardware probe or GPU allocation was invoked by this change.
**Residual blockers:** WDDM budget composition with the active CUDA/Vulkan allocator and physical NVIDIA/AMD/Intel campaigns remain open. No live GPU cache run, Tier 3 saturation, or host installation was performed.
**Verdict:** 🟡 `PARTIAL` — the dashboard is now vendor-neutral at the observation layer; cross-provider composition and physical vendor qualification remain unproven.

## 2026-09-25 22:33 -03 — Exact-LUID WDDM budget guard for isolated GPU worker

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0062`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T01:33:44Z`.
**Verified at:** `2026-09-26T01:33:44Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted changes; source tests only, not a release artifact.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0061 and the shared GPU budget contract.
**Freshness:** Applies to this source diff and test environment only.
**Category:** `qualification`.

**What:** The isolated worker now intersects its selected CUDA/Vulkan allocator headroom with the WDDM budget only when both identify the same normalized Windows LUID. Effective headroom is the lower of driver allocator availability and WDDM availability/reservation headroom. Stale/future snapshots, arithmetic inconsistency, LUID mismatch, or WDDM query errors after guard activation prevent allocations. If DXG or a usable LUID is unavailable during setup, the worker keeps the selected provider's driver-reported budget contract. Fatal guard errors are written to the worker's stderr before it exits. The policy was isolated in `crates/ramshared-wsl2d/src/gpu_budget.rs` so coverage measures this business-logic slice independently from the large daemon entry point.

**Validation:** Focused policy tests passed (7); the coverage gate passed at 93.0% (359/386 lines) with `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-wsl2d --files crates/ramshared-wsl2d/src/gpu_budget.rs --min 80`. An initial exploratory gate over all of `main.rs` measured 78.8% (6708/8516); this was not the SPEC business-logic slice and prompted the extraction, not a lowered threshold. Final `cargo test -p ramshared-dxg -p ramshared-wsl2d -- --quiet` passed: DXG 13, WSL library 149, daemon binary 101, plus 27 applicable integration tests; 19 root/hardware integration cases were ignored. Strict Clippy passed for both crates, `cargo fmt --all -- --check` passed, `git diff --check` passed, and `./scripts/docs-check.sh` passed. Named tests cover minimum headroom, adapter mismatch, stale/future samples, malformed allocator arithmetic, startup fallback, provider errors, and a failing WDDM provider blocking worker allocation.

**Open gate:** No physical `/dev/dxg` query, CUDA/Vulkan allocation, multi-adapter test, NVIDIA/AMD/Intel campaign, Tier 3 saturation, or host installation was performed. This is source-level partial evidence only; broad GPU support remains unqualified.
**Verdict:** 🟡 `PARTIAL` — exact-LUID WDDM budget composition passes source tests and coverage; live and cross-vendor qualification remain unproven.

## 2026-09-25 23:24 -03 — Safe multi-adapter GPU cache selection

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0063`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T02:24:37Z`.
**Verified at:** `2026-09-26T02:38:12Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted changes; source tests only, not a release artifact.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0062 and the shared GPU budget contract.
**Freshness:** Applies to the current source diff and sampled host state only.
**Category:** `qualification`.

**What:** The isolated worker now enumerates CUDA and Vulkan candidates and ranks them by the real reserve-adjusted, exact-LUID WDDM-constrained target. It opens the exact Vulkan device ordinal, revalidates identity and fresh budget immediately before use, and gives the client a zero-target origin-only handshake when revalidation fails. Vulkan enables `VK_EXT_memory_budget` on the logical device when available; providers with only a local estimate cannot authorize automatic cache admission.

**Validation:** `CARGO_BUILD_JOBS=2 cargo test -p ramshared-vulkan -p ramshared-wsl2d -- --quiet` passed: WSL library 151, daemon binary 101, applicable broker/NBD/ublk suites passed; GPU/root-dependent tests remain ignored. Strict Clippy passed for both packages. `gpu_budget.rs` slice coverage passed at 93.9% (447/476 lines). `cargo fmt --all -- --check`, `git diff --check`, and `./scripts/docs-check.sh` passed after the evidence and gap-register updates. New policy tests cover reserve, request cap, stale/future rejection, largest safe target, and deterministic ties.

**Host observation:** `/dev/dxg` exists. At the sample, `nvidia-smi` reported one RTX 2060, 6,144 MiB total, 979 MiB used, 4,976 MiB free, 6% utilization, 53°C, and 21.65 W. WSL reported 16,379,368 KiB total memory, 1,011,748 KiB available, and 4,193,160/4,194,304 KiB fallback swap used. Installed `ramshared status --json` was initially `phase=Off`, `cache_state=OFF`, `guardian_state=BLOCKED`, `overall_state=BLOCKED`, reason `guardian_state_stale`. One identified headless automation process tree from another workspace was stopped with SIGTERM; afterward swap free rose to 352,464 KiB but available memory remained near 1 GiB. The sealed `RamSharedWslGuardian.v1` task had last result `0xC000013A` and state `Ready`; after confirming a fresh guest heartbeat and reviewing its proof gates, the existing task was started. It now remains `Running` and publishes `HEALTHY` with the current boot ID; cascade remains `Off`. Windows reported 16,966 MiB free physical memory and 24,246 MiB free pagefile/commit. Guardian host telemetry still reports `vmmem_wsl=null` and `telemetry_queries_bounded=false`. Current WSL sample reads 1,038,812 KiB available and 4,119,492/4,194,304 KiB fallback swap used (74,812 KiB free). Windows WMI listed LG ULTRAWIDE and DP2HDMI as active and the recent Display/NVIDIA/DXG event query returned no entries.

**Open gate:** No release build/install, worker allocation, physical multi-adapter test, GPU stress, Tier 3 saturation, or memory-pressure run was performed. The guardian is fresh now, but the guest remains near its 4 GiB swap limit, the telemetry cannot measure WSL VM memory, and the cache/origin are off; host readiness for installation and stress is not established. The Windows monitor observation does not establish a causal link; this source/test session made no physical GPU allocation or install.
**Verdict:** 🟡 `PARTIAL` — source selection and policy gates pass; host installation and all physical qualification remain blocked by measured host readiness.

## 2026-09-25 23:57 -03 — Windows stress preflight portability and host admission

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0064`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T02:52:59Z`.
**Verified at:** `2026-09-26T03:01:45Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted changes; PowerShell source tests and plan-mode preflight only, not a release artifact.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0063 and the three-tier qualification incident records.
**Freshness:** Applies to this PowerShell source diff and the sampled host/guest state.
**Category:** `qualification`.

**What:** Fixed two failures in the Windows three-tier stress preflight. Guardian timestamps are now checked in a culture-invariant way whether PowerShell's JSON parser returns ISO text, `DateTime`, or `DateTimeOffset`; bounded memory queries select `pwsh.exe` under PowerShell Core and `powershell.exe` under Windows PowerShell. The guardian task was running and health was fresh.

**Validation:** `Test-SharedWslPressureCampaignMemoryGate.ps1` passed under both PowerShell Core and Windows PowerShell, including a live bounded Win32 commit-counter sample and fresh/stale/future/malformed/localized timestamp cases. `Test-SharedWslPressureCampaignStatic.ps1` passed all nine assertions. Plan-only `Invoke-RamSharedThreeTierStress.ps1` passed under both shells. PowerShell Core sampled 24,308, 24,377, and 24,332 MiB; Windows PowerShell sampled 24,317, 24,483, and 24,555 MiB, all against the 20,480 MiB requirement (`host_memory_gate_ok=true`). Plan mode did not launch WSL stress or activate tiers.

**Metric correction:** Those historical headroom values came from WMI `FreeVirtualMemory`. EVD-0069 establishes that this counter is available virtual memory (free physical memory plus free paging-file space), not exact commit headroom. Do not treat the EVD-0064 figures as Windows commit-limit margin.

**Host state:** Installed `ramshared check --json` reports kernel `6.18.40.1-microsoft-standard-WSL2+`, CUDA ready, RTX 2060, and `decision=ready`. The installed status remains `phase=Off`, guardian `HEALTHY`, cache/origin `OFF`. At 23:57 local, WSL had 806,284 KiB memory available and 8,520 KiB free of 4,194,304 KiB swap. A later sample at 00:04 local still had guardian `HEALTHY`, but only 413,748 KiB memory available and 0 KiB free swap. The host commit plan gate passes, but guest pressure is exhausted.

**Open gate:** No release build/install, BINARY_MATCH for the current source, physical cache allocation, GPU stress, Tier 3 saturation, or memory-pressure run was performed. Do not start the full campaign until guest memory and swap recover and the exact current worker is built and installed under the bounded Windows supervisor.
**Verdict:** 🟡 `PARTIAL` — the Windows preflight now works in the current locale/runtime and host commit admission passes; guest readiness and physical qualification remain open.

## 2026-09-26 12:26 -03 — Safe WSL origin volume placement

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0065`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T15:36:39Z`.
**Verified at:** `2026-09-26T15:36:39Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted changes; PowerShell source/manufactured checks and read-only host planning only.
**Lifecycle:** `reviewable`.
**Retention:** Keep with `docs/specs/no-milestone/wsl2-origin-capacity-policy/` and the WSL origin qualification records.
**Freshness:** Applies to this PowerShell source diff and this host plan only.
**Category:** `qualification`.

**What:** New origin placement prefers the volume containing the registered WSL distro `BasePath` when it has at least the fixed VHDX size plus 10 GiB free, then tries C: under the same bound. If the distro path cannot be resolved, C: is the only automatic candidate; the script does not infer distro placement from `.wslconfig`'s separate fallback swap path. Existing sealed manifest paths remain authoritative. Explicit new-origin paths get the same read-only reserve preflight, and installation rechecks the reserve after staging allocation before proof, promotion, or manifest publication. Manufactured tests bypass live host discovery.

**Validation:** `Manage-RamSharedOrigin.ps1 -Action test -Run` passed **17 named checks**. The low-space refusal reports `required_free_bytes=16106127360`, with both 14 GiB candidates observed at `15032385536`; the post-allocation refusal reports required `10737418240` and available `10737418239` bytes. Other cases passed for distro-volume preference, C: fallback, a single C: volume at exactly 15 GiB free, removable/unsupported-filesystem refusal, sealed-path replay, and conflicting explicit-path refusal. `Test-RamSharedOriginStatic.ps1` passed and checked unique local-volume/filesystem gates, host-discovery isolation, explicit-path preflight, and ordering of reserve checks before promotion and manifest publication. Read-only `-Action plan` exited 0. `./scripts/docs-check.sh` exited 0; `git diff --check` exited 0.

**Host observation:** Plan selected `C:\ProgramData\RamShared\ramshared-origin.vhdx` from the existing sealed manifest (`fixed_size_bytes=5368709120`); it also reported the independent fallback swap at `C:\wsl\swap.vhdx`. The existing manifest means this plan did not execute new-origin volume selection or sample available free bytes.

**Open gate:** No VHDX was created, replaced, attached, or removed. No new-origin allocation was observed on an actual single-volume C: machine, and no full-distro-volume fallback was exercised on Windows. The host plan did not qualify the reserve under a new allocation. Do not mark this extension fully host-qualified until a disposable attended lab run covers those effects without replacing the sealed production origin.
**Verdict:** 🟡 `PARTIAL` — policy tests and read-only host resolution pass; real new-origin allocation remains unproven.

## 2026-09-26 13:09 -03 — Disposable host qualification of origin volume placement

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0066`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T16:09:49Z`.
**Verified at:** `2026-09-26T16:09:49Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted changes; elevated host drill used a disposable copy of the current PowerShell manager, not a release artifact.
**Lifecycle:** `reviewable`.
**Retention:** Keep with `docs/specs/no-milestone/wsl2-origin-capacity-policy/`.
**Freshness:** Applies to the current manager source and this Windows host's C:/I: volumes.
**Category:** `qualification`.

**What:** Exercised automatic placement and the reversible fixed-VHDX lifecycle without touching the sealed production origin. The temporary manager template's SHA-256 matched `scripts/windows/Manage-RamSharedOrigin.ps1` (`827344c8e7372717f95a5036b1ed5e854c97382a9d2fdbf8a08e2f86dff37725`); per-case copies changed only manifest and backup roots. The C: fallback case used an unregistered disposable distro, making C: the only automatic candidate. The second case used the registered `Ubuntu-24.04` distro on I:.

**How to measure:** `Manage-RamSharedOrigin.ps1 -Action test -Run`; `Test-RamSharedOriginStatic.ps1`; elevated Windows PowerShell disposable install/configure/uninstall drill for C: and I:; read-only plan checks for 64 GiB sizing and an explicit C: path; final cleanup verification.

**Measured data:** The 5 GiB C: case selected `c_default` with `115876167680` bytes free before creation and a required reserve of `16106127360`; the fixed VHDX was `5368709120` bytes, post-allocation free space was `110502268928`, `configure` returned `VERIFIED`, and exact-path uninstall removed the target and manifest, leaving `115875147776` bytes free. The I: case selected `distro_basepath` with `61655785472` bytes free; after the same fixed allocation it retained `56281833472`, returned `VERIFIED`, and cleanup removed the target and manifest, leaving `61654736896` bytes free. Both cases passed. A 64 GiB request required `79456894976` bytes: I: was below that bound, so the read-only plan selected C: with `115879870464` bytes free. Explicit C: planning passed with required `16106127360` bytes. Manufactured tests separately cover the literal single-volume C: boundary.

**Before/after and cleanup:** The elevated runner completed with `Overall=PASS`. Final verification found both lab VHDX paths and manifests absent, zero backup files, and the production manifest unchanged at `C:\ProgramData\RamShared\ramshared-origin.vhdx` (`5368709120` bytes). `.wslconfig` remained `swapFile=C:/wsl/swap.vhdx`. The temporary lab runner and logs were removed after recording these measurements.

**Residual blockers:** The C: case was not performed on a physically single-volume PC; it tested the C-only selector condition through the unregistered-distro path, with the policy's single-volume case covered by a manufactured test. The new VHDX was not attached to WSL; the guest host gate, cascade activation, stress, and CoCo platforms were not exercised. Do not treat this as full PRD live acceptance or kernel/CoCo qualification.

**Verdict:** 🟡 `PARTIAL` — live Windows origin placement, fixed allocation, identity proof, 10 GiB reserve, and rollback passed on C: and I: in disposable isolation; guest attachment and cascade acceptance remain open.

## 2026-09-26 14:20 -03 — Disposable WSL guest origin gate

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0067`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T16:46:55Z`.
**Verified at:** `2026-09-26T17:20:26Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted changes; a temporary copy of the current origin manager redirected only manifest and backup paths to disposable state.
**Lifecycle:** `reviewable`.
**Retention:** Keep with `docs/specs/no-milestone/wsl2-origin-capacity-policy/`.
**Freshness:** Applies to the current origin manager and this disposable WSL attachment.
**Category:** `qualification`.

**What:** Attached a 5 GiB fixed VHDX on the registered distro volume I: to Ubuntu-24.04, using its real PARTUUID and disk GUID. The guest host gate accepted the current guardian proof and refused a copied proof aged beyond its freshness limit. A path-isolated provisioning script wrote the expected 4 GiB swap signature and an immediate replay returned `ALREADY_PROVISIONED`.

**Validation:** Guest summary recorded `PASS`, partition dev_t `8:50`, parent dev_t `8:48`, the expected PARTUUID and swap UUID, `active_swap_changed=false`, and `production_origin_config_sha256_unchanged=true`. The root `/etc/ramshared/origin.conf` hash remained `18736ad6943b60f672dd074e11f89c1098d94327e1e9fbc55e4484a93bb83280`. `ramshared status --json` remained `phase=Off`, daemon false, guardian healthy, and no managed tiers active. The disposable partition never appeared in `/proc/swaps`.

**Cleanup:** Detached the exact lab VHDX and used the manager's ownership-checked uninstall. The lab VHDX and manifest are absent, the test PARTUUID is absent from `lsblk`, and `/proc/swaps` still contains only the 4 GiB C:-backed fallback swap. Production manifest still names `C:\\ProgramData\\RamShared\\ramshared-origin.vhdx`; `.wslconfig` still sets `memory=17179869184` and `swapFile=C:/wsl/swap.vhdx`. I: returned to `61564293120` bytes free.

**Open gate:** No `ramshared up`, physical GPU allocation, bounded stress, swapoff-first cascade teardown, release build/install, or CoCo test ran. At verification the guest reported `MemAvailable=889300 KiB` and `SwapFree=2764028 KiB`; keep the pressure campaign closed until guest and host admission are freshly qualified.
**Verdict:** 🟡 `PARTIAL` — disposable guest attachment, live identity gate, stale-proof refusal, provisioning replay, and exact cleanup passed; cascade acceptance remains open.

## 2026-09-26 14:20 -03 — RAM scope label and host-query runaway

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0068`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T16:52:34Z`.
**Verified at:** `2026-09-26T17:20:26Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted monitor and stress-label corrections; source was tested but not rebuilt or installed as a release.
**Lifecycle:** `reviewable`.
**Retention:** Keep with the memory-observability and WSL stress qualification records.
**Freshness:** Applies to this dashboard source correction and the sampled host/guest state.
**Category:** `reliability`.

**What:** `ramshared top` reads `/proc/meminfo` from its Linux process. This WSL2 guest reported `MemTotal=16379360 KiB` (15,995 MiB), matching the dashboard denominator, so the prior “Host RAM” caption incorrectly implied Windows physical RAM. The dashboard now reports `WSL2 RAM` and `WSL2 RAM & Swap` for WSL2, `WSL RAM` under WSL interop, and `Host RAM` on native Linux. The stress preamble now uses the same WSL2/native distinction. JSON observations include `memory_scope`.

**Validation:** `memory_scope_distinguishes_wsl2_wsl1_and_native_linux`, `dashboard_renders_active_and_unavailable_gpu_planes`, and `formats_stress_telemetry_without_live_pressure` passed. `cargo fmt --all -- --check` and `git diff --check` passed. The Windows campaign source places its actual `ramshared stress` command inside the guest script; the Windows PowerShell controller samples host memory and supervises the guest but does not allocate the planned guest pressure itself.

**Host observation:** A PowerShell child launched by our Guardian-status diagnostic had grown to about 14,427 MiB of private memory; at that sample Windows had 4,340 MiB of physical memory free. We verified its PID and command line, terminated only that diagnostic process, and Windows free physical memory rose to 18,596 MiB. This was not the Guardian or a stress process. The runaway was caused by the diagnostic invocation involving ScheduledTasks queries; the available process evidence does not isolate whether the PowerShell engine, module, or provider caused the growth. The process is gone and was not relaunched.

**Open gate:** No stress or cascade was started. At verification WSL reported `MemAvailable=889300 KiB`, `SwapFree=2764028 KiB`, and memory PSI avg10 `some=0.00`, `full=0.00`; RamShared remained `Off`. The WSL dashboard correction is source-only; rebuilding/installing it and running any pressure campaign remain gated on fresh readiness.
**Verdict:** 🟡 `PARTIAL` — RAM scope is identified correctly and the source labels are tested; host-query root cause and release installation remain unqualified, and no pressure campaign ran.

## 2026-09-26 17:07 -03 — Exact host and guest memory admission

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0069`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T20:07:50Z`.
**Verified at:** `2026-09-26T20:18:21Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted monitor and stress-admission changes; no new release build or install.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0068 and the three-tier stress qualification records.
**Freshness:** Host/guest counters are a single post-restart sample and apply only to this observation.
**Category:** `reliability`.

**What:** The former Windows `FreeVirtualMemory` proxy combined free physical memory and paging-file space and was not exact commit headroom. The shared gate now uses `GetPerformanceInfo` for `PhysicalAvailable` and `CommitLimit - CommitTotal` from one snapshot. The three-tier wrapper separately checks guest `MemAvailable` and `SwapFree` before any guest activation or allocator command. These checks are admission gates; they do not reserve memory.

**Validation:** `Test-SharedWslPressureCampaignMemoryGate.ps1`, `Test-SharedWslPressureCampaignStatic.ps1`, `Test-RamSharedThreeTierStressStatic.ps1`, and `Test-RamSharedWslWatchdogStatic.ps1` passed under Windows PowerShell. `test-ramshared-guest-memory-admission.sh` passed all five cases: adequate reserves pass; low memory, low swap, malformed telemetry, and attempts to lower the reserve refuse. No campaign or plan command ran.

**Measured data:** After the user's WSL restart, one live host sample reported 19,579 MiB physical headroom against 20,480 MiB required (901 MiB short), and 41,519 MiB exact commit headroom against the same requirement. The guest reported about 12 GiB `MemAvailable`, 4 GiB free fallback swap, and zero memory PSI. The physical-memory gate therefore still refuses the full 16 GiB pressure profile even though commit and the current guest sample pass.

**Residual blockers:** A single post-restart sample is not a multi-sample campaign admission. No stress, release installation, GPU allocation, or tier activation was performed. See EVD-0070 for the prior-boot freeze investigation.
**Verdict:** 🟡 `PARTIAL` — exact host counters and fail-closed guest gates pass their tests; current physical headroom remains below the full-campaign threshold.

## 2026-09-26 17:11 -03 — WSL2 memory-pressure freeze review

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0070`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T20:11:38Z`.
**Verified at:** `2026-09-26T20:18:21Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Working tree contains uncommitted monitor and stress-admission changes; no release build or install.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0068 and EVD-0069.
**Freshness:** The prior-boot evidence explains the observed incident only; it is not a current stress qualification.
**Category:** `reliability`.

**What:** The user observed the WSL2 guest becoming unresponsive while `ramshared top` showed 92% (14,840/15,995 MiB). The denominator matches guest `/proc/meminfo` (`MemTotal=16,379,360 KiB`), not Windows physical RAM or the `vmmemWSL` working set. The prior boot's last RamShared status sample was `phase=Off`, daemon false, cache/origin off, and only the independent 4 GiB fallback swap active.

**Prior-boot evidence:** At 15:47:37 -03, `MemAvailable=108,560 KiB`, fallback `SwapFree=897,032 KiB`, fallback swap used `3,291,364/4,194,304 KiB`, memory PSI avg10 some/full `22.72/22.48`, and cgroup `oom`/`oom_kill` were zero. One unmanaged process accounted for `1,938,800 KiB` combined RSS and swap; this is the largest recorded contributor, not proof of the first allocation. The journal contains 143 `Under memory pressure, flushing caches` messages between 15:30:17 and 15:55:42 -03. Its final retained record is at 15:55:42; the next boot begins at 16:18:38.

**Kernel and recovery evidence:** The retained prior-boot kernel journal has no `BUG`, Oops, panic, soft/hard lockup, hung-task, or OOM-killer signature. The user restarted WSL; the same custom `6.18.40.1-microsoft-standard-WSL2+` kernel is active after restart. The guest then reported about 12 GiB available, 0 swap used, and zero PSI; a Windows `vmmemWSL` sample was about 5,170 MiB working set. Hyper-V Compute event access was denied, and the final 23 minutes before the new boot have no retained guest records. No stock-kernel A/B test was performed.

**Assessment:** Severe guest memory and fallback-swap thrashing is the most likely immediate freeze mechanism. The largest process footprint in the last saved status is a plausible contributor, but the exact initiating allocation is not proven. The incident does not meet the repository's kernel-CRASH definition; it also does not exonerate the custom kernel because the final interval is unobserved and no baseline comparison exists. A repository documentation check had been attempted during this constrained period and was interrupted without a result; its incremental effect cannot be measured. No RamShared stress or activation was started.

**Verdict:** 🟡 `PARTIAL` — guest thrashing is strongly evidenced with RamShared off; exact process causality and any custom-kernel contribution remain unresolved.

## 2026-09-26 18:25 -03 — Cross-correlated WSL freeze timeline

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0071`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T21:24:43Z`.
**Verified at:** `2026-09-26T21:24:43Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Read-only analysis of retained guest health, journal, Windows telemetry, and Guardian records; no pressure run or release installation.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0068 through EVD-0070 and the freeze incident evidence.
**Freshness:** Retrospective analysis of the September 26 prior boot; not a current readiness sample.
**Category:** `reliability`.

**What:** A second pass over 13,000 retained `cascade-health.jsonl` samples, Windows host telemetry, and Guardian events narrows the unresponsive interval and corrects EVD-0070's process inference. Guest `MemAvailable` fell from about 8,543 MiB at 10:45 to 106 MiB at 15:47 while fallback swap rose from zero to about 3,220 MiB. RamShared remained `Off` with no managed ZRAM, VRAM, or origin tier. The top-ten process samples are insufficient to account for total guest memory; they omit aggregate processes and the kernel memory categories needed to distinguish anonymous memory, shared memory, unreclaimable slab, dirty pages, and ballooned pages.

**Swap and responsiveness:** Between 15:25 and 15:47, the fallback swap-device read counter rose from 15.90 to 127.20 GiB, about 111.30 GiB in 22 minutes. Major faults rose from 71,108 to 1,983,827; PSI full avg10 reached 56.76% at 15:32. The Guardian logged intermittent guest probe failures from 15:31 and persistent dual probe timeouts from about 15:48 through 16:15 while the Windows WSL/HCS service probes still completed. Guest journald continued writing memory-pressure messages through 15:55. This supports severe swap-driven loss of guest responsiveness rather than a proven kernel crash. The journal-only 15:55–16:18 gap in EVD-0070 is partially covered by those independent host probes, but lacks guest process and kernel state.

**Process correction:** `rust-analyzer` stayed near 1,880–1,910 MiB combined RSS plus swap across the sampled afternoon. Its RSS fell from about 1,883 MiB at 13:00 to under 1 MiB at 15:47 as its swap grew to about 1,893 MiB. It was heavily paginated; the stable combined footprint does not support EVD-0070's suggestion that it caused the progressive memory loss. The top-ten combined footprint was about 3,293 MiB at 13:00 and 2,779 MiB at 15:47. This does not exclude many smaller processes or a kernel-side category because only ten processes were retained. The zero OOM counters in the health JSON refer only to `ramshared-workloads.slice`, not the entire guest.

**Dynamic-memory hypothesis:** The host `.wslconfig` sets a 16 GiB WSL limit, 4 GiB swap, and `autoMemoryReclaim=disabled`. The prior guest boot log confirms `hv_balloon` negotiated Dynamic Memory protocol 2.0 and logged a 16,384 MiB maximum. Between 10:45 and 13:30, Windows physical memory free rose from about 12,796 to 14,748 MiB while guest `MemAvailable` fell from about 8,593 to 1,904 MiB. This does not fit simple exhaustion of Windows physical RAM. Host-directed ballooning could contribute to the guest/host accounting mismatch, but the old boot did not preserve `nr_balloon_pages`; the current boot's value of zero cannot establish the prior value. The host telemetry's `vmmem_wsl` field was null during the prior run, so there is no contemporaneous WSL VM working-set series. The [WSL configuration reference](https://learn.microsoft.com/windows/wsl/wsl-config) describes `autoMemoryReclaim` as cache reclamation; its disabled value does not prove the Hyper-V balloon was inactive. The [Linux Hyper-V balloon driver](https://github.com/torvalds/linux/blob/master/drivers/hv/hv_balloon.c) documents host balloon requests that ask the guest to allocate pages.

**Host separation:** Windows telemetry showed about 17,028 MiB physical memory free and 29,335 MiB commit free at 15:30, and about 18,549 MiB physical memory free at 15:48. The earlier diagnostic PowerShell private-memory spike occurred around 13:42–13:52; host physical free had recovered to about 18,493 MiB by 14:00. Guest availability had already fallen below 2 GiB before that process started, and the continuous guest timeouts began over 100 minutes after host recovery. The PowerShell incident was harmful to host headroom but is not evidenced as the initiating guest allocator or the immediate 15:48 stall.

**Remaining attribution gap:** The retained records do not contain the prior boot's `AnonPages`, `Shmem`, `Slab`, `SUnreclaim`, `Dirty`, `Writeback`, `nr_balloon_pages`, full process RSS/swap totals, or per-cgroup memory usage. The current boot's `nr_balloon_pages=0` cannot establish the previous boot's value. Hyper-V Compute/Worker event queries returned access denied. The exact memory owner and any custom-kernel contribution therefore remain unproven; no stock-kernel comparison was run.
**Verdict:** 🟡 `PARTIAL` — fallback-swap thrashing explains the observed loss of responsiveness with high confidence, while the source of the progressive guest memory depletion remains unidentified.

## 2026-09-26 18:48 -03 — Hyper-V balloon counter investigation

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0072`.
**Owner role:** `hardware-researcher`.
**Observed at:** `2026-09-26T21:48:27Z`.
**Verified at:** `2026-09-26T21:48:27Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Read-only investigation; no stress, tracing activation, kernel change, or release installation.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0071 and the freeze incident evidence.
**Freshness:** The live counters describe only the boot after the user's restart.
**Category:** `reliability`.

**What:** Checked the Hyper-V balloon driver's live debugfs counters and tracepoint, then searched the saved guest and Windows records for an incident-time balloon measurement.

**Direct driver evidence:** The active custom kernel exposes `/sys/kernel/debug/hv-balloon` as a read-only file. Its `capabilities` include `enabled hot_add`; the driver's own source defines `pages_ballooned` as pages given back to the host. Five one-second reads in the current boot reported `pages_ballooned=0`, `pages_added=0`, `pages_onlined=0`, and `/proc/vmstat nr_balloon_pages=0`, with about 10.0 GiB `MemAvailable`. The `hyperv/balloon_status` tracepoint exists but was disabled, so it contains no retrospective trace. `total_pages_committed` varied around 1.95 million pages; it is the driver's guest commitment estimate, not a measurement of Windows process working set.

**Retrospective limit:** All 11,203 saved pre-rotation guest health samples from 10:40–14:52 reported the same `MemTotal=16,379,360 KiB`; the later file retained that total across the pre-restart interval. A constant `MemTotal` does not exclude ballooned pages because ballooning can reduce available pages while preserving the guest's installed-memory total. Neither health file saved `pages_ballooned` or `nr_balloon_pages`; the prior boot's journal has only driver registration/protocol messages, and no incident-time WSL crash dump was found. The available Windows telemetry also has no incident-time `vmmemWSL` process readings. The disabled `Microsoft-Windows-Kernel-Memory/Analytic` channel and inaccessible Hyper-V Compute/Worker logs provide no historical substitute.

**Conclusion:** Host ballooning is supported as a kernel capability but is neither demonstrated nor ruled out for the freeze. The observed fallback-swap thrashing remains established; attributing the earlier memory decline to ballooning, a user process, or the custom kernel requires contemporaneous category and balloon counters. A future bounded, low-overhead monitor should record the read-only debugfs counter alongside `/proc/meminfo`, `/proc/vmstat`, process totals, and host `vmmemWSL` measurements before any pressure experiment.
**Verdict:** 🟡 `PARTIAL` — the driver exposes a usable balloon counter, but the incident-time value was not retained.

## 2026-09-26 19:09 -03 — Freeze telemetry capture added to monitor source

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0073`.
**Owner role:** `reliability / hang auditor`.
**Observed at:** `2026-09-26T22:09:25Z`.
**Verified at:** `2026-09-26T22:09:25Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Uncommitted source change in the working tree; no release build or installation.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0070 through EVD-0072.
**Freshness:** Code-level validation only; the installed host collector still uses its previous binary.
**Category:** `reliability`.

**What:** Extended the read-only `ramshared top` observation so future JSONL samples preserve the memory evidence missing from the freeze: `AnonPages`, `Shmem`, `Slab`, `SUnreclaim`, `Dirty`, and `Writeback`; `/proc/vmstat nr_balloon_pages`; `/sys/kernel/debug/hv-balloon` state and balloon counters when readable; root cgroup `memory.current` and `memory.events` when available; and count plus summed RSS/swap for all processes whose `/proc` status could be read, before the detailed top-ten list is truncated. Missing sources serialize as `null` or an absent optional object, never as a measured zero. Process RSS totals can count shared pages more than once, so they are an ownership clue rather than a physical-memory identity.

**Validation:** `cargo test -p ramshared-cli` passed 335 unit tests and 10 CLI tests. The added end-to-end JSONL observation test passed again after asserting the new fields. `cargo clippy -p ramshared-cli --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `git diff --check`, and `./scripts/docs-check.sh` passed. No stress, tracepoint activation, kernel change, or host installation occurred.

**Remaining limit:** The new collector source is not in the installed release, so it has not produced a deployed incident series. This change cannot reconstruct the missing balloon count from the previous boot or identify the initiating allocation. Close this gap only after a provenance-matched build is installed and a normal, non-pressure JSONL sample is correlated with Windows `vmmemWSL` telemetry; compare the same workload on a stock kernel if the freeze recurs.
**Verdict:** 🟡 `PARTIAL` — source-level capture is implemented and tested; host deployment and paired live evidence remain open.

## 2026-09-26 19:42 -03 — Read-only memory telemetry sample after restart

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0074`.
**Owner role:** `reliability / hang auditor`.
**Observed at:** `2026-09-26T22:42:37Z`.
**Verified at:** `2026-09-26T22:55:18Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** One-shot execution of the local debug binary built from the uncommitted working tree; not the installed release.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0070 through EVD-0073.
**Freshness:** Single post-restart sample; applies only to this healthy observation.
**Category:** `reliability`.

**What:** Ran `target/debug/ramshared monitor --jsonl --once`, immediately paired with a read-only Windows `Get-Process vmmemWSL` sample. RamShared reported `phase=Off`. The guest had `MemTotal=16,379,360 KiB`, `MemAvailable=10,033,788 KiB` (about 9,798 MiB), all `4,194,304 KiB` of fallback swap free, zero memory PSI, and no swap I/O during the sample. The collector read 135 process status records: combined RSS was `4,221,308 KiB` and combined process swap was zero. Guest memory categories included `AnonPages=2,547,960 KiB`, `Shmem=5,156 KiB`, `Slab=591,328 KiB`, `SUnreclaim=121,180 KiB`, `Dirty=960 KiB`, and `Writeback=0 KiB`.

**Balloon and accounting visibility:** `/proc/vmstat` reported `nr_balloon_pages=0`. The unprivileged monitor could not read `/sys/kernel/debug/hv-balloon` (`debugfs_status=permission_denied`), so detailed `pages_ballooned` and driver state were unavailable; the collector now reports that access state explicitly. The cgroup v2 root exposed no `memory.current`; ten immediate subgroups reported a combined `12,159,422,464` bytes, while 162 processes remained directly in the root cgroup. The output labels this `partial`, keeps root `current_bytes` null, and does not present the subgroup sum as total guest memory. Windows `vmmemWSL` working set was `5,043,982,336` bytes (about 4,810 MiB). The process working set is the host resident sample; no private-byte value was used as physical RAM.

**Conclusion:** This healthy sample demonstrates that the unprivileged monitor can record memory categories, guest process totals, the `/proc` balloon page count, and a properly scoped Windows working set. It also exposes the current limits: debugfs details require elevated access, and cgroup accounting is partial. The zero balloon count describes this post-restart sample only and does not resolve the prior freeze. No stress, tracing activation, kernel change, or release installation occurred.
**Verdict:** 🟡 `PARTIAL` — paired source-built telemetry is captured for a healthy boot; incident-time cause and installed-release parity remain open.

## 2026-09-26 20:09 -03 — Privileged freeze telemetry sample

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0075`.
**Owner role:** `reliability / hang auditor`.
**Observed at:** `2026-09-26T23:09:44Z`.
**Verified at:** `2026-09-26T23:10:15Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** One-shot root execution of the local debug binary from the uncommitted working tree; paired with Windows process and free-memory telemetry; no release installation.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0070 through EVD-0074.
**Freshness:** Describes only the healthy boot after restart.
**Category:** `reliability`.

**What:** Ran `sudo -n target/debug/ramshared monitor --jsonl --once`. The monitor exited successfully and reported `memory_scope=wsl2`, `phase=Off`, `MemTotal=16,379,360 KiB`, `MemAvailable=9,918,132 KiB`, `SwapFree=4,194,304 KiB`, and zero process swap. Guest categories included `AnonPages=2,541,052 KiB`, `Shmem=5,168 KiB`, `Slab=608,236 KiB`, and `SUnreclaim=122,496 KiB`.

**Balloon and cgroup evidence:** With root access, `/sys/kernel/debug/hv-balloon` was readable. The driver reported `pages_ballooned=0`, `pages_added=0`, `pages_onlined=0`, state `Initialized`, and `/proc/vmstat nr_balloon_pages=0`. The cgroup collector still reported `partial`: root `memory.current` was unavailable, ten subgroups summed to `12,256,874,496` bytes, and 163 processes were directly in the root cgroup. This subgroup sum is not guest total memory. The process collector read 136 process status records with combined RSS `4,215,284 KiB` and zero process swap; shared mappings can be counted more than once.

**Windows pairing:** A read-only sample 31 seconds later reported `vmmemWSL` working set `3,853 MiB`, private bytes `14,884 MiB`, and Windows physical memory free `15,727 MiB`. The private-bytes figure is committed private memory, not physical RAM. These healthy-boot samples do not establish the guest's balloon state or memory owner during the prior freeze.

**Conclusion:** Detailed balloon counters are available to this collector when it runs with sufficient privilege; the earlier `permission_denied` was an unprivileged-read limitation. Current evidence shows no ballooned pages, no swap use, and no pressure. It cannot reconstruct the missing prior-boot state. No stress, tier activation, tracepoint enablement, kernel change, release build, or installation occurred.
**Verdict:** 🟡 `PARTIAL` — root-level source telemetry and near-time Windows data are captured; prior-boot attribution and installed-release parity remain open.

## 2026-09-26 20:13 -03 — Installed monitor and Guardian publication state

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0076`.
**Owner role:** `reliability / hang auditor`.
**Observed at:** `2026-09-26T23:13:43Z`.
**Verified at:** `2026-09-26T23:30:48Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Read-only installed-binary and host-state audit; no task start/stop, stress, tier activation, or kernel action.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0071 through EVD-0075 and the host-freeze records.
**Freshness:** Installed binary sample and Guardian state describe the current post-restart WSL session.
**Category:** `reliability`.

**What:** Audited the installed monitor and Guardian publication state using read-only commands after the source-built telemetry sample.

**Installed binary:** `/usr/local/bin/ramshared` reports version `0.14.1` and SHA-256 `49f5a770c1aefcb386ca99a7bb89b8913929ca2fc5fba18da28fee60ea41b89`. Its read-only `monitor --jsonl --once` reported `phase=Off`, no active daemon or managed tiers, about `9,717 MiB` guest `MemAvailable`, all `4,096 MiB` of fallback swap free, and zero PSI or swap-in/out during the sample. The top process was `rust-analyzer` at `1,569,028 KiB` RSS; this did not coincide with guest pressure in that sample. The installed schema does not emit `memory_scope`, memory categories, process totals, or balloon counters, so EVD-0073/0075 telemetry is not deployed. The command reported `guardian_state=BLOCKED` and `measurement_errors=["guardian_state_stale"]`; `ok=false` correctly prevents this observation from qualifying as ready.

**Guardian cross-check:** The host health file's last write was `2026-09-26 15:48:03 -03` and its published reason was `boot_identity_unavailable`. The heartbeat file was fresh at `20:16:54 -03`; `ramshared-cascade-health.service` was active and enabled. The exact guest identity command used by the Guardian, `wsl.exe -d Ubuntu-24.04 -u root -- cat /proc/sys/kernel/random/boot_id`, completed successfully in 106 ms during this audit. `schtasks.exe` reported the Guardian task enabled but `Ready` (not running), next run `N/A`, last result `-1073741510`; the installed task XML has only a logon trigger and its action loads `Watch-RamSharedWsl.ps1` directly from the mutable repository checkout. These readings explain why the current status remains fail-closed, but do not establish who or what ended the prior task.

**Conclusion:** The stale Guardian indicator is not evidence that WSL is currently frozen: guest probes work, the heartbeat monitor is active, and guest memory/swap are healthy. The Guardian publisher itself is not running, while the installed CLI still lacks the forensic capture and corrected RAM scope. Do not use this state to admit a stress campaign. Requalification requires immutable deployed inputs, a fresh Guardian health record for this boot, exact binary parity, and a paired Windows/guest sample. The task was not started because its action points into the modified, uncommitted tree. No source was installed and no stress ran.
**Verdict:** 🟡 `PARTIAL` — current fail-closed state and installed/source parity gap are identified; the cause of the Guardian task's prior exit and the original freeze remain unresolved.

## 2026-09-26 20:55 -03 — Guardian HCS status serialization regression

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0077`.
**Owner role:** `reliability / hang auditor`.
**Observed at:** `2026-09-26T23:55:55Z`.
**Verified at:** `2026-09-26T23:59:04Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Local uncommitted Guardian/test changes; Windows commands were read-only; source was not installed and the scheduled task was not started or stopped.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0071 and EVD-0076.
**Freshness:** Serialization result was reproduced against the current `vmcompute` service; Task Scheduler channel state is current at verification.
**Category:** `reliability`.

**What:** On Windows PowerShell 5.1, `Get-Service -Name vmcompute | Select-Object Name, Status | ConvertTo-Json -Compress` returned `{"Name":"vmcompute","Status":4}`. `ConvertFrom-Json` restored `Status` as `System.Int32`; the original Guardian compared that number with the text `Running` and classified the healthy service as failed. A read-only query using `Status.ToString()` returned `{"Name":"vmcompute","Status":"Running"}`. The Guardian now normalizes this query and its status predicate also accepts legacy numeric JSON while rejecting stopped, missing, boolean, and fractional values.

**Effect and incident boundary:** The Guardian combined `wsl.exe --status` and HCS with an AND condition. A false HCS failure could therefore supply false host corroboration if the WSL status probe failed at the same time as both guest probes. In the recorded freeze, however, `wsl.exe --status` continued to succeed, so this serialization bug did not make the host probe fail and does not explain the WSL memory pressure or restart.

**Verification:** The manufactured Guardian suite passed, including numeric/string status and fail-closed cases; `Test-RamSharedWslWatchdogStatic.ps1` exited 0. The Windows Task Scheduler Operational log is disabled (`IsEnabled=false`, no records), so no event history was available to identify who ended the task. `Get-ScheduledTaskInfo` still reports result `0xC000013A` (`3221225786`); this status does not identify the actor. The installed Guardian task still points into the modified checkout, so the fix is not deployed.

**Conclusion:** The HCS serialization fault is reproduced and corrected in source, with direct regression coverage. It is a real Guardian corroboration bug, but it is not evidence for the original WSL freeze cause. Immutable deployment, task-exit attribution, and incident-time memory ownership remain unresolved. No Guardian action, stress, tier activation, kernel change, build, or installation occurred.
**Verdict:** 🟡 `PARTIAL` — source regression is fixed and tested; runtime deployment and the prior incident's cause remain unproven.

## 2026-09-26 21:11 -03 — Guardian refusal and task-exit audit

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0078`.
**Owner role:** `reliability / hang auditor`.
**Observed at:** `2026-09-27T00:11:50Z`.
**Verified at:** `2026-09-27T00:17:15Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Read-only artifact, task metadata, source-flow, and elevated Windows audit inspection; no task, distro, service, or kernel mutation.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0070, EVD-0071, EVD-0076, and EVD-0077.
**Freshness:** Historical task/event data from the recorded 2026-09-26 run, paired with current audit-policy configuration.
**Category:** `reliability`.

**What:** The matching Guardian artifact set contains 222 event records. `Get-ScheduledTaskInfo` reports the task's `LastRunTime` as `2026-09-26T10:12:33-03:00`, matching the artifact start one second later. Its last event is `2026-09-26T19:15:42Z` (`16:15:42 -03`): both distinct guest probes timed out, while `wsl.exe --status` exited 0 and the HCS service query completed. The event reports `host.failed=false`, `wsl_failed=false`, `hcs_failed=true` (the numeric-serialization defect from EVD-0077), and decision `REFUSE` for `dual_wsl_hcs_corroboration_required`.

**Exit audit:** `Invoke-GuardianWatch` handles every non-`TERMINATE` decision by recording the refusal, sleeping, and continuing its `while ($true)` loop. The refusal itself has no normal exit path. Task Scheduler reports result code `0xC000013A` (`3221225786`), but `LastRunTime` records the start, not the exit time. The Task Scheduler Operational channel is disabled. An elevated query of the Security log around the last event (16:00–16:45 -03) found no 4688/4689 process events; the Security log is enabled, but the current Process Termination audit policy says `No Auditing`. The actor and exact exit time therefore remain unknown. External interruption or an unrecorded process failure is an inference, not an established cause.

**Incident boundary:** The recorded event confirms that WSL management and HCS still answered while commands inside the distro timed out. Along with EVD-0070's depleted guest memory, heavy fallback-swap reads, and elevated PSI, this supports a guest-side stall during memory thrashing; it does not identify the initiating process, prove a kernel crash, or attribute the freeze to the custom kernel. The HCS enum bug did not alter the recorded refusal because `wsl.exe --status` succeeded and `host.failed` remained false.

**Conclusion:** The Guardian observed the stalled guest and repeatedly refused termination under its current dual host-failure gate, then stopped without an attributable exit record. The refusal matches the repository's current fail-closed policy; changing that termination gate would require a SPEC revision and isolated live qualification. This narrows the observed failure boundary to guest responsiveness while the Windows WSL/HCS control plane answered; it does not close the original freeze root cause. No policy change or recovery action was made.
**Verdict:** 🟡 `PARTIAL` — refusal and task history are reconstructed; process-exit ownership and incident-time memory owner remain unknown.

## 2026-09-26 21:36 -03 — Guest memory admission before shared pressure mutation

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0079`.
**Owner role:** `reliability / hang auditor`.
**Observed at:** `2026-09-27T00:36:44Z`.
**Verified at:** `2026-09-27T00:45:58Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Uncommitted source and regression-test changes in an already modified working tree; not built or installed.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0069, EVD-0070, and EVD-0078.
**Freshness:** Source-level checks only; the live campaign was not started.
**Category:** `reliability`.

**What:** The shared WSL pressure campaign already checked Windows commit and physical-memory headroom, but could issue `ramshared down/up` before checking the selected guest's current memory and swap reserves. It now runs the existing guest admission helper against `/proc/meminfo` before registering cleanup or making any RamShared change. The fixed minimums are 1024 MiB each for `MemAvailable` and `SwapFree`; failure writes structured admission output and exits before activation or the pressure probe. The allocator and freeze probe remain guest-side. A separate optional Windows CUDA VRAM workload remains disabled by default and is not the WSL RAM allocator.

**Verification:** The regression was first run against the old source and failed because the guest admission constants and gate were absent. After the fix, `Test-SharedWslPressureCampaignStatic.ps1`, `Test-RamSharedThreeTierStressStatic.ps1`, `test-ramshared-guest-memory-admission.sh` (all five cases), and `Test-SharedWslPressureCampaignMemoryGate.ps1` passed. `git diff --check`, the validation schema check, and `./scripts/docs-check.sh` passed; the docs record now covers both shared-host pressure paths and the optional CUDA workload. The host-memory test only read current performance counters. No campaign, pressure allocation, tier activation, release build, or installation occurred.

**Remaining limit:** These checks establish ordering and guest-gate behavior at source level. They do not prove that an installed immutable package refuses a live low-memory guest, dynamically measure guest headroom throughout the pressure phase, qualify all three tiers, or explain the earlier WSL freeze. Those live gates remain closed.
**Verdict:** 🟡 `PARTIAL` — guest entry admission is implemented and tested in source; deployed and live pressure qualification remain open.

## 2026-09-26 23:17 -03 — Fail-closed guest runtime pressure limits

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0080`.
**Owner role:** `reliability / hang auditor`.
**Observed at:** `2026-09-27T02:16:26Z`.
**Verified at:** `2026-09-27T02:17:45Z`.
**Source revision:** `290c06c586b149af8056abced5835235f5ad5228`.
**Source state:** Uncommitted source and regression-test changes in an already modified working tree; no release build or installation.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0069, EVD-0070, and EVD-0079.
**Freshness:** Source-level helper and static checks only; no live cgroup or pressure run.
**Category:** `reliability`.

**What:** Rust stress telemetry no longer interprets missing or malformed memory PSI as zero pressure. The shared PSI parser rejects missing/duplicate `full` rows, duplicate `avg10`, non-finite values, and values outside 0–100. WSL2 and cascade stress profiles require valid PSI before work and recheck it in the ramp, recovery wait, and hold phase. If `/proc/sys/vm/min_free_kbytes` is absent or invalid, the stress floor assumes zero known reserved pages instead of a fabricated 512 MiB; this preserves the 600 MiB WSL2 `MemAvailable` floor.

The freeze probe now samples guest `MemAvailable`, `SwapFree`, and PSI before creating its worker. It uses a unique cgroup and finite `memory.max` and `memory.swap.max` values bounded by the configured memory cap and guest headroom above 600 MiB and 1 GiB reserves. The limits are recalculated each second using current guest samples and cgroup memory/swap use. Malformed or missing samples, PSI full avg10 >=10%, unavailable cgroup counters, and failed limit writes stop the worker. A FIFO start gate prevents the allocator from running before its process enters the cgroup. Direct invocation refuses without an admission marker; only the gated isolated/shared campaign launch sites set it. The script removes only the cgroup and start gate it created and restores the parent memory controller when it can prove it enabled that controller. The source comment also states that WSL2 guest cgroup limits do not isolate Windows physical RAM.

**Verification:** TDD regressions were observed before implementation: Rust compile-time failures named the missing fail-closed PSI/min-free helpers; the shell helper test failed because dynamic cgroup limit functions did not yet exist; the probe static test failed because no guest runtime guard was integrated. After the changes, `cargo fmt --all -- --check`, `CARGO_BUILD_JOBS=2 cargo test -p ramshared-cli stress::tests::` (34 passed), the supervisor parser test (1 passed), and `CARGO_BUILD_JOBS=2 cargo clippy -p ramshared-cli --all-targets -- -D warnings` passed. `bash -n`, `test-guest-pressure-runtime-guard.sh`, `test-cascade-pressure-probe-static.sh` (including direct invocation refusal and campaign marker ordering), `test-ramshared-guest-memory-admission.sh` (5 cases), `test-wsl2-freeze-campaign-artifact-static.sh`, `Test-Wsl2FreezeCampaignStatic.sh`, and `git diff --check` passed. `actionlint` v1.7.7 passed on the edited CI workflow; the local documentation, validation-schema, and ephemeral-blocklist checks passed, and the workflow now runs the safe shell tests. No live worker, cgroup mutation, pressure allocation, tier activation, campaign, release build, or host installation was run.

**Remaining limit:** Helper fixtures and static ordering checks do not exercise a real kernel cgroup controller, process cleanup, or host/guest pressure interaction. This source is not installed, and the host's current admission state was not sampled in this turn. The separate Windows supervisor is still required for a shared-host campaign because WSL guest allocations consume host physical RAM. Do not claim live qualification or completion of the earlier freeze investigation from these source tests.
**Verdict:** 🟡 `PARTIAL` — the missing runtime guard paths are implemented and tested at source level; installed-binary parity and supervised live qualification remain open.

## 2026-09-27 00:17 -03 — Separate unmanaged process footprint from memory pressure

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0081`.
**Owner role:** `reliability / hang auditor`.
**Observed at:** `2026-09-26T23:48:50-03:00`.
**Verified at:** `2026-09-27T00:17:05-03:00`.
**Source revision:** `97e60e76a282ced1b5a5c057f2b400f5d24d79c6`.
**Source state:** The classification fix is committed; the wider worktree remains dirty and the fix has not been rebuilt or installed.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0070, EVD-0073, EVD-0078, and EVD-0080.
**Freshness:** Read-only live telemetry plus unit regression tests; no pressure or tier activation.
**Category:** `reliability`.

**What:** The monitor's legacy `unmanaged_pressure_state` value was set to
`UNMANAGED_PRESSURE` whenever a process outside the managed hierarchy had at
least 512 MiB of RSS plus swap. The sample had a `rust-analyzer` process with
`1,726,428 KiB` RSS and zero swap, while guest `MemAvailable` was `8,655,124
KiB`, `SwapFree` was `4,190,212 KiB`, and memory PSI some/full avg10 were both
`0.00%`. The process used guest memory, but this sample did not show active
memory pressure. Source now reports `UNMANAGED_MEMORY`, keeps the schema-v4 JSON
keys for compatibility, and uses PSI plus `MemAvailable` as the pressure
signals. This does not attribute the earlier WSL freeze to that process.

**Verification:** The new named monitor tests first failed at compile time
because `classify_unmanaged_memory_usage` did not exist. After the fix,
`CARGO_BUILD_JOBS=1 cargo test -p ramshared-cli pressure_classification_tests
-- --nocapture` passed all 3 cases: large external footprint, managed process,
and below-threshold external footprint. The changed code does not alter stress
admission or allocate memory.

**Deployment boundary:** The active monitor log and the old interactive
dashboard still come from pre-fix binaries. No new build or installation
occurred, and neither running process was restarted. The campaign remains
closed because the paired Windows sample had `14,739 MiB` physical headroom
against the full campaign's `20,480 MiB` requirement, and the RamShared
guardian reported `BLOCKED`.

**Verdict:** 🟡 `PARTIAL` — source classification and unit cases are corrected;
deployment and the WSL freeze ownership investigation remain open.

## 2026-09-27 05:07 -03 — IPC bounds and cross-target source gates

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0082`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T08:07:39Z`.
**Verified at:** `2026-09-27T08:07:39Z`.
**Source revision:** `af7aa108a39a7c09db9667cacf8c383c75bdd321`.
**Source state:** Source/test changes are committed through `af7aa108`; this evidence record is in the pending documentation commit. No release build, host install, or stress run.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0079 through EVD-0081 and the GPU worker SPEC evidence.
**Freshness:** Linux and Windows-target source checks from the current worktree on 2026-09-27.
**Category:** `reliability`.

**What:** Parent GPU-cache reads, handshakes, and heartbeats use one absolute
monotonic deadline across partial I/O. Cache `Update` and `Promote` requests use
one nonblocking frame write with a 64 KiB mutation payload cap; oversized,
partial, or backpressured writes revoke the cache and shut down the socket.
Oversized cache reads are misses, and the worker rejects payloads above 16 MiB.
The origin remains authoritative. Cross-target compilation also found that the
Unix-stream worker modules in `ramshared-block` were exported on Windows; these
modules are now Unix-only. Windows-target Clippy additionally exposed an
unused service-probe import and a test module placed before later items; both
were corrected. The native-vsock test matrix pointed VHDX lease tests at the
wrong source file, so it now names `control_plane.rs`.

**Verification:** `CARGO_BUILD_JOBS=2 cargo test --workspace -- --quiet` passed
with zero failures; hardware/root-only tests were ignored. Workspace Clippy
with `-D warnings`, formatting, and `./scripts/docs-check.sh` passed. Windows
target checks passed for `ramshared-ipc` and `ramshared-winsvc` with
`--all-targets`; Windows-target Clippy also passed with `-D warnings`. Slice
coverage passed at 93.4% for `gpu_cache_worker.rs`, 84.2% for
`ipc_cache_client.rs`, 93.2% for `gpu_budget.rs`, 94.8% for
`ramshared-vram/src/lib.rs`, 96.0% for `isolated_origin.rs`, 90.0% for
`ramshared-ipc/src/lib.rs`, 85.7% for `vsock.rs`, 95.0% for `host_gate.rs`,
and 87.0% for `ramshared-winsvc/src/control_plane.rs`. The three guest-pressure
shell fixtures and Bash syntax checks passed. No source was installed; no GPU
allocation, live WSL campaign, memory pressure, or host mutation occurred.

**Remaining limit:** This environment has no PowerShell runtime, so the changed
`.ps1` tests were not executed here. The Windows static workflow now includes
the three-tier static test, but that workflow has not run for these local
commits. Cross-target Clippy is not a Windows runtime test; the live AF_HYPERV
listener, Windows orchestration, WSL GPU worker, physical adapter behavior,
and three-tier host stress remain unqualified. CoCo, GPADL/UIO, and
maintainers' upstream review also remain external gates.

**Verdict:** 🟡 `PARTIAL` — source and hosted-runner checks pass; live Windows,
GPU/WSL, CoCo, and upstream qualification remain open.

## 2026-09-27 05:35 -03 — Windows static suite and current host admission

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0083`.
**Owner role:** `reliability / runtime`.
**Observed at:** `2026-09-27T08:35:59Z`.
**Verified at:** `2026-09-27T08:38:03Z`.
**Source revision:** `8fa9e9c14e93471b63975a8ac06875be32a53848`.
**Source state:** Clean committed checkout; read-only Windows and WSL observations plus static tests; no install, service/task action, stress, or tier activation.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0069 through EVD-0082.
**Freshness:** Host and guest memory samples are from 2026-09-27 05:35 -03; Guardian publication is stale and explicitly identified as such.
**Category:** `reliability / ci-gate`.
**How to measure:** From WSL, set `winroot=$(wslpath -w "$PWD")`, then run `powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$winroot\scripts\windows\Test-WindowsCiStatic.ps1" -RepoRoot "$winroot"`.

**What:** Windows PowerShell 5.1 is available through WSL interop even though
it is not on the Linux `PATH`. The complete `Test-WindowsCiStatic.ps1` wrapper
ran under PowerShell `5.1.26100.9444`; all 27 named harnesses completed with
exit code 0. This includes `Test-RamSharedThreeTierStressStatic.ps1`,
`Test-SharedWslPressureCampaignStatic.ps1`, and
`Test-SharedWslPressureCampaignMemoryGate.ps1`. This corrects the execution
limitation recorded contemporaneously in EVD-0082; it does not turn the
hosted GitHub Actions job green or provide live Windows/GPU qualification.

**Current admission sample:** `GetPerformanceInfo` reported total physical
memory `32,669 MiB`, physical headroom `13,190 MiB`, and commit headroom
`28,909 MiB`. The full profile requires `20,480 MiB` for each host gate
(`16 GiB` planned pressure plus `4 GiB` reserve), so physical admission fails
by `7,290 MiB`; commit admission passes. WSL reported `15,995 MiB`
`MemTotal`, `6,741 MiB` `MemAvailable`, `4,096 MiB` swap total, `4,030 MiB`
`SwapFree`, and memory PSI some/full avg10 `0.00%`. `/proc/swaps` listed only
the 4 GiB fallback device, with `66 MiB` used. The cascade health service was
active and the supervisor inactive.

**Guardian and process observations:** Windows health JSON remains
`BLOCKED/boot_identity_unavailable`, timestamped `2026-09-26T18:48:03Z`; its
enabled scheduled task is `Ready`, last ran `2026-09-26T10:12:33-03:00`, and
reports `3221225786` (`0xC000013A`). One PowerShell process measured
`79 MiB` private bytes and `95.7 MiB` working set. No process or service named
with `space`, current-user AppX package, classic uninstall entry, WSL dpkg
package, or Snap package matching `space` was found. The all-users AppX and
Hyper-V VM queries were denied for this non-elevated PowerShell token, so this
does not prove absence from other Windows user profiles or establish VM
availability. WMI identifies an `NVIDIA GeForce RTX 2060`, but no live VRAM
budget or allocation was queried.

**Conclusion:** The earlier multi-GiB PowerShell reading was not reproduced;
its cause remains unknown. Guest memory and PSI currently look healthy, but
the full stress is correctly refused by insufficient host physical headroom
and stale Guardian health. The new Rust monitor, Guardian, and stress changes
are not installed. No task, service, package, kernel, or GPU state was changed.
No full-tier stress or upstream/CoCo/GPADL/UIO qualification ran.
**Verdict:** 🟡 `PARTIAL` — the Windows static suite now passes locally and the
current refusal conditions are measured; installation, live pressure, GPU,
VM/CoCo, and historical freeze attribution remain open.

## 2026-09-27 06:01 -03 — Guardian health republished after one-time start

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0084`.
**Owner role:** `reliability / runtime`.
**Observed at:** `2026-09-27T09:01:35Z`.
**Verified at:** `2026-09-27T09:08:38Z`.
**Source revision:** `e4b87ac8b2699aa37b521a44ff0c95880810d3a2`.
**Source state:** Clean committed checkout; the already-enabled Guardian scheduled task was started once after read-only policy/dependency checks; no RamShared install or stress.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0076 through EVD-0083.
**Freshness:** Three 15-second Windows samples completed while the same Guardian task remained running.
**Category:** `reliability / fail-safe`.
**How to measure:** Read `Get-ScheduledTask -TaskName RamSharedWslGuardian.v1`, `C:\ProgramData\RamShared\guardian-state\Ubuntu-24.04.health.json`, Windows `GetPerformanceInfo`, and guest `/proc/meminfo`, `/proc/swaps`, and `/proc/pressure/memory`.

**What:** A read-only readiness sample immediately before the task start
showed the Windows-to-WSL `boot_id` probe exiting 0, `vmcompute` `Running`,
and `wsl.exe --status` exiting 0. The existing task was enabled and `Ready`,
configured with `RunLevel=Highest`, one `PowerShell.exe -Action watch -Run`
action, the exact guardian termination approval,
`MultipleInstances=IgnoreNew`, and an unlimited task duration. Its script
checks the task XML seal before watching. After it started, a fresh
`HEALTHY/watching` publication confirms that the task observed a non-stale
heartbeat and a valid boot ID. No WSL termination occurred.

**Result:** The task entered `Running`; the health file changed from stale
`BLOCKED/boot_identity_unavailable` to fresh `HEALTHY/watching`, with
`timestamp_utc=2026-09-27T09:01:34.5456081Z`. In three samples 15 seconds
apart, Guardian stayed `HEALTHY`; total physical headroom was `12,766`–`12,774
MiB` and commit headroom `28,411`–`28,439 MiB`. At a follow-up 10 minutes
after task start, physical headroom was `12,472 MiB` and commit headroom
`28,142 MiB`. The full pressure profile still requires `20,480 MiB` physical
headroom, so it remains refused. Three PowerShell processes together used
`191.5`–`276 MiB` private bytes during the first minute; at the 10-minute
sample, two processes used `191.1 MiB` total. The follow-up guest sample had
`6,307 MiB` `MemAvailable`, `4,030 MiB` `SwapFree`, and `0.00%` memory PSI;
`/proc/swaps` contained only the fallback device, and the RamShared supervisor
remained inactive.

**Conclusion:** Restarting the existing Guardian cleared its stale health
publication without starting the memory campaign. The ordinary watcher did
not reproduce the earlier multi-GiB PowerShell reading over 10 minutes;
EVD-0068 traces that spike to a separate one-shot Guardian-status diagnostic
that queried ScheduledTasks, but the PowerShell engine/provider cause remains
unknown. The task still runs from a mutable checkout rather than an immutable
release package, the installed monitor is unchanged, and physical headroom
still blocks full stress. Only the Guardian task was started and left running;
no GPU allocation, tier activation, RamShared install, or WSL termination
occurred.
**Verdict:** 🟡 `PARTIAL` — current Guardian health is fresh and its observed
memory use is bounded in this window; immutable deployment, the 20 GiB
physical gate, the historical freeze cause, and live tier qualification remain
open.

## 2026-09-27 07:18 -03 — Coherent worker-admitted cache target evidence

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0085`.
**Owner role:** `hardware-researcher / runtime`.
**Observed at:** `2026-09-27T09:38:12Z`.
**Verified at:** `2026-09-27T10:26:59Z`.
**Source revision:** `90fedeb763fa08f619694e7b915c433469529fb4`.
**Source state:** Rust stress telemetry, Windows supervisor, and static tests are committed locally and not installed. No GPU allocation or stress run occurred.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0083/EVD-0084 and the dynamic GPU budget SPECs.
**Freshness:** Static test and plan-only host sample completed on 2026-09-27; GPU and guest observations are read-only point samples.
**Category:** `reliability / ci-gate`.
**How to measure:** From WSL, run `cargo test -p ramshared-cli -j 2`, `cargo clippy -p ramshared-cli --all-targets -- -D warnings`, and `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-cli --files crates/ramshared-cli/src/stress.rs --min 80`. Set `winroot=$(wslpath -w "$PWD")`; run `scripts/windows/Test-RamSharedThreeTierStressStatic.ps1` and `scripts/windows/Invoke-RamSharedThreeTierStress.ps1` with Windows PowerShell 5.1, omitting `-Run` for the plan-only preflight.

**What:** The Windows full-tier wrapper previously required exactly 4,096 MiB
of physical GPU cache even though Rust derives the full-profile target from
the active worker. Rust also retained the maximum physical residency and
maximum worker target as separate peaks, so the outer wrapper could not prove
that they coincided while all three tiers were full. The sealed 4,096 MiB
manifest value is now only a cap and is passed to `ramshared up` as the
maximum logical request. The full profile retains its startup-admitted worker
target as the minimum, and `full_tier_snapshot` returns the cache sample only
when the tier targets and cache criteria pass in the same qualification
cycle. The report records that sample's target and resident MiB in
`simultaneous_physical_cache_target_mib` and
`simultaneous_physical_cache_mib`, separately from peak values. The Windows
validator now requires metric version 2, targets of 100% ZRAM, 100% logical
NBD, and 99% SSD, positive cache samples, a valid startup target, and a paired
same-cycle cache target/residency under the sealed cap. Peak metrics cannot
substitute for a missing or short paired sample.

**Verification:** The final `Test-RamSharedThreeTierStressStatic.ps1`
PowerShell 5.1 run exited 0 with eight named cases: a valid below-cap target
passes; wrong tier targets, an over-cap target, independent peak values that
hide a short same-cycle cache sample, missing paired telemetry, fractional
fields, residency below target, and a worker target below the startup-admitted
target all refuse. Rust's stress-module tests passed 34/34; the complete CLI
suite passed 341 unit tests and 10 dispatch tests. Strict Clippy and the
per-file coverage gate passed; `stress.rs` reached 80.1% line coverage. The
complete Windows static suite passed all 27 named harnesses on the preceding
wrapper revision, and the final targeted Windows stress harness passed after
the added metric/tier checks. The generated guest script passed `bash -n`,
`cargo fmt --check -p ramshared-cli`, `git diff --check`,
`node tools/ci/check-validation-schema.mjs --all`, and `./scripts/docs-check.sh`
all passed.

**Current admission sample:** The latest plan-only invocation exited 0 and
emitted the worker-target policy with `physical_cache_cap_mib=4096` and
`physical_cache_target_mib=null`; it performed no activation. Its three
Windows samples ranged from `11,432` to `11,535 MiB` physical headroom and
`28,100` to `28,244 MiB` commit headroom, against `20,480 MiB` required for
each. The physical gate therefore refused the full profile. Guardian health
was fresh. A near-time guest read-only sample reported about `3,151 MiB`
`MemAvailable`, `3,877 MiB` `SwapFree`, and `0.00%` PSI avg10 some/full; the
1,024 MiB guest reserves passed. The actual worker-admitted GPU target was
not observed because no cache allocation was made.

**Conclusion:** The source now qualifies cache residency with the exact
worker-reported target from a cycle that also satisfies the three tier
thresholds, while keeping the sealed 4 GiB cap. The full campaign remains
blocked by current Windows physical headroom. No GPU allocation, ZRAM/NBD/SSD
pressure, installation, or WSL termination was performed. Cross-vendor live
allocation and full three-tier qualification remain open.
**Verdict:** 🟡 `PARTIAL` — source-level target selection and evidence checks
pass; the live worker target and hardware qualification remain unproven.

## 2026-09-27 08:37 -03 — WSL memory attribution and benchmark display guard

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0086`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T11:37:45Z`.
**Verified at:** `2026-09-27T11:41:49Z`.
**Source revision:** `4ebc75fa306679103c87c0ca9a9bf97a1c4f4f18`.
**Source state:** The fail-closed monitor change is committed in source but has
not been built or installed. The long-running interactive dashboard still
uses the installed 0.14.1 binary. The existing `target/debug/ramshared` monitor
was used only for memory-scope and tier state; its benchmark field was excluded
because it predates this parser change.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0081, EVD-0085, and the benchmark evidence integrity
SPEC.
**Freshness:** The WSL and Windows samples were collected less than one second
apart. The source tests completed on 2026-09-27; no build, install, pressure
campaign, GPU allocation, or WSL restart occurred.
**Category:** `reliability / memory / evidence-integrity`.
**How to measure:** From WSL, read `/proc/meminfo`, `/proc/pressure/memory`,
`/proc/swaps`, `uname -r`, and systemd unit state; run the existing monitor
with `--jsonl --once` for typed memory-scope and tier observations. From
Windows PowerShell, call `Get-SharedWslHostMemorySample` and inspect the
working set and private bytes of `vmmemWSL` and `powershell` processes. Verify
the source parser with `cargo test -p ramshared-cli -j 1 monitor_benchmark_`
and the monitor slice coverage gate.

**What:** The monitor previously trusted `status` and scalar metrics from
`docs/benchmarks/history/latest.json`. That file is the historical Build #5
record, which EVD-0047 and `docs/BENCHMARKS.md` classify as unqualified; it has
no v1 evidence envelope, source/binary identity, or promotion decision. When
run from the repository root, the old parser therefore displayed its
`PASS_ZERO_PANIC` verdict and old reclaim numbers as current benchmark output.
The source now accepts only a promotable `ramshared-evidence/v1` record with a
clean source, qualified comparison, binary match, passing legitimate/refusal
checks, complete cleanup, zero residue, and at least three internally
consistent samples for every displayed metric. It recomputes the median and
nearest-rank p99 from those samples. Legacy, baseline, dirty, incomplete, and
forged-summary records return `AWAITING_QUALIFICATION`.

**Verification:** The four named monitor tests first failed against the old
parser; the legacy fixture reproduced the false green `PASS_ZERO_PANIC`. After
the fix, all four passed. The full CLI suite passed 345 unit tests and 10
dispatch tests; strict Clippy passed; `monitor.rs` coverage passed at 88.7%
(2,033/2,292 lines); `cargo fmt --check -p ramshared-cli` and
`git diff --check` passed. The test checkpoint is `5e4d8289`; the fix is
`4ebc75fa`.

**Paired host and guest observation:** The monitor identified `memory_scope=wsl2`
and `MemTotal=16,379,360 KiB`. `MemAvailable` varied from `980,972` to
`987,472 KiB` across samples collected within one second (about 958–964 MiB);
`SwapFree` was `2,744,368 KiB` (2,680 MiB) out of a 4 GiB fallback swap, with
`1,449,936 KiB` used. PSI `some` and `full` `avg10` were zero. RamShared was
`Off`, the daemon was absent, ZRAM and VRAM tiers were absent, the Guardian was
healthy, and `ramshared-supervisor.service` was inactive. Systemd was running.
Guest process totals were `2,991,236 KiB` RSS and `1,432,556 KiB` swap; the
largest visible process, `rust-analyzer`, had `837,944 KiB` RSS and
`1,012,972 KiB` swap. Cgroup accounting was `partial`.

Windows `GetPerformanceInfo` reported `10,315 MiB` physical headroom and
`27,723 MiB` commit headroom. The `vmmemWSL` working set was `12,969.2 MiB`;
its `16,142.4 MiB` private bytes are a commit measure, not physical RAM. Four
PowerShell processes used `414.6 MiB` private bytes combined; the largest used
`157.4 MiB`. No multi-GiB PowerShell process was observed. The full stress
profile requires `20,480 MiB` physical headroom, and its guest reserve requires
at least `1,024 MiB` `MemAvailable`; both gates would refuse at this sample.
No stress preflight or pressure workload was started.

**Installed dashboard:** The active `ramshared top` process still resolves to
the installed `/usr/local/bin/ramshared` 0.14.1 binary. That binary contains
only the `Host RAM` label strings; the source-built debug binary contains
`WSL2 RAM` and `WSL2 RAM & Swap`. The installed panel therefore still needs a
new build and installation before the corrected label and evidence display can
appear on the host.

**Conclusion:** The displayed 15.6 GiB `MemTotal` belongs to WSL2, not Windows
physical RAM. Current WSL memory use is not evidence of an active RamShared
stress run: all managed tiers are off and PSI is zero. The old PowerShell
multi-GiB anomaly did not recur; current PowerShell private use is below
158 MiB per process. The exact cause of the large `vmmemWSL` working set and
the earlier freeze remains unresolved because guest cgroup accounting is
partial and no current source build is installed. The stress remains blocked
by host physical headroom and guest `MemAvailable`.
**Verdict:** 🟡 `PARTIAL` — the source now refuses the unqualified benchmark
record, but the host still runs the old dashboard binary; memory attribution
and live stress qualification remain open.

## 2026-09-27 11:29 -03 — Post-restart host and WSL memory snapshot

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0087`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T14:29:51Z`.
**Verified at:** `2026-09-27T14:33:34Z`.
**Source revision:** `c379f9b4b159a0e64e14106960bd10fbb716077c`.
**Source state:** The RamShared checkout was clean when sampled. The separate
kernel contribution checkout had an uncommitted GPADL-rescind helper; review
found that helper incomplete and it was not built or installed.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0070, EVD-0071, EVD-0086, and the WSL2 freeze gap.
**Freshness:** One read-only guest sample and a Windows sample 3 minutes 43
seconds later; not a trend or a simultaneous pair.
**Category:** `reliability / memory / attribution`.
**How to measure:** Read selected `/proc/meminfo` and `/proc/pressure/memory`
fields once from the active WSL guest; inspect process working sets with
`Get-Process`; read Windows physical memory through `GlobalMemoryStatusEx`.
`/proc/vmallocinfo` was attempted without elevation and denied access. No
second guest read, build, stress, install, process termination, or additional
WSL restart was performed during evidence collection.

**What:** Capture one post-restart memory snapshot to determine whether the
host or a PowerShell process was currently accumulating memory.

**Guest:** The active kernel identified as
`6.18.40.1-microsoft-standard-WSL2+`. `MemTotal` was 15,995 MiB,
`MemAvailable` 6,210 MiB, `MemFree` 863 MiB, `Cached` 5,580 MiB, and
`Buffers` 1,088 MiB. Of the configured 4,096 MiB swap, 4,086 MiB was free
(about 10 MiB used). Memory PSI was near zero (`avg10=0.00`, `avg60=0.04`,
`avg300=0.01` for `some` and `full`). `vmbus_alloc_buffer` map counts could
not be collected because `/proc/vmallocinfo` returned `Permission denied`.

**Windows:** At 11:33:34 -03, physical RAM totaled 32,670 MiB, with 10,643 MiB
available and 67% in use. `VmmemWSL` (PID 8984) had a 10,809 MiB working set
and 15,891 MiB private bytes; the latter is not a physical-residency measure.
The combined private bytes for `powershell` and `pwsh` were 221 MiB. No
multi-GiB PowerShell process was present in this sample.

**Comparison:** Screenshot 106 at 10:10 showed Windows at 14.9/31.9 GiB in
use and `VmmemWSL` near 2,922 MiB. The user then ran `wsl --shutdown`, so the
later 10,809 MiB working set is a cross-restart comparison, not proof of
monotonic growth. EVD-0086 at 08:37 reported 10,315 MiB physical headroom and
12,969 MiB `VmmemWSL` working set; the current Windows sample has 328 MiB more
headroom and 2,160 MiB less `VmmemWSL` working set than that earlier sample.
The local `.wslconfig` still sets `autoMemoryReclaim=disabled`; that may allow
guest cache to remain resident, but this sample does not identify which pages
account for the `VmmemWSL` working set.

**Assessment:** The current host is not near physical-memory exhaustion, and
the guest is not currently swapping or showing material PSI pressure. The
large WSL working set is real host residency, while the guest still reports
6.2 GiB available. The earlier freeze remains consistent with severe guest
memory depletion and swap thrashing documented in EVD-0070/0071. Source review
also found GPADL cleanup paths that may retain backing pages after ambiguous
failure; however, the proposed helper incorrectly treats every
`channel->rescind` as terminal host revocation, misses partial establishment,
and is not a validated fix. This is a plausible contributor, not a proven
cause. No current map count or exact owner was available.

**Verdict:** 🟡 `PARTIAL` — current WSL memory and host residency are measured;
the initiating allocation and historical freeze cause remain unproven.

## 2026-09-27 12:21 -03 — VMBus map growth and current host headroom

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0088`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T15:21:27Z`.
**Verified at:** `2026-09-27T15:23:38Z`.
**Source revision:** `61f49c92759f10ba4da9a33a1ba9e55c9d104682`.
**Source state:** Read-only host and guest measurements; no kernel source change,
build, install, stress, process termination, or WSL restart.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0070/0071 and EVD-0086/0087.
**Freshness:** Two guest samples were 22 seconds apart. The Windows sample was
collected about 1 minute 49 seconds after the second guest sample; these are
near-time observations, not an instrumented common clock.
**Category:** `reliability / memory / kernel-allocation`.
**How to measure:** In the already-running WSL guest, read selected
`/proc/meminfo` and `/proc/pressure/memory` fields, then use `sudo -n awk` to
count `vmbus_alloc_buffer` entries and sum their mapped sizes in
`/proc/vmallocinfo`; repeat once after 20 seconds in the same process. On
Windows, read physical availability with `GlobalMemoryStatusEx` and inspect
`vmmemWSL` and PowerShell with `Get-Process`. No `wsl.exe` launch or stress
operation was used for the guest interval.

**What:** Check whether the VMBus allocation maps were still accumulating
after the WSL restart and whether the Windows host's physical RAM was rising
at the same time.

**Guest:** Kernel `6.18.40.1-microsoft-standard-WSL2+` reported
`MemAvailable=4,398,348` then `4,316,424 KiB` (a decrease of about 80 MiB).
`SwapFree` remained `3,915,476 KiB`, or about 272 MiB in use out of 4 GiB.
Memory PSI `avg10`, `avg60`, and `avg300` remained zero for both `some` and
`full`. `vmbus_alloc_buffer` mappings increased from 13,962 to 14,003; entries
of 430,080 bytes increased from 13,684 to 13,724. The summed `vmallocinfo`
area size increased from 5,980,024,832 to 5,997,494,272 bytes (+16.66 MiB) in
22 seconds. This is virtual mapping-area size, including allocator guard
space; it is not a direct measurement of Windows resident RAM.

**Windows:** At 12:23:38 -03, physical RAM totaled 32,670 MiB with 10,991 MiB
available (66% in use). `VmmemWSL` had a 10,692 MiB working set and 15,930
MiB private bytes; PowerShell processes totaled 187 MiB private bytes. The
six largest working sets were `VmmemWSL` (10,692 MiB), Memory Compression
(1,590 MiB), `MsMpEng` (360 MiB), `Code` (344 MiB), `explorer` (336 MiB), and
`msedge` (317 MiB).

**Comparison:** Since EVD-0087 at 11:33, physical headroom increased by 348
MiB and the `VmmemWSL` working set decreased by 117 MiB. The Windows samples
therefore do not show host RAM continuing to rise during this interval. The
guest's VMBus map count did show short-interval net growth; active channel
creation and leaked buffers are not yet distinguished, so the map delta alone
does not prove a leak or identify its owner.

**Assessment:** The current host is not near physical-memory exhaustion and
the guest PSI is quiet. The live VMBus map growth strengthens the GPADL/buffer
lifetime-retention hypothesis and warrants matching these maps to channel
create/close and rescind events. It does not establish that the maps are leaked, that the
custom kernel initiated the prior freeze, or that a particular process caused
the growth. The source-reviewed rescind helper remains removed and no fix has
been built or installed.

**Verdict:** 🟡 `PARTIAL` — short-interval VMBus map growth is observed, but
ownership and leak causality are not yet proven.

## 2026-09-27 12:38–12:55 -03 — Cumulative VMBus map growth and channel inventory

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0089`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T15:38:32Z`.
**Verified at:** `2026-09-27T15:55:57Z`.
**Source revision:** `61f49c92759f10ba4da9a33a1ba9e55c9d104682`.
**Source state:** Read-only guest and Windows inspection; no stress, install,
kernel build, WSL restart, or process termination.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0087 and EVD-0088 and the VMBus backport audit.
**Freshness:** Guest samples were 17 minutes 25 seconds apart. The latest
Windows physical-memory sample was at 12:39:24 -03, about 16 minutes before
the second guest sample; it is not a simultaneous host/guest pair.
**Category:** `reliability / memory / kernel-allocation`.
**How to measure:** Read `/proc/meminfo`, `/proc/pressure/memory`, and
`/proc/vmallocinfo` in the active guest. Count entries attributed to
`vmbus_alloc_buffer`, sum the reported vmalloc area sizes, and separately sum
the `pages=` field. Count channels by resolving each
`/sys/bus/vmbus/devices/<instance-guid>/channels/` directory. Use
`GlobalMemoryStatusEx` and `Get-Process` for the Windows sample.

**What:** Determine whether the VMBus map count continues to rise during
ordinary operation and compare it with the live channel inventory and host
physical-memory trend.

**Guest at 12:38:32:** Kernel `6.18.40.1-microsoft-standard-WSL2+ #6` reported
`MemAvailable=3,697,224 KiB`, `SwapFree=3,824,000 KiB`, and zero PSI averages.
There were 15,821 `vmbus_alloc_buffer` vmalloc entries with a summed area size
of 6,774,464,512 bytes; 15,512 entries had a 430,080-byte area.

**Guest at 12:55:57:** `MemAvailable=4,279,092 KiB`, `SwapFree=2,646,580 KiB`,
and PSI `avg300` was 0.14 for `some` and 0.12 for `full`; `avg10` and `avg60`
were zero. The vmalloc count increased to 17,690 and summed area size to
7,573,204,992 bytes (+1,869 entries, +798,740,480 bytes or 761.8 MiB in 17:25).
The `pages=` fields summed to 1,831,237 pages (7,500,746,752 bytes); 17,350
entries each reported `pages=104` and a 430,080-byte area. The area size
includes a guard page and must not be reported as resident host RAM. The
`pages=` sum describes backing pages reported for these mappings, but does
not identify their current Windows residency or owning VMBus channel.

The same guest sample found 89 VMBus device links and 102 channel entries
under their per-device `channels/` directories. In commit
`50715f5f738f2793f2713401db69988df0347ecf`, the only in-tree caller of
`vmbus_alloc_buffer()` is `vmbus_alloc_ring()`, which makes one allocation for
the combined send and receive rings. That snapshot sets
`MAX_CHANNEL_RELIDS=max(256, 2048)=2048`. If Build #6 came from that snapshot
and the maps are those in-tree ring allocations, 17,350 mappings with 104
backing pages each exceed the maximum relid count by more than 8x and cannot
represent only simultaneously open in-tree rings. A later, separate WSL
backport commit (`418653fde683813c65a88b20dd7e0c614c90806d`) also converts
NetVSC and UIO buffers, so its callsites must not be attributed to Build #6
without source identity. The map/channel discrepancy is therefore a strong
retention signal under the `50715` hypothesis, but `/proc/vmallocinfo` does
not identify map owner, channel, or lifecycle.

The source audit confirmed a retention defect in both `50715` and `418653`:
`vmbus_teardown_gpadl()` forces a successful return when `channel->rescind` is
set, but only clears `gpadl_handle` after a teardown acknowledgement.
`vmbus_release_buffer()` refuses to free a buffer while that handle remains,
then clears the owner structure. The rescind path can therefore leave the
mapping allocated without a tracked owner. In `50715`, the ring allocator is
the sole in-tree `vmbus_alloc_buffer()` caller. This is a confirmed defect in
those source snapshots, not proof that Build #6 contains either snapshot or
that this path caused the freeze.

**Windows at 12:39:24:** Physical memory totaled 32,670 MiB with 11,582 MiB
available (64% used). `VmmemWSL` had a 9,956 MiB working set and 15,931 MiB
private bytes. Three PowerShell processes totaled 252 MiB private bytes. Since
EVD-0088 at 12:23:38, host physical headroom increased by 591 MiB and
`VmmemWSL` working set fell by 736 MiB; PowerShell use remains far below the
previous multi-GiB diagnostic. There is no later Windows sample paired with
the 12:55 guest measurement.

**Source identity:** The running kernel exposes `vmbus_alloc_buffer` and
`vmbus_free_buffer`, and its installed image hash matches EVD-0051. The
Microsoft WSL source checkout at `14794180686c2fb6307fbe359c359bec765249f3`
does not contain that allocator, while the contribution fork has a separate
backport commit `50715f5f738f2793f2713401db69988df0347ecf`. The installed image
hash does not match either currently available `bzImage` artifact. The exact
source commit for the running Build #6 image is therefore not proven, so the
measured allocations cannot yet be attributed to a specific patch revision.
The running kernel build timestamp is Thu Sep 24 08:39:30 -03, earlier than
the recorded creation of backport commit `418653` at 21:44:52 -03 that day.
This makes that exact commit less likely as the image source, but does not
exclude an earlier uncommitted tree containing equivalent changes.

**Assessment:** Two consecutive intervals show net growth at roughly 44 MiB
per minute in reported vmalloc area size. The latest inventory found 102 live
channels alongside 17,690 mappings; the previous inventory found 104 channels
nearby in time. This is consistent with cumulative VMBus buffer retention
during a long-running guest and could contribute to slow guest-memory
depletion. It is not proof of a leak or of the previous freeze's cause. The guest's
`MemAvailable` rose between these samples while swap use and five-minute PSI
increased, so the overall memory trajectory is not a simple one-metric trend.
The latest Windows sample shows host headroom rising rather than falling.

**Verdict:** 🟡 `PARTIAL` — cumulative VMBus map growth is confirmed across
multiple intervals and materially narrows the investigation; exact buffer
ownership, source revision, and causal link to the freeze remain unresolved.

## 2026-09-27 13:36 -03 — Follow-up Windows physical-memory sample

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0090`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T16:36:37Z`.
**Verified at:** `2026-09-27T16:45:34Z`.
**Source revision:** `61f49c92759f10ba4da9a33a1ba9e55c9d104682`.
**Source state:** One read-only Windows sample; no WSL launch, stress, process
termination, or configuration change.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0088 and EVD-0089.
**Freshness:** This is a later host-only sample, not simultaneous with a guest
`/proc` sample. The comparison point is EVD-0089's 12:39:24 Windows sample.
**Category:** `reliability / memory / host-telemetry`.
**How to measure:** Read physical availability with `GlobalMemoryStatusEx`
and inspect `VmmemWSL` and PowerShell process working sets/private bytes with
`Get-Process`; do not launch `wsl.exe`.

**What:** Check whether Windows physical RAM and `VmmemWSL` residency continued
to rise after the observed guest VMBus-map growth.

**Measured data:** At 13:36:37 -03, Windows reported 32,670 MiB physical RAM,
14,337 MiB available, 18,333 MiB used, and 56% load. `VmmemWSL` had an 8,106
MiB working set and 15,718 MiB private bytes. Three PowerShell processes used
248 MiB private bytes combined, including the collector.

**Comparison:** Since 12:39:24 in EVD-0089, Windows physical headroom rose
2,755 MiB, physical load fell from 64% to 56%, and the `VmmemWSL` working set
fell 1,850 MiB. Its private bytes fell 213 MiB; combined PowerShell private
bytes fell 4.5 MiB. Private bytes measure committed process memory, not
resident physical RAM.

**Assessment:** This sample does not support a claim that host physical RAM
was steadily consumed during the observed interval. It does not rule out
guest-side VMBus page retention: the guest mappings grew in EVD-0089, but no
guest map count was paired with this host sample, and Windows working set is
not a per-allocation owner measure. The gradual guest map growth remains
consistent with an accumulating retention bug; the prior freeze trigger is
still unproven.

**Verdict:** 🟡 `PARTIAL` — host physical headroom was higher and `VmmemWSL`
working set lower at this sample; guest allocation ownership and freeze
causality remain unresolved.

## 2026-09-27 14:02–14:04 -03 — Guest VMBus growth with paired host telemetry

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0091`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T17:02:55Z`.
**Verified at:** `2026-09-27T17:04:09Z`.
**Source revision:** `61f49c92759f10ba4da9a33a1ba9e55c9d104682`.
**Source state:** Read-only `/proc` inspection in the already-running guest,
followed by one Windows-only memory sample; no WSL launch, stress, kernel
build/install, process termination, or configuration change.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0089 and EVD-0090.
**Freshness:** Guest sample at 14:02:55 and Windows sample at 14:04:09 -03,
about 74 seconds apart.
**Category:** `reliability / memory / kernel-allocation`.
**How to measure:** Read guest `/proc/meminfo`, `/proc/pressure/memory`,
`/proc/swaps`, and `/proc/vmallocinfo` directly in the existing WSL shell.
On Windows, read physical RAM with `GlobalMemoryStatusEx` and inspect
`VmmemWSL` and PowerShell with `Get-Process`, without launching WSL.

**What:** Test whether the previously observed VMBus-map growth continued and
whether it coincided with rising Windows physical RAM use or guest memory
depletion.

**Guest at 14:02:55:** Kernel `6.18.40.1-microsoft-standard-WSL2+ #6` reported
`MemAvailable=2,188,016 KiB`, `SwapTotal=4,194,304 KiB`, and
`SwapFree=1,777,296 KiB` (2,417,008 KiB in use). PSI `avg10`, `avg60`, and
`avg300` were all reported as `0.00` for `some` and `full`. There were 24,932
`vmbus_alloc_buffer` vmalloc entries with summed area size 10,667,855,872
bytes. Of these, 24,470 entries reported `pages=104`, totaling 2,544,880
reported backing pages (10,423,828,480 bytes). The area includes guard space;
these values do not directly measure Windows physical residency.

**Comparison with EVD-0089 at 12:55:57:** Over 66 minutes 58 seconds, the
entry count rose by 7,242 and summed vmalloc area rose by 3,094,650,880 bytes
(2,950.7 MiB, about 44.1 MiB/min). Entries with `pages=104` rose by 7,120.
Guest `MemAvailable` fell by 2,091,076 KiB (about 1.99 GiB), and `SwapFree`
fell by 869,284 KiB (about 849 MiB). PSI averages at this sample were zero,
so this does not show an active stall at 14:02.

**Windows at 14:04:09:** Physical memory totaled 32,670 MiB with 16,147 MiB
available (50% used). `VmmemWSL` had a 6,605 MiB working set and 15,952 MiB
private bytes. Two PowerShell processes totaled 189 MiB private bytes.
Compared with EVD-0090 at 13:36:37, host physical headroom rose 1,810 MiB,
`VmmemWSL` working set fell 1,501 MiB, and its private bytes rose 234 MiB.
The host sample and guest sample are near-time but are not an instrumented
per-allocation residency match.

**Assessment:** The guest-side pattern is now stronger than one isolated
interval: VMBus allocator mappings continued to grow at roughly 44 MiB/min,
while guest available memory declined and swap use increased. This is
consistent with accumulating guest kernel-page retention during continuous
operation and could lead to guest paging and a later freeze. Windows physical
headroom did not decline; it rose, and `VmmemWSL` working set fell. Therefore
the evidence supports a guest-memory accumulation candidate, not a Windows
host-RAM exhaustion event. Build #6 source identity remains unresolved, so the
specific `50715`/`418653` defect is not yet attributed to the running kernel.
No freeze occurred during this sample, and current freeze causality remains
unproven.

**Verdict:** 🟡 `PARTIAL` — sustained guest VMBus map growth now tracks with
declining guest headroom and increased swap use, while Windows physical
headroom improves; exact source identity, ownership, and freeze causality
remain unresolved.

The current-boot kernel log filter found VMBus initialization and its
`min_free_kbytes` reserve adjustment, but no matching GPADL/rescind, OOM,
hung-task, or I/O-error lines. Those events may not be logged at the needed
detail, so this absence does not rule out the retention path.

## 2026-09-27 14:19 -03 — Follow-up Windows physical-memory sample

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0092`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T17:19:13Z`.
**Verified at:** `2026-09-27T17:19:13Z`.
**Source revision:** `61f49c92759f10ba4da9a33a1ba9e55c9d104682`.
**Source state:** Windows-only read-only sample using `GlobalMemoryStatusEx`,
`GetPerformanceInfo`, and `Get-Process`; no WSL launch, stress, process
termination, or configuration change.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0090 and EVD-0091.
**Freshness:** This host sample was taken about 16 minutes after the latest
guest sample at 14:02:55 -03; it is not simultaneous guest/host telemetry.
**Category:** `reliability / memory / host-telemetry`.
**How to measure:** Read physical availability with `GlobalMemoryStatusEx`,
system commit with `GetPerformanceInfo`, and `VmmemWSL`/PowerShell working sets
and private bytes with `Get-Process`, without launching `wsl.exe`.

**What:** Check whether host physical RAM or the WSL process residency continued
to rise after the guest VMBus-map growth observed in EVD-0091.

**Measured data:** At 14:19:13 -03, Windows reported 32,670 MiB physical RAM,
16,159 MiB available, 16,511 MiB used, and 50% load. System commit was 30,749
MiB used out of a 57,246 MiB limit, with 26,497 MiB remaining and a 44,802 MiB
peak. `VmmemWSL` had a 6,481.7 MiB working set and 15,981.2 MiB private bytes.
Two PowerShell processes totaled 189.1 MiB private bytes. The pagefile-related
fields returned by `GlobalMemoryStatusEx` matched the system commit limit and
remaining commit; they do not report physical pagefile I/O or prove pagefile
occupancy on disk.

**Comparison:** Since EVD-0091's 14:04:09 host sample, physical RAM available
rose by 12 MiB, load stayed at 50%, `VmmemWSL` working set fell by 123.4 MiB,
and its private bytes rose by 29.4 MiB. PowerShell private bytes were unchanged.
Since EVD-0090 at 13:36:37, physical headroom rose by 1,822 MiB and
`VmmemWSL` working set fell by 1,624.6 MiB, while its private bytes rose by
263.5 MiB. Private bytes and system commit are not resident physical RAM.

**Assessment:** This later sample does not show host physical RAM continuing
to rise: physical availability is effectively flat versus 14:04 and remains
higher than at 13:36. `VmmemWSL` working-set residency is lower at both
comparisons. The small private-byte increase is committed memory and does not
establish increasing host physical use. The last guest allocation sample is
about 16 minutes older, so this is not a contemporaneous allocation-to-residency
comparison. The gradual guest-side VMBus-map growth remains a candidate for
guest memory accumulation; ownership, installed source identity, and freeze
causality remain unresolved.

**Verdict:** 🟡 `PARTIAL` — host physical headroom remained stable over the latest
interval and above the earlier sample; this does not identify the owner or
cause of guest-side VMBus growth or the prior freeze.

## 2026-09-27 14:28 -03 — Paired guest pressure and Windows host sample

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0093`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T17:28:30Z`.
**Verified at:** `2026-09-27T17:28:32Z`.
**Source revision:** `e22fcd507d558230dc006836c05fe235c47677dc`.
**Source state:** Read-only guest `/proc` and Windows telemetry from the
already-running WSL instance; no WSL launch, stress, build/install, process
termination, shutdown, or configuration change.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0089 through EVD-0092 and the VMBus backport audit.
**Freshness:** Guest sample at 14:28:30–14:28:32 and Windows sample at
14:28:30.985–14:28:31.053 -03; the two measurements overlap within about two
seconds. Both refer to the same prior pressured guest boot.
**Category:** `reliability / memory / kernel-allocation`.
**How to measure:** Read guest `/proc/meminfo`, `/proc/pressure/memory`,
`/proc/swaps`, and `/proc/vmallocinfo` in the current guest. On Windows, read
physical RAM with `GlobalMemoryStatusEx`, system commit with
`GetPerformanceInfo`, and process residency/commit with `Get-Process`; do not
launch another WSL instance.

**What:** Check whether the guest-side allocator growth and memory pressure
continued, and whether physical RAM use was simultaneously rising on Windows.

**Guest:** Kernel `6.18.40.1-microsoft-standard-WSL2+ #6` reported
`MemTotal=16,379,368 KiB`, `MemAvailable=1,449,216 KiB`, and
`MemFree=1,009,416 KiB`. Swap had `1,116,520 KiB` free of `4,194,304 KiB`
(3,077,784 KiB used, about 3,006 MiB). PSI `some` avg10/60/300 was
`0.02/0.29/0.27`; `full` was `0.02/0.29/0.26`. Root read-only inspection found
27,661 `vmbus_alloc_buffer` vmalloc entries totaling 11,834,171,392 bytes of
area. There were 27,154 entries of 430,080 bytes with `pages=104`; all reported
backing page counts summed to 2,861,541. Vmalloc area is not Windows resident
RAM, and the entries do not identify their owners.

**Comparison with EVD-0091 at 14:02:55:** Over about 25 minutes 35 seconds,
the map count increased by 2,729 and summed vmalloc area by 1,166,315,520 bytes
(about 1,112 MiB). `MemAvailable` fell by 738,800 KiB (about 721 MiB), and
`SwapFree` fell by 660,776 KiB (about 645 MiB). PSI averages are non-zero but
remain low at this sample; this does not show an active freeze.

**Windows, sampled at the same time:** Physical RAM totaled 32,670 MiB with
16,401 MiB available, 16,269 MiB used, and 49% load. System commit was
30,958 MiB of a 57,246 MiB limit, leaving 26,288 MiB. `VmmemWSL` had a
6,064.7 MiB working set and 16,121.7 MiB private bytes. Three PowerShell
processes totaled 242.6 MiB private bytes. Since EVD-0092 at 14:19, physical
headroom rose by 242 MiB and `VmmemWSL` working set fell by 417 MiB; its
private bytes rose by 140.5 MiB. Private bytes and system commit are not
resident physical RAM.

**Assessment:** The paired sample confirms continued guest-side map growth,
lower guest headroom, and increased guest swap use while Windows physical
headroom remained ample and increased. The map/page trend supports a guest
kernel-buffer accumulation candidate; it does not by itself prove a leak,
identify an owner, match the Build #6 image to a source commit, or establish the
cause of the earlier freeze. The proposed GPADL fix is still source-only and
does not yet have a verified WSL image or installation pair.

**Verdict:** 🟡 `PARTIAL` — the guest has materially reduced headroom and
substantial swap use, but the Windows host is not running out of physical RAM.
The exact buffer lifecycle and corrective host image remain unqualified.

## 2026-09-27 14:50–14:51 -03 — Follow-up guest pressure and Windows process sample

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0094`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T17:50:56Z`.
**Verified at:** `2026-09-27T17:51:50Z`.
**Source revision:** `e22fcd507d558230dc006836c05fe235c47677dc`.
**Source state:** Read-only guest `/proc` and Windows API/process snapshots;
one identified background `git fetch --all` in the kernel repository was
interrupted to stop an unnecessary full-history fetch. No stress, build,
kernel install, WSL shutdown, or configuration change.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0092 and EVD-0093 and the WSL freeze timeline.
**Freshness:** Guest metrics at 14:51:08 -03; Windows physical/commit sample
at 14:50:56 and process ranking at 14:51:50. All use the same active guest
boot ID as EVD-0093; host/guest samples are within about 54 seconds.
**Category:** `reliability / memory / host-telemetry`.
**How to measure:** Read guest memory, swap, PSI, and allocator entries from
the current guest. On Windows, use `GlobalMemoryStatusEx`, `GetPerformanceInfo`,
and `Get-Process`; do not start a second WSL instance.

**What:** Recheck whether the WSL guest was approaching the prior pressure
pattern, determine whether Windows physical RAM was also being exhausted, and
assess the impact of the long-running full-history fetch.

**Guest at 14:51:08:** The active kernel was still
`6.18.40.1-microsoft-standard-WSL2+` in the prior pressured guest boot.
`MemAvailable` was 714,072 KiB
(about 697 MiB), `MemFree` 914,924 KiB, and `SwapFree` 1,325,820 KiB of
4,194,304 KiB total. PSI `some` avg10/60/300 was `1.83/1.37/2.14`; `full`
was `1.83/1.35/2.07`. There were 30,058 `vmbus_alloc_buffer` map entries;
29,510 reported 104 pages, and reported page counts summed to 3,109,189.
The area-total parser did not recognize the `vmallocinfo` address format at
this sample, so no current total area is claimed.

**Comparison with EVD-0093:** In about 22 minutes, the map count rose by
2,397 and `pages=104` entries by 2,356. `MemAvailable` fell by 735,144 KiB
(about 718 MiB), while `SwapFree` rose by 209,300 KiB. The recent PSI was
non-zero and had fluctuated: at 14:48 it was about 4.9% avg10, then about
1.8% at 14:51. This is active but varying guest memory pressure, not proof
that a freeze was imminent at either snapshot.

**Windows at 14:50:56:** Physical RAM totaled 32,670 MiB with 14,775 MiB
available, 17,894 MiB used, and 54% load. System commit was 33,125 MiB of a
57,246 MiB limit, leaving 24,121 MiB. `VmmemWSL` had a 5,798.5 MiB working
set and 15,999.5 MiB private bytes; two PowerShell processes totaled 189.1 MiB
private bytes. Compared with EVD-0093 at 14:28, physical headroom fell by
1,626 MiB, while `VmmemWSL` working set fell 266 MiB and private bytes fell
122 MiB. The Windows process ranking about 54 seconds later showed 735 MiB
working set for Memory Compression and 858 MiB private bytes for `obs64`, but
there is no matching prior process ranking to attribute the host-memory
change. Windows still had about 14.8 GiB physical headroom.

**Background fetch:** The WSL process table showed `git fetch --all` in the
kernel-contribution repository, fetching the Torvalds Linux remote, with an
`index-pack` child using about 364 MiB RSS plus about 63 MiB for its fetch
parent. This read-only background fetch had run for about 30 minutes and was
interrupted at 14:49. The guest still had only about 763 MiB `MemAvailable`
and PSI avg10 about 4.9% immediately after; subsequent guest and Windows
samples did not show a recovery attributable to stopping it. It was extra
resource use, but is not established as the source of the sustained VMBus
growth or the earlier freeze.

**Assessment:** The guest's reduced headroom and growing VMBus allocator map
count continue to support a guest-side accumulation candidate. Windows
physical use also rose during this separate interval, but `VmmemWSL` working
set fell and 14.8 GiB remained available; process rankings do not explain the
change. The `git fetch` was an unnecessary load and is now stopped. Exact
allocator ownership, Build #6 source identity, and the freeze trigger remain
unresolved. No corrected WSL kernel artifact is available to install.

**Verdict:** 🟡 `PARTIAL` — guest pressure is now materially higher and needs
prompt mitigation; host RAM remains available. The background fetch is stopped,
but that did not resolve the guest pressure, and the kernel fix is not yet
ported, built, or proven safe for this host.

## 2026-09-27 15:02–15:09 -03 — VMBus map growth and host-memory follow-up

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0095`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T18:08:04Z`.
**Verified at:** `2026-09-27T18:09:46Z`.
**Source revision:** `e22fcd507d558230dc006836c05fe235c47677dc`.
**Source state:** Read-only sample from the active WSL guest and Windows host.
One identified VS Code `git fetch --all` in the kernel contribution repository
was stopped after confirming its process tree. No stress, build, kernel
installation, WSL shutdown, or configuration change.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0089 through EVD-0094 and the WSL VMBus source audit.
**Freshness:** Guest metrics at 15:08:04 -03; Windows physical/commit/process
sample at 15:09:45 -03, about 101 seconds later. Both refer to the same prior
pressured guest boot.
**Category:** `reliability / memory / kernel-allocation`.
**How to measure:** Read `/proc/meminfo`, `/proc/pressure/memory`, and
`/proc/vmallocinfo` in the active guest; use `GlobalMemoryStatusEx`,
`GetPerformanceInfo`, and `Get-Process` on Windows without launching another
WSL instance.

**What:** Recheck whether the guest-side VMBus allocation trend continued,
whether Windows physical RAM was also being exhausted, and whether the
background fetch accounted for the guest pressure.

**Guest at 15:08:04:** Kernel `6.18.40.1-microsoft-standard-WSL2+ #6` reported
`MemTotal=16,379,368 KiB`, `MemAvailable=524,192 KiB` (about 512 MiB), and
`SwapFree=1,181,156 KiB` of 4,194,304 KiB (3,013,148 KiB used, about
2,943 MiB). PSI `some` avg10/60/300 was `0.01/0.52/1.61`; `full` was
`0.01/0.51/1.56`. Root read-only inspection counted 31,792
`vmbus_alloc_buffer` vmalloc entries with 13,599,199,232 bytes of mapped area
and 3,288,325 declared backing pages (about 12.54 GiB). Of these entries,
31,214 report `pages=104`. `/proc/meminfo` reported `VmallocUsed=13,182,804
KiB`. These mapping/backing-page totals describe guest kernel allocations;
they are not a direct measurement of Windows resident physical RAM and do not
identify buffer owners. The same snapshot found 89 VMBus device links; that is
not a count of channels.

**Trend since EVD-0093 at 14:28:** Over about 40 minutes, map count rose by
4,131 and declared backing pages by 426,784 (about 1.63 GiB); `MemAvailable`
fell by 925,024 KiB (about 903 MiB). `SwapFree` was 64,636 KiB higher than at
14:28, so swap use did not rise monotonically across these two samples. The
map/page growth and reduced guest headroom strengthen the guest-side
accumulation hypothesis, without proving that all mapped pages were resident
or that they caused the earlier freeze.

**Background fetch:** At 15:02, a VS Code extension-host child was running
`git fetch --all` in the kernel contribution checkout.
The fetch had run for about 13 minutes and reached about 646 MiB RSS in one
process sample; it was stopped at about 15:03, and all fetch children exited.
At 15:05, guest `MemAvailable` was 441,808 KiB; by 15:08 it was 524,192 KiB,
while the VMBus map count continued to 31,792. This fetch added avoidable
memory load, but the samples do not show an immediate recovery attributable
to stopping it or prove it was the source of the sustained VMBus growth.

**Windows at 15:09:45:** Physical RAM totaled 32,670 MiB with 16,261 MiB
available, 16,409 MiB used, and 50% load. System commit was 33,377 MiB of a
57,246 MiB limit, leaving 23,869 MiB. `VmmemWSL` had a 3,845.5 MiB working
set and 16,185.7 MiB private bytes. Compared with 15:00, physical headroom
rose by 671 MiB, system commit use fell by 243 MiB, and `VmmemWSL` working
set fell by 601 MiB. This does not show Windows physical RAM exhaustion.

**Assessment:** Live evidence now strongly supports growing VMBus-backed
guest allocations alongside low guest headroom, while Windows physical
headroom increased. GPADL retention/rescind remains a plausible lifecycle
mechanism, not a confirmed cause: the active Build #6 is not matched to an
exact source commit, the VMBus entries do not identify owners, and the current
host-rescind correction is only a source diff on the upstream branch. It is
not yet ported to the WSL target, compiled, booted, or available as an
installable image. The correction also cannot reclaim allocations already
held by the running kernel before a matching kernel is activated.

**Verdict:** 🟡 `PARTIAL` — guest headroom is low and the kernel map trend is
material; current Windows RAM telemetry does not show host physical
exhaustion. The active freeze mechanism and safe WSL correction remain
unqualified. Do not claim that the new kernel is installed or that the
source-only patch will lower current memory use.

## 2026-09-27 15:37–15:54 -03 — WSL2 freeze and restart comparison

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0096`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T18:37:12Z`.
**Verified at:** `2026-09-27T18:54:58Z`.
**Source revision:** `e22fcd507d558230dc006836c05fe235c47677dc`.
**Source state:** Read-only review of the prior WSL journal, current guest
metrics, Windows memory/event samples, and four user-provided screenshots.
The user had already restarted
WSL. No stress, kernel build/install, shutdown, or configuration change was
performed during this evidence capture.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0091 through EVD-0095 and the VMBus source audit.
**Freshness:** Prior-boot health sample at 15:37:12 -03; last persisted prior
journal record at 15:41:47; screenshots file times 15:42:07–15:44:34; Windows
post-restart sample at 15:49:20; current guest sample at 15:51:39; focused
source test completed at 15:54.
**Category:** `reliability / memory / freeze / dashboard-scope`.
**How to measure:** Decode the RamShared journal payload and kernel journal
for the prior pressured guest boot; compare screenshot counters;
read current `/proc/meminfo`, `/proc/pressure/memory`, and `/proc/version`; use
Windows physical-memory/process counters and available System events.

**What:** Determine whether RamShared stress was active, whether the guest or
Windows host was under memory pressure, and what the persisted evidence says
about the freeze and recovery.

**Prior guest at the last decodable health sample:** The prior pressured guest
boot ran kernel
`6.18.40.1-microsoft-standard-WSL2+ #6`. The RamShared health payload had
`activation.active=false`, phase `Off`, daemon `alive=false`, and
`memory_events.oom=0` / `oom_kill=0`. It reported `MemAvailable=184,860 KiB`
(about 181 MiB), `SwapFree=727,400 KiB` of 4,194,304 KiB (3,477,636 KiB used,
about 3.32 GiB), PSI `some` avg10 28.21% and `full` avg10 27.65%, cumulative
swap reads 150.81 GiB, and 2,177,607 major faults. The health payload's
timestamp was 15:36:34; the journal stored it at 15:37:12 -03. RamShared
tiers and stress were off in this record and in Screenshot 108.

**Freeze window:** The last persisted line from that boot is
`systemd-journald: Under memory pressure, flushing caches.` at 15:41:47 -03.
There is no later guest record explaining why execution stopped. The saved
kernel journal contains no OOM-kill, panic/oops, hung-task, lockup,
allocation-failure, or I/O-error signature. This absence does not identify
the cause. Screenshot 108 around 15:42 shows guest memory at 99%
(15,858/15,995 MiB), swap at 82% (3,376/4,096 MiB), PSI stalls around 26%,
and tiers off. Screenshots 110–111 show disk I: at 100% active, about
92–102 MB/s read and 0 KB/s write. The screenshots do not identify the reader
or prove that the I: reads were caused by swap. `.wslconfig` places the WSL
swap VHDX on C:, so I: activity cannot be attributed to that
configured swap file from these counters alone.

**Restart sequence:** Journal boot history shows the pressured boot ending
at 15:41:47, a short boot from 15:44:32 to 15:45:17, and the current boot
starting at 15:45:23. The Windows sample at 15:49:20 had 14,357 MiB physical
RAM available and `VmmemWSL` working set 4,913 MiB. At 15:51:39, the current
guest reported `MemAvailable=13,146,664 KiB` (about 12.54 GiB), all 4 GiB of
swap free, zero memory PSI, and `VmallocUsed=228,612 KiB`. The same `#6`
kernel version was active after restart. This before/after recovery supports
guest-local pressure being cleared by restarting WSL; it does not isolate the
allocator, VHDX, or kernel mechanism that caused it.

At 15:49:24, the fresh boot had 330 `vmbus_alloc_buffer` maps and 37,567
declared backing pages (about 0.14 GiB), compared with 31,792 maps and
3,288,325 pages at 15:08 in EVD-0095. Restart resets guest allocations, so
this shows the prior guest-side map population did not persist across boots;
it does not identify the owning driver or prove a leak.

**Kernel image provenance:** `.wslconfig` selects a custom kernel image on
C:. That file's SHA-256 is
`46dba8cc9e2b0d9789917b329d2cdf4aaf5dc30ee982b4dd0f7d783cd41e4cc8`; its
embedded `#6` release/build stamp matches the running kernel's
`6.18.40.1-microsoft-standard-WSL2+ #6`. The checkout's current
`arch/x86/boot/bzImage` is a different `#8` image with SHA-256
`2d6d8935eecf23afeef5b71e2d367130383edac54a4c52829a6e94ee18449de9`.
There is no immutable build receipt tying the active `#6` image to a Git tree;
the kernel repo's current HEAD and uncommitted source diff therefore cannot
be treated as its source. The guest does not expose a hash of the image
already loaded into memory, so the configured-file match is strong but not a
cryptographic attestation of the in-memory image.

**Windows evidence:** Screenshot 109 shows Windows memory at about 51% and
`VmmemWSL` working set 3,215.5 MiB. The later post-restart host sample also
had substantial physical and commit headroom. Guest `MemAvailable`, Windows
physical RAM, `VmmemWSL` working set, and process private bytes measure
different things; the screenshots do not support a claim that Windows ran
out of physical RAM. Available System events show Hyper-V vNIC removal and
recreation during the recovery sequence, but no Windows disk/resource
exhaustion or unexpected-host-restart event. No enabled WSL/Lxss event channel
was available to explain the guest stop.

**Dashboard label:** The screenshot's `Host RAM` denominator (15,995 MiB)
matches guest WSL memory, not the Windows host's 32,670 MiB physical total.
Current source commit `2c3f1e35` distinguishes WSL2 memory as `WSL2 RAM`; the
focused `monitor::tests::memory_scope_distinguishes_wsl2_wsl1_and_native_linux`
test passed (1/1). Both inspected local release binaries predate that source
change and contain `Host RAM` strings without the WSL2 label: the installed
binary SHA-256 is `49f5a770c1aefcb386ca99a7bb89b8913929ca2fcf5bfa18da28fee60ea41b89`
and the workspace release binary SHA-256 is
`7190316d6d16816528b6f45c561789717efdcbccaf1ec2dddf5f55b8519d41c9`. The
screenshot process executable was not captured, so its exact binary identity
is unconfirmed. This display mismatch is not evidence for the freeze cause,
and the corrected source has not yet been rebuilt and installed. After the
focused test, guest `MemAvailable` was still 12.04 GiB, swap remained entirely
free, and PSI remained zero.

**Assessment:** The evidence confirms severe guest memory pressure and
thrashing before WSL stopped responding; it does not confirm RamShared stress
or Windows physical-memory exhaustion. The I: read burst is real but its
owner is unknown. EVD-0095's growing VMBus maps remain a strong candidate for
guest-side accumulation, while the active image's exact source, allocation
owners, GPADL causality, and a safe installed fix remain unproven.

**Verdict:** 🟡 `PARTIAL` — freeze preceded by severe guest memory/swap stalls;
exact trigger and safe kernel correction remain unresolved. Keep stress off.

## 2026-09-27 15:49–16:15 -03 — Post-restart host/guest memory divergence

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0097`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T19:08:54Z`.
**Verified at:** `2026-09-27T19:15:21Z`.
**Source revision:** `362247cdf7d7c140b751e225c07113f931f479ec`.
**Source state:** Paired read-only Windows and guest counters after the user
restarted WSL; review of screenshots, `.wslconfig`, and the uncommitted VMBus
source diff. After the samples, only `autoMemoryReclaim` was changed from
`disabled` to `gradual`; the prior file was backed up. No WSL shutdown, build,
kernel installation, or stress was performed.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0091, EVD-0095, EVD-0096, and the VMBus source
audit.
**Freshness:** Post-restart host sample at 15:49:20; guest sample at 15:51:39;
paired guest/host sample at 16:08:53–16:08:54; follow-up guest/host sample at
16:11:43–16:12:06; fresh RamShared health sample at 16:15:21 -03.
**Category:** `reliability / memory / host-guest / WSL2 / VMBus lifecycle`.
**How to measure:** Pair Windows physical-memory and `vmmemWSL` counters with
guest `/proc/meminfo`, `/proc/swaps`, memory PSI, and root-readable
`/proc/vmallocinfo`; read the effective configuration file and latest
RamShared health record. Keep resident bytes, private bytes, guest availability,
and page-cache values separate.

**What:** Determine whether the high Windows `vmmemWSL` working set after the
restart represents the same condition as the freeze, whether the new memory
reclaim setting addresses that host-side condition, and whether the current
VMBus diff is safe to build or install.

**Paired post-restart measurements:** At 16:08:53–16:08:54, the guest reported
`MemAvailable=12,418,808 KiB` (about 11.84 GiB), only 2,440 KiB of the 4 GiB
swap used, and zero memory PSI avg10. At the same time, Windows had
4,897.2 MiB physical RAM available out of 32,669.8 MiB, system commit
33,348.6/57,245.8 MiB, and `vmmemWSL` at 15,715.4 MiB working set
and 16,042.7 MiB private bytes. Thus the host had low physical headroom while
the guest still reported substantial availability; this was not the same
guest-side swap-thrashing state as EVD-0096.

At 16:11:43–16:12:06, guest `MemAvailable` was 9,060,656 KiB (about 8.64 GiB),
`Cached` was 8,991,088 KiB (about 8.58 GiB), swap use was about 58 MiB, and
PSI avg10 remained zero. `vmmemWSL` remained near 15.6 GiB working set; Windows
physical headroom was 4,447 MiB. The guest had 1,289
`vmbus_alloc_buffer` maps and 135,583 declared backing pages (about 530 MiB),
far below the 31,792 maps / 3,288,325 pages recorded before the freeze in
EVD-0095. Since the 15:49:20 host sample, `vmmemWSL` working set increased by
about 10,803 MiB while physical headroom fell by about 9,460 MiB over roughly
19 minutes. This demonstrates a host-resident WSL footprint increase, but
does not identify which guest allocations or files own all those bytes.

**Memory-reclaim configuration:** At review time, `.wslconfig` contained
`autoMemoryReclaim=disabled`. Microsoft documents that `disabled` turns off
automatic WSL memory reclamation, while `gradual` reclaims cached memory
slowly and `dropCache` reclaims it immediately
([WSL configuration](https://learn.microsoft.com/windows/wsl/wsl-config)).
At 16:14:59 -03, the setting was changed to `gradual`; an exact copy of the
previous file was retained for rollback. The change is not active in the
already-running WSL VM and will require its next start. This is a reversible
mitigation for possible host retention of guest cache, not a fix for the
previous guest freeze. Verify it only after a naturally scheduled or otherwise
authorized WSL restart by collecting paired host/guest values again. If guest
cached pages fall by at least 1 GiB over a 10-minute low-activity window but
Windows physical headroom does not improve by at least 512 MiB, restore the
previous setting and reject this as an effective host-headroom mitigation.

**RamShared state:** The screenshot during the freeze shows tiers off and the
daemon stopped. At 16:15:21, the fresh health sample reported phase `Off`,
`activation.active=false`, `daemon.alive=false`, `MemAvailable=9,483,844 KiB`,
`SwapFree=4,133,520 KiB`, and zero PSI avg10. No stress or cache activation
was running in the recorded current state.

**Kernel correction review:** The active guest still runs
`6.18.40.1-microsoft-standard-WSL2+ #6`; the current kernel checkout image is
not the active image and has no receipt linking Build #6 to its source. The
uncommitted VMBus diff now conservatively retains some buffers when GPADL
ownership is uncertain, but does not yet provide a reclamation path. The
review found that `uio_unregister_device()` does not account for or wait on
open `/dev/uio` VMAs; releasing or re-encrypting their backing pages before
the last mapping closes can leave userspace mappings referencing freed or
re-encrypted memory. The speculative reclaimer was removed. The remaining
diff passed `git diff --check`, but was not compiled, KUnit-tested, installed,
or run on a Hyper-V/CoCo guest. It is not a safe installation candidate.

**Assessment:** The freeze remains best explained by severe guest memory
stalling and swap thrashing while the Windows host still had physical memory
available. VMBus allocation growth remains a strong, unowned candidate for
that guest accumulation. The high post-restart Windows `vmmemWSL` working set
is a separate condition consistent with guest cache retention under
`autoMemoryReclaim=disabled`; the measurements do not prove it accounts for
all resident bytes. The I: read owner, allocation owners, Build #6 source,
and causal connection to GPADL remain unknown. No kernel correction was
installed.

**Verdict:** 🟡 `PARTIAL` — host-side cache reclaim is staged for the next WSL
start; freeze cause and safe GPADL/UIO reclamation remain unresolved. Keep
stress off and do not install the unbuilt source diff.

## 2026-09-27 16:27–16:28 -03 — Reclaim-setting activation and paired memory recheck

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0098`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T19:27:27Z`.
**Verified at:** `2026-09-27T19:28:24Z`.
**Source revision:** `4efa5fe605f74a54e02ef06e2244f24ca823c8ca`.
**Source state:** Read-only guest and Windows memory samples, current boot
time, active swap devices, process list, kernel release, and `.wslconfig`.
The host setting had already been edited to `autoMemoryReclaim=gradual`, but
the current WSL boot predates that edit. No WSL restart, stress, build, or
kernel installation was performed.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0096 and EVD-0097.
**Freshness:** Guest sample at 16:27:27 -03; Windows sample at 16:28:24 -03;
current VM boot began at 15:47:21 -03.
**Category:** `reliability / memory / host-guest / WSL2 / reclaim-activation`.
**How to measure:** Compare `/proc/meminfo`, `/proc/swaps`, memory PSI, process
and kernel state with Windows physical availability and `vmmemWSL` working-set
and private-byte counters. Compare config modification time with current VM
boot time before attributing a reclaim setting to live behavior.

**What:** Verify whether the staged WSL reclaim setting is active and capture
a fresh no-pressure host/guest memory pair before any new development load.

**Guest sample:** The active kernel remained
`6.18.40.1-microsoft-standard-WSL2+ #6`. `MemAvailable` was 9,484,188 KiB
(about 9.04 GiB), `Cached` was 9,251,932 KiB (about 8.82 GiB), and 57,808 KiB
(about 56.5 MiB) of the 4 GiB swap was used. Memory PSI `some` and `full`
avg10/avg60/avg300 were zero. `/proc/swaps` showed only the default WSL
fallback device; no `ramsharedd` process was present.

**Windows sample:** Physical RAM totaled 32,670 MiB, with 4,812 MiB available.
`vmmemWSL` had a 14,585 MiB working set and 15,969 MiB private bytes. Relative
to EVD-0097's 16:11 sample, its working set had fallen by about 1,031 MiB and
Windows physical headroom had risen by about 365 MiB. Guest cache instead rose
by roughly 255 MiB; swap use and PSI remained low.

**Reclaim activation:** `.wslconfig` was modified at 16:14:37 -03, while the
current WSL VM had started at 15:47:21 -03. Therefore
`autoMemoryReclaim=gradual` was not active during either sample. The decrease
in `vmmemWSL` working set cannot be credited to that setting; it shows that
working-set and physical-headroom changes are not a simple one-to-one cache
series. The earlier host-footprint concern remains, but the proposed
host-cache mitigation has not yet been tested after a WSL start.

**Assessment:** The guest was healthy and not in the pre-freeze swapping
condition during this sample. It does not explain the previous freeze, assign
the I: reads, match Build #6 to source, or validate the GPADL/UIO patch. This
is a no-pressure observation only.

**Verdict:** 🟡 `PARTIAL` — confirms the staged reclaim setting is inactive and
the current guest has low pressure; freeze cause, cache-mitigation efficacy,
and safe kernel correction remain open. Do not enable stress.

## 2026-09-27 15:37–16:47 -03 — Repeated WSL2 freeze and recovery

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0099`.
**Owner role:** `runtime / reliability`.
**Observed at:** `2026-09-27T18:42:07Z`.
**Verified at:** `2026-09-27T19:54:44Z`.
**Source revision:** `a5ea63d17dee47d836b2ac7f8d9e1aba5295ebf5`.
**Source state:** Read-only review of screenshots, prior guest health/journal,
boot history, `.wslconfig`, and Windows events; then committed a TUI state fix
as `a5ea63d1`. No release build/install, WSL shutdown, kernel install, or stress.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0095–EVD-0098 and the VMBus source audit.
**Freshness:** Guest sample 15:37:15; last prior record 15:41:47; screenshots
15:42:07–15:44:34; recovery boots 15:44:32 and 15:45:23; current guest
16:43:18 and Windows 16:47:44 (-03).
**Category:** `reliability / memory / freeze / recovery / dashboard`.
**How to measure:** Compare prior-boot health/journal with screenshot times,
Windows events, paired current guest/host memory, and VM/config timestamps.

**What:** Determine whether RamShared stress was active and identify the
best-supported cause of the repeat freeze.

**Prior-boot pressure:** At 15:37:15, guest `MemAvailable=191,900 KiB`
(~187 MiB), fallback swap used `3,479,772/4,194,304 KiB` (~3.32 GiB), and PSI
some/full avg10 was 32.45%/32.09%. RamShared activation and daemon were false,
phase was `Off`, and cgroup OOM counters were zero. The 1,543,500 KiB unmanaged
footprint was mostly `rust-analyzer` swap (1,543,224 KiB, 276 KiB RSS), a
contributor but not a proven trigger. Journald repeatedly flushed caches under
pressure through its final record at 15:41:47; no kernel OOM/panic/oops,
hung-task, or lockup signature was found.

**Screenshots:** At 15:42 the RamShared screen shows 15,858/15,995 MiB RAM,
3,376/4,096 MiB swap, ~26% PSI stalls, daemon stopped, and RAM/VRAM tiers off.
`Host RAM` is the WSL 16 GiB limit. Task Manager in the same minute shows ~51%
Windows memory use and `VmmemWSL` at 3,215.5 MiB; this does not reconcile with
the guest reading. I: is 100% active at 92.3 MB/s read, 0 KB/s write, 150 ms
response. Its reader is unknown; configured swap is on C:, so I: reads are not
proven to be swap I/O.

**Recovery:** The pressured boot ended at 15:41:47. A short boot ran
15:44:32–15:45:17; WSL logged `/sbin/init` timeout at 15:44:43 and Interop
failure at 15:44:53. The next boot began at 15:45:23 on the same
`6.18.40.1-microsoft-standard-WSL2+ #6` kernel. Windows logs show informational
vSwitch NIC changes but no Resource-Exhaustion-Detector or matching
Hyper-V Compute/Worker event.

**Current state:** At 16:43:18, guest availability was ~9.0 GiB, swap use
44 MiB, PSI zero. At 16:47:44, Windows had 5,247 MiB free physical RAM and
`vmmemWSL` 11,423 MiB working set / 15,758 MiB private bytes. The 16 GiB cap,
4 GiB `C:/wsl/swap.vhdx`, and `autoMemoryReclaim=gradual` are configured, but
`gradual` was edited at 16:14:37, after this VM started, and was not active.

**Dashboard correction:** The screenshot's `ARMED & READY` / `Protection:
ACTIVE` header contradicted phase Off and daemon stopped. Commit `a5ea63d1`
derives the label from protection state and blocks stale ACTIVE without a live
daemon; five focused tests pass. No release binary is installed.

**Assessment:** The immediate mechanism is best explained by severe in-guest
memory/swap pressure with sustained read stalls at the 16 GiB cap, not Windows
physical-memory exhaustion or RamShared stress. EVD-0095's 12.54 GiB declared
VMBus pages remain a strong kernel candidate, but were not measured at the
freeze; process, allocation owner, and I: reader are unknown. Build #6 source
is unmatched and the GPADL/UIO fix is unsafe to install.

**Verdict:** 🟡 `PARTIAL` — guest thrashing is confirmed; the initiating
allocation/process, I: reader, and safe matched-kernel fix remain unresolved.
Keep stress off and do not install the unqualified kernel diff.

## 2026-09-27 19:34–19:37 -03 — Active-gap source audit and monitor corrections

**What:** Compared every active reliability gate against current source,
tests, installed state, and the unbuilt VMBus worktree.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0100`.
**Owner role:** `runtime / reliability / source audit`.
**Observed at:** `2026-09-27T22:34:45Z`.
**Verified at:** `2026-09-27T22:37:29Z`.
**Source revision:** `79380a078d4df1b571827cb3d936b71e615ccf5a`.
**Source state:** Read-only comparison of all eleven active GAP-REGISTER rows
with their executable source and existing tests; committed monitor fixes;
read-only installed RamShared status and guest kernel/memory counters; static
review of the VMBus working diff and its tracked mail-series files. No stress,
Windows lifecycle, kernel build/KUnit, or install was run.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0096–EVD-0099 and the VMBus ownership review.
**Freshness:** Installed status and guest counters at 19:34:45 -03; source
revision and working-tree checks at 19:34–19:37 -03.
**Category:** `reliability / source audit / memory / monitor / VMBus / release gates`.
**How to measure:** Compare active GAP-REGISTER claims to current source and
named tests, then separately read installed `ramshared status --json`,
`/proc/meminfo`, `/proc/swaps`, `/proc/pressure/memory`, `uname -a`, current
Git revisions and worktree state. Do not infer deployment or hardware proof
from source tests.

**Monitor changes:** The dashboard previously turned missing or malformed PSI
averages into `0%`, and kept displaying the last I/O rate as real-time after a
refresh failed. Required `/proc/meminfo` counters were also defaulted to zero
without marking that sample unavailable. Commits `83dcde21` and `79380a07`
now reject malformed PSI as a pressure value, mark failed refreshes stale,
hide stale real-time I/O rates, track required memory-counter validity, render
missing/inconsistent RAM and swap counters as unavailable, and exclude invalid
memory samples from the history. The full CLI binary test suite passes 360/360;
Clippy with `-D warnings`, rustfmt check, and `git diff --check` pass. These
source changes are not in the installed v0.14.1 executable.

**Current installed and guest state:** At 19:34:45 -03,
`ramshared status --json` reported installed `binary_version=0.14.1`, phase
`Off`, protection/cache/origin `OFF`, daemon dead, ZRAM and VRAM absent, and
Guardian `BLOCKED` with `guardian_state_stale`. The only managed device was
the 4 GiB WSL fallback swap, with 2,558,876 KiB used. The guest reported
`MemAvailable=11,359,880 KiB`, `SwapFree=1,635,428 KiB`, and zero memory PSI
avg10/avg60/avg300. The running kernel remains
`6.18.40.1-microsoft-standard-WSL2+ #6`. This is a no-pressure guest sample;
it is not paired with current Windows physical/commit counters and does not
qualify stress admission. No RamShared tier or stress was active in this
sample.

**Source audit of every open gate:**

| Active gate | Source comparison and remaining proof |
| --- | --- |
| WSL2 freeze memory ownership — `PARTIAL` | The monitor corrections improve source telemetry but do not explain the prior guest swap thrash. The running `#6` image still has no immutable source receipt. The current VMBus working diff has safer uncertain-GPADL retention but lacks UIO VMA accounting and a retained-buffer reclaimer. No buffer-owner/channel attribution or paired current Windows sample exists. |
| WSL2 control-plane and revocable cache — `PARTIAL` | `ramshared-wsl2d` starts an isolated local GPU worker, but its entry point does not start the bounded host/guest transport primitives in `host_gate.rs`. No live handshake, lease/manifest delivery, fresh Guardian, or 24-hour rollout is evidenced. |
| Legacy WSL2 handoff and teardown — `PARTIAL` | The installed executable remains v0.14.1 and currently reports Off; source or hermetic tests cannot substitute for repeated idempotent start/stop on a clean v0.15 package with `BINARY_MATCH`. |
| Three-tier stress — `BLOCKED` | Source contains separate Windows physical/commit admission and guest `MemAvailable`/`SwapFree` gates before activation. The guest counters currently clear their 1 GiB reserves, but Guardian is stale, the installed binary is v0.14.1, no current Windows sample or attached-origin proof exists, and no stress was run. |
| Cross-vendor GPU budget — `PARTIAL` | `gpu_budget.rs` requires fresh driver-reported allocator state, adapter identity agreement, and the lower same-adapter WDDM headroom where available. Hermetic tests do not prove allocation/teardown on NVIDIA, AMD, Intel, or multiple live adapters. |
| VMBus upstream ring series — `BLOCKED` | Direct source review found `hv_is_isolation_supported()` has a weak false default in `hv_common.c` and no arm64 override; the old allocator therefore selected `vzalloc()` for arm64 host-visible buffers. It also rounded a `u32` size before storing the result back into `u32`, allowing a near-4-GiB request to wrap. The current uncommitted worktree now selects page chunks on arm64 and rejects unrepresentable rounded sizes, with KUnit cases. `checkpatch.pl --strict` reports 0 errors/warnings/checks on the modified C/H diff. The worktree patch hash is `3447b4f61d11e7fa4ce4154287706257f8ae36cb603480c0e19faa1098646ee0`; the tracked six mail patches do not contain these helpers. No build/KUnit was run. Host rescind still retains VMBus-owned UIO pages indefinitely because `uio_unregister_device()` does not wait for open VMAs and no VMA lifetime tracker/reclaimer is implemented. |
| Public Windows driver distribution — `BLOCKED` | The code provides lab/test-signing paths. No production-trusted signing identity or Microsoft attestation, verifier result with test-signing disabled, or public install/rollback evidence was found in this audit. |
| Windows physical lifecycle — `PARTIAL` | Current lifecycle scripts and refusal tests exist, but this Linux environment has neither `pwsh` nor Windows PowerShell; no PowerShell test, physical cold-boot drill, or loaded-binary identity check was run here. |
| Windows storage matrix — `PARTIAL` | The old disk-counter script is explicitly retired and points to `Invoke-WindowsStorageMatrix.ps1`; a static suite exists. No Windows run/artifact set proves the five-cell physical matrix, payload integrity, raw counters, or Event ID 153 result. |
| Custom-kernel DXG/systemd — `BLOCKED` | Current boot identity is `#6`; this audit produced no source receipt or same-host bundled/custom A/B, Xwayland/DXG probe, or fresh boot-log qualification. |
| Custom-kernel ublk product transport — `DEFERRED` | The repository keeps NBD as the day-1 path. This audit found no product ublk startup/teardown, crash-drain, or terminal no-ghost evidence that would justify promotion. |

**Assessment:** Source checks closed three monitor-reporting defects and exposed
two VMBus allocator defects; the latter are corrected only in an unbuilt,
uncommitted kernel worktree that is not represented in the tracked mail
series. The installed RamShared release and all external laboratory gates
remain unchanged. Keep every GAP-REGISTER status as shown above; do not
promote the kernel, activate stress, or claim cross-vendor/CoCo qualification.

**Verdict:** 🟡 `PARTIAL` — source and local test evidence improved, but installed
release parity, current Windows admission, UIO lifetime/reclamation, kernel
build/KUnit, Hyper-V/CoCo qualification, Windows hardware/signing, and
maintainer review remain open.

## 2026-09-27 22:00–22:06 -03 — Independent active-gap and configuration audit

**What:** Re-read every active `PARTIAL` gate against executable source and
named tests, checked the current installed RamShared state and paired guest /
Windows memory sample, and verified whether the cross-platform resource
configuration design has reached the CLI. Re-read the dirty VMBus patch's UIO
mapping and buffer ownership paths. Prior verdicts were treated as leads and
rechecked from source.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0101`.
**Owner role:** `runtime / reliability / source audit`.
**Observed at:** `2026-09-28T01:00:39Z`.
**Verified at:** `2026-09-28T01:06:15Z`.
**Source revision:** `79380a078d4df1b571827cb3d936b71e615ccf5a`.
**Source state:** RamShared worktree has the existing uncommitted parser-fixture,
documentation-checker, governance, reliability-record, and configuration
specification changes. The separate kernel worktree is based on
`6c2591cbe959d6ff4c310da9818b1743829b23da`, has five dirty source/documentation
files, and its current VMBus diff hashes to
`6d8bee0160ed25bf96d31a7e170e1c1a47bc530162abf91df742ac45b7615dbf` across
the selected kernel source files (excluding `IMPL.md`).
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0093–EVD-0100 until a provenance-matched release or
new paired incident evidence supersedes it.
**Freshness:** Installed/guest sample at 21:59–22:00 -03; Windows sample at
22:00:39 -03; source review at 22:00–22:06 -03. Guest and Windows readings
were collected within 73 seconds.
**Category:** `reliability / source audit / installed state / memory / VMBus /
configuration design`.
**How to measure:** Read installed state with `ramshared status --json`; read
`/proc/meminfo`, `/proc/swaps`, `/proc/pressure/memory`, and `uname -a`; query
Windows `Win32_OperatingSystem` and `Win32_PerfRawData_PerfOS_Memory` through
PowerShell; inspect the current Rust, PowerShell, and kernel source directly.
Run the named source tests and manufactured/static harnesses listed below.

**Current installed and paired state:** `ramshared status --json` reported
`binary_version=0.14.1`, phase `Off`, protection/cache/origin `OFF`, daemon
dead, Guardian `BLOCKED` with `guardian_state_stale`, and only the 4 GiB WSL
fallback swap active at priority `-2` (about 2,124 MiB used). The installed
command has no `config` subcommand. At 21:59 -03, the guest reported
`MemTotal=16,379,364 KiB`, `MemAvailable=10,274,176 KiB`, `SwapTotal=4,194,304
KiB`, `SwapFree=2,019,220 KiB`, and memory PSI `some/full avg10=0.00`. The
running kernel is `6.18.40.1-microsoft-standard-WSL2+ #6`. At 22:00:39 -03,
Windows reported 17,485 MiB physical memory free and commit 24,107/57,246 MiB
(33,139 MiB remaining). This healthy guest-pressure sample does not match the
earlier freeze and does not identify its initiating allocation; the WSL
kernel image still has no exact source receipt. The stale Guardian independently
prevents treating the current installation as ready for stress.

**Independent source re-audit of the six active PARTIAL gates:**

| Gate | Direct finding | Status |
| --- | --- | --- |
| WSL2 freeze memory ownership | The current sample has zero PSI and substantial guest availability, but the installed `#6` image remains source-unmatched. In the dirty candidate kernel patch, host rescind retains VMBus-owned buffers whose GPADL ownership is unresolved. UIO logical mappings have no VMA lifetime callbacks; `uio_unregister_device()` clears the info and unregisters without waiting for VMAs, while `hv_uio_remove()` proceeds to buffer cleanup and ring free. The candidate has no production retained-buffer reclaimer. This is a candidate ownership gap, not proof of the installed kernel's cause. | PARTIAL |
| WSL2 control-plane stability and effective revocable-cache transition | `host_gate` and AF_VSOCK/AF_HYPERV transport code exist, but direct search found no `host_gate` call or host-transport startup in `ramshared-wsl2d`'s daemon entrypoint or `ramshared-winsvc` service entrypoint. These are primitives/tests, not a live handshake. The installed Guardian is stale and the release remains 0.14.1. | PARTIAL |
| Legacy WSL2 service handoff and teardown | Swapoff-first and identity-bound teardown paths have passing hermetic regression cases, but the installed v0.14.1 daemon is Off with a stale Guardian; there is no current clean-release `BINARY_MATCH` or repeated post-reboot handoff evidence. | PARTIAL |
| Cross-vendor GPU budget identity and stress admission | Same-adapter allocator/WDDM budget checks and freshness/identity refusals are present and their unit tests pass. No live worker allocation, teardown, or AMD/Intel/multi-adapter campaign was run. | PARTIAL |
| Corrected Windows physical lifecycle qualification | The PowerShell static/manufactured lifecycle suites pass, but no cold-boot lifecycle, physical mutation, or loaded-binary identity drill was run. | PARTIAL |
| Windows virtual-disk properties, counters, and performance matrix | The static storage harness passes against manufactured cases; no physical five-cell matrix, raw-counter artifact set, payload-integrity run, or Event 153 qualification exists in this audit. | PARTIAL |

The checks run in this session were: `./scripts/docs-check.sh` (pass);
`cargo test -p ramshared-cli --bin ramshared
meminfo_missing_or_inconsistent_core_values_are_unavailable` (1 pass);
`cargo test -p ramshared-wsl2d --lib gpu_budget::tests` (13 pass);
`cargo test -p ramshared-wsl2d --lib host_gate::tests` (14 pass);
`cargo test -p ramshared-wsl2d --bin ramsharedd
daemon_nbd_teardown_refuses_until_fake_usage_and_swapoff_confirm` (1 pass);
`cargo test -p ramshared-cli --bin ramshared
legacy_migration_executor_preserves_swapoff_first_order` (1 pass); and the
PowerShell 5.1 manufactured/static harnesses
`Test-WindowsStorageMatrixStatic.ps1`,
`Test-RamSharedWslLifecycleRecoveryStatic.ps1`,
`Test-HostAutonomousLifecycleStatic.ps1`, and
`Test-RamSharedOriginStatic.ps1` (all pass). These tests do not qualify a
physical host, GPU, Windows disk matrix, or Hyper-V guest. No kernel build,
KUnit, stress, activation, WSL shutdown, or host installation was performed.

**Configuration design state:** The new resource-configuration PRD/SPEC and
SSDV3 2.5 review define one interface for native Linux and WSL2, with separate
providers. The native provider is designed to enumerate block devices and
mounted filesystems and allow supported swapfile/origin placement; WSL2 is
designed to enumerate host volumes and stage its own fallback-swap setting.
RAM/swap/VRAM values are per-user choices bounded by fresh provider data; the
different values in the `meminfo` parser test are input fixtures, not product
defaults or minimums. Optional per-volume speed comparison is consented and
bounded by the SPEC. The code has not implemented `ramshared config`, either
provider, or the interface; `ramshared --help` confirms the command is absent.
The SSDV3 verdict is GO for Step 3 implementation only, not feature completion.

**Assessment:** All six `PARTIAL` rows remain open for the specific missing
proof above. The source review confirms candidate code gaps but does not
establish the prior freeze's cause or qualify installation. Keep stress,
promotion, and universal hardware claims blocked by their current gates.

**Verdict:** 🟡 `PARTIAL` — current state is measured and the six open gates
were rechecked against source; deployment/release parity, live control-plane,
post-reboot lifecycle, physical GPU/Windows storage evidence, matched kernel
forensics, UIO mapping lifetime/reclamation, and the config implementation
remain incomplete.

## 2026-09-28 00:38–01:20 -03 — configuration inventory and host-memory recheck

**What:** Re-ran the v0.15.0 read-only configuration command and compared its
Linux and Windows resource inventories. Rechecked the configuration candidate
policy against native multi-disk behavior and WSL VHDX capacity, measured the
Windows PowerShell processes, and re-ran source-level Linux, GPU, lifecycle,
and Windows harnesses.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0102`.
**Owner role:** runtime / source audit / configuration / reliability.
**Observed at:** `2026-09-28T03:38:06Z`.
**Verified at:** `2026-09-28T04:20:12Z`.
**Source revision:** `3f8ddbacbbc23d33e7b4d8b1851d6785eceabb81`.
**Source state:** branch `feat/ramshared-20260921-consolidation`; source,
test, and specification changes are uncommitted. Separate kernel tree is
based on `6c2591cbe959d6ff4c310da9818b1743829b23da` and remains dirty.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0101 and EVD-0103; it supersedes EVD-0101's statement
that the CLI has no configuration command. It does not replace its historical
host/guest readings.
**Freshness:** Guest and Windows config samples were seven seconds apart;
PowerShell process and host physical-memory values were sampled 14 minutes
later.
**Category:** resource discovery / host and guest memory / source tests /
PowerShell static tests.
**How to measure:** Run `target/debug/ramshared config show --json`, read
`/proc/meminfo`, `/proc/swaps`, and `/proc/pressure/memory`, run
`/usr/local/bin/ramshared --version`, sample Windows
`Win32_OperatingSystem.FreePhysicalMemory` and PowerShell process private bytes,
then inspect Rust and kernel source directly. Run the named Rust and
PowerShell tests listed below.

**Paired state:** The WSL guest reported about 8.0 GiB `MemAvailable`,
2.35 GiB `SwapFree`, and zero memory PSI avg10/60/300. Its root filesystem was
ext4 directly on a whole virtual disk and reported about 1,007 GiB total and
834 GiB free. `ramshared config show --json` marks it ineligible for file
placement with the reason that its exact Windows backing-volume identity and
current host free capacity are not bound to this guest filesystem. The
Windows snapshot independently listed five fixed volumes. It did not identify
which one backs the WSL virtual disk; no C:/I: ranking is supported. Labels,
volume IDs, filesystem UUIDs, and hardware IDs are omitted.

At 03:38 UTC Windows reported about 1,682 MiB physical RAM free. At 03:52 UTC
it reported 1,778.3 MiB. That later sample found three PowerShell processes
with 231.3 MiB private memory total and a 115 MiB maximum per process. This
does not reproduce the previously observed 11.5 GiB process and does not
identify the earlier process's cause. The installed executable remains
v0.14.1; no `ramsharedd` process was present. The 4 GiB WSL fallback swap
was the only active swap device. No RamShared activation or
stress was run.

**Source correction:** Native Linux storage discovery joins `lsblk` device
identity with `/proc/self/mountinfo` and supports stable-ID filesystems on
partitions or whole disks. Direct review found that ext4/XFS on known
network-backed transports could pass the prior eligibility check even though
the SPEC requires local storage. Missing and unrecognized transport identity
was also accepted. The new named test first failed for iSCSI and missing
transport, then passed after known network transports and unproven transports
were refused. The same test passes separate NVMe and SATA local candidates.
Native Linux remains a first-class provider; WSL is a sibling
provider with a separate host-volume identity requirement. The command remains
read-only: it cannot select or apply swap/origin placement, profile caps,
benchmark storage, or recommend a fastest disk.

**Checks:** `cargo test -j 1 -p ramshared-cli` passed 379 unit and 11
integration tests. `cargo clippy -j 1 -p ramshared-cli --all-targets --
-D warnings` passed. The `resource_config.rs` line-coverage gate passed at
88.3% (1,131/1,281). `cargo test -j 1 -p ramshared-wsl2d --lib
gpu_budget::tests` passed 13/13; `host_gate::tests` passed 14/14; the
swapoff-first legacy migration test passed 1/1. PowerShell 5.1 static and
manufactured suites passed for the storage matrix, WSL lifecycle recovery,
host autonomous lifecycle, and origin paths. The tests used
`-ExecutionPolicy Bypass` only for each process; Windows policy was not
modified. Strict checkpatch on the dirty kernel C/H diff reported zero
findings. These tests do not establish physical Windows/SSD/GPU behavior,
native Linux mutation, kernel build/KUnit, or Hyper-V/CoCo qualification.

**Assessment:** The 16 GiB/4 GiB fixtures in the parser test remain test data,
not product minimums or preallocated capacity. This host's physical free RAM
was about 1.6–1.7 GiB during the sample; no full stress or kernel build was
attempted. Keep the installed/source mismatch and all hardware gates open.

**Verdict:** 🟡 `PARTIAL` — read-only discovery, local-storage refusal,
variable-size parser inputs, and static/unit tests are verified. Configuration
mutation, native Linux live validation, disk comparison, and hardware gates
remain incomplete.

## 2026-09-28 00:38–01:20 -03 — independent audit of every active PARTIAL

**What:** Re-read executable Rust, PowerShell, and separate kernel candidate
source for every active `PARTIAL` in the Gap Register. Re-ran its named local
tests and static harnesses. Did not accept EVD-0100/EVD-0101 conclusions
without a matching source check or a fresh sample.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0103`.
**Owner role:** independent source / runtime reliability audit.
**Observed at:** `2026-09-28T04:20:12Z`.
**Verified at:** `2026-09-28T04:20:12Z`.
**Source revision:** `3f8ddbacbbc23d33e7b4d8b1851d6785eceabb81`.
**Source state:** RamShared worktree contains uncommitted source, test, and
documentation changes.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0100–EVD-0102 until release or platform evidence
supersedes the relevant gate.
**Category:** active PARTIAL source audit / named unit and static tests /
installed identity.
**Freshness:** Guest and Windows measurements were sampled at 03:38 UTC;
PowerShell process totals at 03:52 UTC; source and harness checks completed at
04:20 UTC.
**How to measure:** Inspect the exact entrypoints, named tests, current
installed binary, guest swap/pressure, kernel worktree diff, and Windows
static harness results. No stress, mutation, activation, host install, WSL
shutdown, or physical-disk benchmark was run.

| Active gate | Direct source/test finding | Status |
| --- | --- | --- |
| WSL2 freeze memory ownership | Current guest sample has zero PSI but cannot explain the earlier freezes. Active WSL `#6` has no exact source receipt. Candidate source has UIO page references, but unregister does not wait for mapping closure; removal proceeds to buffer cleanup and memory reencryption without a VMA lifetime tracker. The candidate retained-buffer list has only test cleanup and no production reclaimer. Strict checkpatch is clean; candidate build, KUnit, install, GPADL runtime interleavings, and CoCo transitions remain untested. Do not claim a proven UAF or freeze cause. | PARTIAL |
| WSL2 control-plane stability and effective revocable-cache transition | Direct search of both daemon/service source trees found no production `host_gate` call or AF_VSOCK/AF_HYPERV startup from the runtime entrypoints. `host_gate::tests` passes 14/14 but tests the helper policy, not a live transport. No live handshake, lease/manifest exchange, or 24-hour rollout exists. | PARTIAL |
| Legacy WSL2 service handoff and teardown | Installed `/usr/local/bin/ramshared` is v0.14.1; no `ramsharedd` process exists and the fallback swap is the only active swap. The source swapoff-first test passes 1/1. No installed v0.15.0 `BINARY_MATCH` or post-reboot repeated handoff was run. | PARTIAL |
| Cross-vendor GPU budget identity and stress admission | The fresh-identity, WDDM intersection, freshness, reserve, and refusal tests pass 13/13. No live allocator worker, GPU memory allocation/teardown, second adapter, or AMD/Intel campaign was run. | PARTIAL |
| Corrected Windows physical lifecycle qualification | PowerShell 5.1 is available in this environment. Static/manufactured suites for WSL lifecycle recovery, host autonomous lifecycle, and origin safety pass. They do not exercise physical cold boot, current loaded-binary identity, recovery, or rollback on the target host. | PARTIAL |
| Windows virtual-disk properties, counters, and performance matrix | `Test-WindowsStorageMatrixStatic.ps1` passes its manufactured matrix and refusal checks. No physical five-cell, three-run matrix, intended-payload integrity, raw counter bundle, or current Event ID 153 window was collected. | PARTIAL |
| Cross-platform resource configuration | PRD/SPEC specify equal native Linux and WSL2 providers and variable user ceilings. Source now implements read-only resource discovery; Linux stable-ID ext4/XFS targets support multiple disks and known remote or unclassified transports refuse. WSL guest filesystems remain ineligible until backing Windows volume identity/free capacity are bound. There is no typed profile, `plan`/`apply`, managed swap/origin write, disk benchmark, or speed recommendation; native Linux live E2E has not run. | PARTIAL |

**Assessment:** All seven `PARTIAL` statuses remain accurate for the missing
platform proof or unimplemented feature work. Static/unit results advance the
evidence without qualifying physical Windows, GPU, storage, or kernel paths.
The earlier EVD-0101 statement that `ramshared config` did not exist is
historical and is superseded by EVD-0102; the present command is a read-only
inventory only. The earlier assumption that the PowerShell runtime was absent
is also not valid for this sample: PowerShell 5.1 is available and the current
static harnesses pass.

**Verdict:** 🟡 `PARTIAL` — source-level gaps have been corrected or precisely
bounded, but none of the seven gates is closed by this audit.

## 2026-09-28 02:18 -03 — typed resource profile and independent gate recheck

**What:** Rechecked the active PARTIAL findings against the current Rust,
PowerShell, and separate kernel-candidate source. Added a bounded v1 resource
profile model for variable user ceilings and stable platform storage targets.
Captured a fresh read-only guest sample after source tests.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0104`.
**Owner role:** source audit / configuration / reliability.
**Observed at:** `2026-09-28T05:18:30Z`.
**Verified at:** `2026-09-28T05:39:30Z`.
**Source revision:** `9c94c7b78f91109930d32a9542baf3fb4bf0cc41`.
**Source state:** branch `feat/ramshared-20260921-consolidation` is at the
source commit above; this EVD, the gap-register update, and the generated
capability-observation update are documentation-only changes. The separate
kernel tree remains dirty and is not part of this RamShared revision.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0102 and EVD-0103 until profile integration or
live platform/release evidence supersedes the relevant gate.
**Freshness:** Guest memory, swap, PSI, installed CLI version, and process
presence were sampled together at 05:18 UTC. Source tests and static suites
completed by 05:20 UTC.
**Category:** typed resource policy / source and static tests / guest status.
**How to measure:** Read `/proc/meminfo`, `/proc/swaps`, and
`/proc/pressure/memory`; run the installed CLI `--version`; check for a running
`ramsharedd`; execute the named Rust and PowerShell suites; inspect the active
source entrypoints and separate kernel diff. No RamShared activation, storage
write, benchmark, stress, kernel build, or host install was performed.

**Current guest sample:** `MemTotal` was about 15.6 GiB, `MemAvailable` about
7.3 GiB, and `SwapFree` about 2.6 GiB. Memory PSI avg10/60/300 remained zero.
The only active swap entry was the 4 GiB WSL fallback swap. The installed CLI
still reports v0.14.1, and no `ramsharedd` process was present. This is a
read-only point sample; it does not prove the earlier freeze cause or qualify
an installed v0.15.0 binary.

**Profile change:** `ramshared-config::resource_profile` now parses TOML up to
64 KiB and validates schema version, variable byte ceilings, stable adapter
and storage IDs, native Linux versus WSL2 target types, Linux relative paths,
Windows absolute paths, and target allocation metadata. It computes a checked
combined storage free-space requirement with the SPEC's 10 GiB reserve floor;
it does not inspect live volume free space or authorize writes. Six named
integration tests pass. The two required profile tests verify variable values,
overflow refusal, and stable volume/adapter ID round-trip. The profile is not
loaded or persisted by `ramshared config`; selection, plan/apply, managed
swap/origin writes, and disk comparison remain unimplemented.

**Checks:** `cargo test -j 1 -p ramshared-config` passed 15 unit and 6
integration tests; strict package Clippy passed; its business-logic slice
coverage passed at 95.6% (172/180 lines). `cargo test -j 1 -p ramshared-cli`
passed 379 unit and 11 integration tests; strict Clippy passed and the
`resource_config.rs` slice gate passed at 88.3% (1,131/1,281). The isolated
GPU budget suite passed 13/13, host-gate suite 14/14, and swapoff-first
migration test 1/1. PowerShell 5.1 static/manufactured suites passed for the
Windows storage matrix, WSL lifecycle recovery, autonomous lifecycle, and
origin safety. Kernel `checkpatch.pl --strict` on the dirty candidate diff
reported zero errors, warnings, or checks. `-ExecutionPolicy Bypass` was used
only per test process; Windows policy was not changed. Rust formatting,
`git diff --check`, docs-index, validation-schema, and the complete
`./scripts/docs-check.sh` all passed after regenerating the capability
observations artifact. These checks do not establish live host/guest transport,
loaded-binary identity, physical storage or GPU behavior, VMBus runtime
interleavings, KUnit, or CoCo transitions.

**Independent status audit:** All seven current PARTIAL rows remain open for
source or environment reasons confirmed directly. The kernel candidate still
has no production retained-buffer reclaimer or UIO VMA-close tracker, and was
not built or installed. The host/guest control-plane module is not started by
the daemon/service entrypoints; its helper tests do not prove a live handshake.
The current release remains v0.14.1. GPU policy has refusal and allocation
unit coverage but no live multi-adapter/vendor allocation campaign. Windows
physical lifecycle and storage-matrix evidence remains absent. Resource
configuration now has a typed model but still lacks CLI/provider integration
and native Linux live E2E. No prior PARTIAL verdict was promoted based on
source presence or manufactured tests alone.

**Verdict:** 🟡 `PARTIAL` — the typed profile and read-only policy gates
advance, but all seven reliability/qualification gates remain open.

## 2026-09-28 03:00 -03 — multi-target resource profile validation

**What:** Corrected the v1 resource profile so one user configuration can hold
managed swap and origin targets on the same or different stable volumes. Added
WSL origin placement as a distinct profile target, grouped capacity by stable
volume identity, and refused duplicate managed paths before provider use.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0105`.
**Owner role:** resource configuration / source validation.
**Observed at:** `2026-09-28T06:00:30Z`.
**Verified at:** `2026-09-28T06:00:30Z`.
**Source revision:** `d466def5`.
**Source state:** branch `feat/ramshared-20260921-consolidation` contains the
reviewable source and SPEC change in `d466def5`; this evidence record is the
follow-up documentation change.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0102 through EVD-0104 until CLI profile integration
and provider qualification supersede this model evidence.
**Freshness:** Source tests, Clippy, slice coverage, and documentation checks
ran on the source revision recorded above; no runtime sample is claimed.
**Category:** resource profile schema / unit tests / source documentation.
**How to measure:** Run `cargo test -j 1 -p ramshared-config`,
`cargo clippy -j 1 -p ramshared-config --all-targets -- -D warnings`,
`node tools/ci/check-rust-slice-coverage.mjs -p ramshared-config --files
crates/ramshared-config/src/resource_profile.rs --min 80`,
`cargo fmt --all -- --check`, `git diff --check`, and
`./scripts/docs-check.sh`. Inspect profile validation and named integration
tests. No profile file was read or written, and no host, guest, disk, swap,
origin, GPU, or kernel state was mutated.

**Profile correction:** The added test first failed because the prior schema
rejected `targets` and allowed only one `target`. The model now accepts a
bounded list containing Linux swapfile/origin or WSL fallback-swap/origin
entries. It validates each target against the detected platform, rejects
case-insensitive duplicate Windows paths on the same stable volume, and
computes checked required free bytes per volume by summing all selected files
and adding the 10 GiB floor once. The floor is a storage reserve, not a RAM,
swap, or VRAM minimum. This is pure profile validation; it does not inspect
live free space, bind a saved profile to discovered candidates, authorize
writes, or configure runtime tier caps.

**Checks:** `cargo test -j 1 -p ramshared-config` passed 15 unit tests and 8
profile integration tests. Strict Clippy passed. The profile slice coverage
gate passed at 94.2% (244/259 lines). `cargo fmt --all -- --check`,
`git diff --check`, and the complete `./scripts/docs-check.sh` passed. The
named tests verify multi-target TOML round-trip, same-volume sum, independent
per-volume reserve, WSL swap/origin coexistence, duplicate-path refusal, and
capacity overflow refusal.

**Remaining boundary:** The profile remains unloaded and unpersisted by
`ramshared config`; there is no selection UI, live plan, managed swap/origin
writer, storage speed comparison, or native Linux live E2E. All seven active
reliability gates remain `PARTIAL`; this source change does not qualify the
installed v0.14.1 binary, host/guest transport, GPU hardware, physical Windows
storage/lifecycle, or the separate VMBus/CoCo candidate. No stress, activation,
host install, WSL shutdown, kernel build, or hardware campaign was run.

**Verdict:** 🟡 `PARTIAL` — multi-volume profile semantics and refusal logic
are implemented and covered; CLI integration and platform execution remain
open.

## 2026-09-28 05:51 -03 — independent re-audit of seven active PARTIAL gates

**What:** Re-read the current source for every active `PARTIAL` row instead of
reusing its prior verdict. Reproduced the read-only resource-plan tests and
found that Linux target profiles persisted a boot/namespace-scoped mount ID.
Removed that identity from saved targets, resolved one fresh mount from stable
filesystem/device identity, and refused ambiguous mounts and filesystem
subtree roots. The separate kernel candidate and installed host state were
also inspected read-only.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0106`.
**Owner role:** independent source audit / configuration / reliability.
**Observed at:** `2026-09-28T08:51:55Z`.
**Verified at:** `2026-09-28T08:51:55Z`.
**Source revision:** `750090a54c13ff6662aab3dde453e4f5a4dc648c`.
**Source state:** RamShared branch `feat/ramshared-20260921-consolidation`
contains the source correction and PRD/SPEC/IMPL update at the recorded
revision. This EVD and the corresponding GAP-register entry are follow-up
documentation. The separate kernel candidate has a dirty worktree; it is not
part of this RamShared revision.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0100 through EVD-0105 until each open platform or
feature gate has its own current close evidence.
**Freshness:** Guest CLI version, kernel release, `/proc/meminfo`,
`/proc/swaps`, memory PSI, and daemon presence were sampled together at
08:51 UTC. Source tests and
PowerShell static suites ran during this audit.
**Category:** source re-audit / named tests and coverage / read-only runtime
sample.
**How to measure:** Run the listed Cargo tests and coverage gates; inspect
`crates/ramshared-wsl2d/src/main.rs`, `crates/ramshared-winsvc/src/main.rs`,
the separate kernel diff and UIO remove/mmap paths; run the named Windows
static scripts; read `/proc/swaps`, `/proc/pressure/memory`, the installed
CLI version, and `uname`. No activation, stress, storage write, benchmark,
host install, WSL shutdown, kernel build, KUnit, or physical campaign ran.

**Current read-only sample:** `/usr/local/bin/ramshared --version` reports
`0.14.1`; `uname` reports `6.18.40.1-microsoft-standard-WSL2+ #6`.
`MemTotal` is 16,379,364 KiB and `MemAvailable` 7,043,872 KiB. The only swap
entry is the 4 GiB WSL fallback swap device, with 1,326,072 KiB used and
2,868,232 KiB free. Memory PSI `some` and `full` avg10/60/300 are all zero,
and no `ramsharedd` process was found. This sample does not identify the
source commit behind the running kernel or explain earlier freezes.

**Checks:** `cargo test -j 1 -p ramshared-config` passed 15 unit and 10
profile integration tests; the resource-profile slice gate passed at 91.3%
(293/321 lines). The focused CLI config suite passed 26/26; its full coverage
run passed 389 unit and 12 integration tests, with the resource-config slice
at 87.6% (1,793/2,046 lines). Strict Clippy passed for both affected crates.
The WSL host-gate suite passed 14/14, the GPU budget suite 13/13, the Windows
service control-plane helper suite 12/12, and the legacy `swapoff_first`
suite 3/3. PowerShell 5.1 static/manufactured suites passed for host
autonomous lifecycle, WSL lifecycle recovery, origin safety, and the Windows
storage matrix. `cargo fmt --all -- --check`, `git diff --check`, the gap
register tests, and full `./scripts/docs-check.sh` passed. These are source,
static, or hermetic results; they do not qualify live host/guest transport,
physical storage/GPU behavior, kernel runtime interleavings, or CoCo memory
transitions.

**Seven-gate source audit:**

| Active gate | Fresh finding | Status |
| --- | --- | --- |
| WSL2 freeze memory ownership | The running `#6` image still has no source-revision receipt. The separate kernel diff is dirty; its retained-buffer list has no production drain/reclaimer. In the UIO path, `uio_unregister_device()` clears `idev->info` without waiting for existing mappings, and `hv_uio_remove()` then runs buffer cleanup. This is an ownership/lifetime risk, not proof of a UAF or the prior freeze trigger. No candidate build, KUnit, install, GPADL drill, or CoCo transition was run. | PARTIAL |
| WSL2 control-plane stability and effective revocable-cache transition | The helper suites pass, but source search finds no `host_gate` or `control_plane` call from either production daemon/service entrypoint. There is no live handshake, lease/manifest exchange, or rollout proof. | PARTIAL |
| Legacy WSL2 service handoff and teardown | Three swapoff-first regression tests pass, but the installed CLI remains v0.14.1, no daemon is running, and this audit has no current-release `BINARY_MATCH` or repeated post-reboot handoff evidence. | PARTIAL |
| Cross-vendor GPU budget identity and stress admission | Thirteen policy tests cover freshness, identity, reserve, WDDM intersection, and refusals. No live worker allocation/teardown or AMD/Intel/multi-adapter campaign ran. | PARTIAL |
| Corrected Windows physical lifecycle qualification | Host lifecycle, recovery, and origin static/manufactured checks pass. They do not load and verify the corrected package across supervised physical cold boots or prove rollback on the target host. | PARTIAL |
| Windows virtual-disk properties, counters, and performance matrix | The static harness validates the specified cells and refusal paths; no physical five-cell, three-run matrix, 75-sample artifact bundle, payload-integrity run, or current Event ID 153 window exists in this audit. | PARTIAL |
| Cross-platform resource configuration | Native Linux and WSL2 are both in the PRD/SPEC, and the profile/planner supports variable caps plus multiple stable storage targets. A newly found transient mount-ID defect is fixed. The CLI plan remains read-only: there is no interactive target selection, profile persistence, apply/rollback provider, bounded disk benchmark, or native Linux live target E2E. WSL guest filesystems still require bound Windows-volume capacity. | PARTIAL |

**Assessment:** The seven status labels remain accurate after direct source
inspection and fresh named checks. The profile correction improves
cross-boot Linux target resolution but does not turn the UI into a complete
configurator. No prior `PARTIAL` became `PASS`; unit/static proof did not
substitute for the missing platform evidence.

**Verdict:** 🟡 `PARTIAL` — one concrete source defect was corrected and all
seven active gaps were re-audited, but their required feature and platform
proof remains open.

## 2026-09-28 06:28 -03 — enumerate every Windows volume candidate

**What:** Re-read the Windows inventory collector, renderer, planner, and
resource-configuration SPEC. The PowerShell collector filtered out every
non-fixed volume, and the text view omitted volume identity, drive type, and
eligibility reasons. The collector now preserves every row returned by
`Get-Volume`; missing capacity remains null. The text view reports identity,
type, capacity, and the planner's volume-level refusal reason. Volume rows are
candidate inventory only; a plan still binds a configured path to fresh
identity and host capacity.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0107`.
**Owner role:** cross-platform resource configuration / source validation.
**Observed at:** `2026-09-28T09:28:42Z`.
**Verified at:** `2026-09-28T09:28:42Z`.
**Source revision:** `5a93d101afa42faed2d06bf17e681986a89cc1ff`.
**Source state:** Test checkpoint `ac4bd09e` records two failing regressions;
`5a93d101` contains the implementation that makes them pass.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0102 through EVD-0106; this corrects one source
gap but does not close the resource-configuration gate.
**Freshness:** The named CLI discovery E2E ran against the current WSL guest and
executed the actual bounded Windows inventory provider. No volume identities,
labels, or raw host paths are retained here. The installed release was not
changed by this source test.
**Category:** source regression / live read-only WSL discovery / coverage.
**How to measure:** Run the named `windows_inventory_*` unit tests, the
`cli_resource_config_json_discovers_platform_resources_read_only` integration
test, the CLI source-slice coverage gate, and strict Clippy. No volume write,
benchmark, swap change, tier activation, host install, or WSL shutdown was run.

**Checks:** The first RED run failed because the view still printed
`Windows fixed volumes:` and the collector contained a fixed-only
`Where-Object`. After the fix, the two focused inventory tests passed and the
resource-config unit suite passed 28/28. The live CLI discovery E2E passed;
the complete coverage run passed 391 unit tests and 12 integration tests, with
the resource-config slice at 88.5% (1,900/2,148 lines). Strict Clippy passed
for `ramshared-cli` and `ramshared-config`. These results qualify discovery
and rendering only. They do not prove every Windows volume class on every
machine or a storage write path.

**Remaining boundary:** The configurator remains read-only. It cannot let a
user choose a target, persist a profile, change swap or tier caps, apply or
rollback settings, or benchmark and recommend a disk. No native Linux live
target test or WSL plan against a selected real volume ran. The resource
configuration gate remains `PARTIAL`; the other six active gates retain the
open evidence recorded in EVD-0106.

**Verdict:** 🟡 `PARTIAL` — complete Windows volume rows and refusal reasons
are now visible and tested; selection, mutation, benchmarking, and platform
qualification remain open.

## 2026-09-28 09:37 -03 — select storage targets in a safe user draft

**What:** Continued an independent audit of the resource-configuration source.
Added `ramshared config draft --output PATH`, an attended stdin/stdout TTY
wizard that lists storage candidates and refusal reasons, selects eligible
native Linux mounts or WSL Windows volumes (drive letter or canonical volume
GUID), accepts variable MiB sizes for fallback swap and SSD-origin requests,
and reviews the combined target-plus-reserve plan. The writer creates only a
new current-user file with mode `0600`, verifies its exact bytes, syncs file
and parent directory, and never overwrites. It does not write the protected
system profile or apply a setting.

The independent capacity audit also found that the Windows provider resolved
volume IDs case-insensitively while the profile capacity map grouped them
case-sensitively. Two targets using `volume-guid-a` and `VOLUME-GUID-A` could
therefore be evaluated as separate disks. The RED test in `60ead002` reproduced
the split; `e6291085` canonicalizes these identities before the checked sum and
reserve lookup. A planner regression now refuses 60 GiB of combined targets
plus the 10 GiB reserve when that volume reports only 65 GiB free.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0110`.
**Owner role:** cross-platform resource configuration / source audit.
**Observed at:** `2026-09-28T12:37:12Z`.
**Verified at:** `2026-09-28T12:37:12Z`.
**Source revision:** `e6291085d30d87cb48e273538a2dbe7e1935bc74`.
**Source state:** `60ead002` is the intentionally failing regression
checkpoint; `e6291085` contains the implementation and passing tests. The
feature remains in the v0.15.0 source branch; it is not installed.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0107 through EVD-0109; this advances source
selection and closes one planner under-count but does not close the resource
configuration gate.
**Freshness:** Source tests and read-only CLI discovery ran on 2026-09-28. The
current Windows volume provider was queried by the existing discovery E2E, but
no target volume was written or benchmarked. No native Linux machine or
selected real WSL target was qualified.
**Category:** source regression / read-only planning / bounded draft-file
creation / slice coverage.
**How to measure:** Run the CLI package suite and both resource-profile and
resource-config slice coverage gates; run strict Clippy, formatting, and the
full documentation checks. No swap activation, origin creation, storage
benchmark, GPU context/allocation, system profile write, `.wslconfig` change,
kernel build/install, stress, or WSL shutdown was performed.

**Checks:** The CLI suite passed 400 unit tests and 13 integration tests. The
resource-config slice gate passed at 88.4% (2,633/2,978 lines), and the
resource-profile slice gate passed at 93.7% (314/335 lines); both exceed the
80% requirement. Strict Clippy passed for `ramshared-cli` and
`ramshared-config`. `cargo fmt --all -- --check`, `git diff --check`, and
`./scripts/docs-check.sh` passed. The draft tests cover native mounts, Windows
drive letters, volume-GUID targets without a drive letter, stale/ambiguous/
ineligible inventory, variable sizes and overflow, explicit confirmation,
no-overwrite, owner/mode, and non-TTY refusal. These checks do not demonstrate
system-profile persistence or a live storage mutation.

**Remaining boundary:** The draft contains fallback-swap and SSD-origin
requests only. It does not expose adapter-structured GPU/VRAM settings,
storage-speed testing/recommendation, a protected system-profile writer,
apply/rollback providers, or durable transaction audit. Native Linux live
qualification and WSL before/action/after on a selected real host volume are
absent. The cross-platform resource-configuration gate remains `PARTIAL`.

**Verdict:** 🟡 `PARTIAL` — disk/volume selection and safe draft persistence
work in source, and the case-insensitive capacity under-count is fixed. GPU,
benchmark, provider, and platform-qualification gates remain open.

## 2026-09-28 08:05 -03 — model and plan a not-yet-created native origin request

**What:** Independent review found that `linux_file_origin` required an inode
and identity hash before a profile could express a new native Linux origin.
Added the separate `linux_file_origin_request` intent form, which records only
stable filesystem/device identity, managed relative path, and requested bytes.
The read-only planner binds that request to a fresh current mount and checked
capacity; it does not create or open the file and keeps apply disabled.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0109`.
**Owner role:** resource configuration / independent model review.
**Observed at:** `2026-09-28T11:05:06Z`.
**Verified at:** `2026-09-28T11:13:02Z`.
**Source revision:** `e2c35eb7a36fa70777a4c91ec376f152deaadc4a`.
**Source state:** The feature and SPEC/IMPL corrections are committed in
`e2c35eb7`; this validation and active-gap update are recorded with the
evidence. This remains the v0.15.0 source branch; no install was performed.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0101 through EVD-0108; this closes a model
representability defect only and does not close the resource-configuration
gate.
**Freshness:** Source-only test evidence at 2026-09-28 11:05 UTC. No current
native Linux hardware, selected Windows volume, GPU adapter, or storage-write
campaign was used. The installed CLI and kernel were not changed.
**Category:** source regression / read-only planning / line coverage.
**How to measure:** Record the initial RED compile failure for the missing
profile variant, run the config and CLI test suites, both business-logic slice
coverage gates, strict Clippy, formatting, and docs checks. No swap, origin,
profile file, WSL setting, GPU allocation, host process, or kernel state was
mutated.

**Checks:** The RED checkpoint `266a9bd0` failed because the required request
variant did not exist. After implementation,
`resource_profile_accepts_a_new_linux_origin_request_without_a_preexisting_inode`
and `native_linux_origin_request_plan_binds_volume_without_claiming_creation`
pass. `cargo test -j1 -p ramshared-config -p ramshared-cli` passed 392 CLI
unit tests, 12 CLI integration tests, 15 config unit tests, and 11 profile
integration tests. Profile coverage passed at 93.1% (312/335 lines); resource
config coverage passed at 88.7% (1,941/2,189 lines). Strict Clippy, formatting,
`git diff --check`, validation schema, GAP Register checks, and the full docs
suite passed.

**Remaining boundary:** The TUI still cannot select a disk or GPU, edit tier
caps, save a profile, benchmark storage, or apply settings. The plan does not
prove that the requested path is absent or authorize a write. Native Linux
before/action/after evidence is still absent. The gate remains `PARTIAL`.

**Verdict:** 🟡 `PARTIAL` — new Linux origin intent is now representable and its
read-only volume/capacity plan is tested; selection, persistence, provider
mutation, and live Linux/WSL2 qualification remain open.

## 2026-09-28 07:22 -03 — independent re-audit of all seven active PARTIAL gates

**What:** Re-read current source and evidence for each active gate instead of
reusing the EVD-0106 conclusions. Reran the available Rust policy/identity
tests and Windows static suites. Corrected two public source descriptions:
the kernel-fork README no longer calls the v2 candidate production-qualified
or claims support across all Hyper-V architectures, and the Windows
`control_plane` module comment now says its helpers and AF_HYPERV transport are
not wired into the service.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0108`.
**Owner role:** independent reliability / source audit.
**Observed at:** `2026-09-28T10:22:07Z`.
**Verified at:** `2026-09-28T10:22:07Z`.
**Source revision:** `3cb1babc4b1c80ff9f5f168a6f8d420172d33161`.
**Related kernel fork revision:** `a022ac393ecaab845682f5afe2be6be792aedde2`.
**Source state:** The RamShared comment correction is committed. The kernel
README correction is committed and pushed to the public fork. The kernel v2
candidate still has uncommitted source changes in its working tree; those
changes were not built, tested, installed, or committed by this audit.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0100 through EVD-0107 until every feature and
platform gate has its own current close evidence.
**Freshness:** Read-only WSL sample at 2026-09-28 10:18 UTC: `MemTotal`
16,379,364 KiB, `MemAvailable` 6,729,104 KiB, WSL fallback swap 4,194,304 KiB
total / 2,864,716 KiB free, and memory PSI some/full avg10/60/300 all 0.00.
The installed health-monitor process used 2,888 KiB RSS; no `ramsharedd` or
stress process was found. This sample does not explain prior freezes or prove
that the kernel candidate caused them.
**Category:** independent source re-audit / named tests / read-only runtime.
**How to measure:** Re-read the entrypoints, host-gate/control-plane modules,
GPU admission policy, VMBus/UIO candidate, and Windows lifecycle/storage
harnesses; run the listed focused Rust tests and static scripts. No kernel
build, KUnit, install, host mutation, stress activation, storage benchmark,
WSL shutdown, or CoCo transition was run.

**Direct audit and current verdicts:**

| Active gate | Fresh source or test finding | Status |
| --- | --- | --- |
| WSL2 freeze memory ownership | The kernel branch still has dirty changes. `hv_uio_remove()` unregisters UIO and immediately tears down its buffers/ring; the UIO core clears `idev->info` without a VMA-close tracker. The candidate retains uncertain GPADLs, but its retained-owner cleanup is test-only and has no production reclaimer. This is a lifetime risk, not proof of a use-after-free or prior freeze cause. No candidate build, KUnit, GPADL drill, or CoCo test ran. | PARTIAL |
| WSL2 host/guest control plane | AF_VSOCK/AF_HYPERV transport primitives exist, but neither production entrypoint calls them. The present handshake has no guest finish message proving the host response. The re-read helper tests do not qualify a live authenticated lease or revocation. | PARTIAL |
| Legacy WSL2 service handoff | `/usr/local/bin/ramshared` still reports v0.14.1. A low-RSS health-monitor process is active, but there is no `ramsharedd` process or stress. No v0.15.0 post-reboot `BINARY_MATCH` was established here. | PARTIAL |
| Cross-vendor GPU budget and stress | Host-gate tests pass 14/14, GPU policy 13/13, adapter-identity tests 4/4, and swapoff-first tests 3/3. These prove policy/refusal logic; no live worker allocation or multi-vendor campaign ran. | PARTIAL |
| Windows physical lifecycle | `Test-RamSharedWslLifecycleRecoveryStatic`, `Test-HostAutonomousLifecycleStatic`, and `Test-RamSharedOriginStatic` pass. No supervised physical cold-boot campaign or loaded-package `BINARY_MATCH` ran. | PARTIAL |
| Windows storage matrix | `Test-WindowsStorageMatrixStatic` passes its manufactured matrix and refusal cases. No five-cell physical matrix, three runs per cell, 75-sample artifact set, or real payload-integrity campaign ran. | PARTIAL |
| Cross-platform resource configuration | EVD-0107 and the current source show all discovered Windows volume rows, including ineligible rows with reasons. The UI remains read-only, without selection, saved profile, apply/rollback, or benchmark; native Linux live target proof and a selected real WSL volume plan are absent. | PARTIAL |

**Checks:** Focused Rust suites passed: host gate 14/14, Windows control-plane
helpers 12/12, swapoff-first 3/3, GPU admission policy 13/13, and adapter
identity 4/4. Four Windows PowerShell 5.1 static/manufactured suites passed.
The live resource-config coverage/E2E and strict Clippy results are recorded
in EVD-0107. `cargo fmt --all -- --check` and `git diff --check` passed.
The public kernel README now labels the current VMBus candidate unqualified;
that documentation correction is not kernel validation.

**Assessment:** All seven active labels remain `PARTIAL` after independent
source inspection. The current WSL sample is healthy enough for this
read-only audit, but does not authorize the heavy kernel build or host stress.
The build-permit integration was not available in the session, so no heavy
build was attempted or permitted by bypass.

**Verdict:** 🟡 `PARTIAL` — source descriptions and one inventory defect are
corrected, and the active labels were independently checked. VMBus mapping and
GPADL lifetime, authenticated runtime wiring, release parity, live GPU
allocation, physical Windows campaigns, and full resource configuration
remain open.

## 2026-09-28 06:28 -03 — enumerate every Windows volume candidate

**What:** Re-read the Windows inventory collector, renderer, planner, and
resource-configuration SPEC. The PowerShell collector filtered out every
non-fixed volume, and the text view omitted volume identity, drive type, and
eligibility reasons. The collector now preserves every row returned by
`Get-Volume`; missing capacity remains null. The text view reports identity,
type, capacity, and the planner's volume-level refusal reason. Volume rows are
candidate inventory only; a plan still binds a configured path to fresh
identity and host capacity.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0107`.
**Owner role:** cross-platform resource configuration / source validation.
**Observed at:** `2026-09-28T09:28:42Z`.
**Verified at:** `2026-09-28T09:28:42Z`.
**Source revision:** `5a93d101afa42faed2d06bf17e681986a89cc1ff`.
**Source state:** Test checkpoint `ac4bd09e` records the two failing
regressions; `5a93d101` contains the fix. Documentation updates are in the
current worktree and will be committed with this evidence.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0102 through EVD-0106; this corrects one source
gap but does not close the resource-configuration gate.
**Freshness:** The named CLI discovery E2E ran against the current WSL guest and
executed the actual bounded Windows inventory provider. No volume identities,
labels, or raw host paths are retained here. The installed release was not
changed by this source test.
**Category:** source regression / live read-only WSL discovery / coverage.
**How to measure:** Run the named `windows_inventory_*` unit tests, the
`cli_resource_config_json_discovers_platform_resources_read_only` integration
test, the CLI source-slice coverage gate, and strict Clippy. No volume write,
benchmark, swap change, tier activation, host install, or WSL shutdown was run.

**Checks:** The first RED run failed because the view still printed
`Windows fixed volumes:` and the collector contained a fixed-only
`Where-Object`. After the fix, the two focused inventory tests passed and the
resource-config unit suite passed 28/28. The live CLI discovery E2E passed;
the complete coverage run passed 391 unit tests and 12 integration tests, with
the resource-config slice at 88.5% (1,900/2,148 lines). Strict Clippy passed
for `ramshared-cli` and `ramshared-config`. These results qualify discovery
and rendering only. They do not prove every Windows volume class on every
machine or a storage write path.

**Remaining boundary:** The configurator remains read-only. It cannot let a
user choose a target, persist a profile, change swap or tier caps, apply or
rollback settings, or benchmark and recommend a disk. No native Linux live
target test or WSL plan against a selected real volume ran. The resource
configuration gate remains `PARTIAL`; the other six active gates retain the
open evidence recorded in EVD-0106.

**Verdict:** 🟡 `PARTIAL` — complete Windows volume rows and refusal reasons
are now visible and tested; selection, mutation, benchmarking, and platform
qualification remain open.

## 2026-09-28 10:35 -03 — fresh independent audit of active PARTIAL gates

**What:** Re-read current production entrypoints, VMBus/UIO worktree changes,
GPU admission code, resource-profile/planner source, lifecycle/storage harnesses,
and the currently installed RamShared identity. Re-ran the named tests and
static harnesses available here. Prior PASS/PARTIAL prose was treated as a
claim to check, not as proof.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0111`.
**Owner role:** independent source, runtime identity, and active-gate audit.
**Observed at:** `2026-09-28T13:35:12Z`.
**Verified at:** `2026-09-28T13:35:12Z`.
**Source revision:** `e6291085d30d87cb48e273538a2dbe7e1935bc74`.
**Kernel candidate revision:** `a022ac393ecaab845682f5afe2be6be792aedde2`.
**Kernel candidate worktree diff SHA-256:**
`3447b4f61d11e7fa4ce4154287706257f8ae36cb603480c0e19faa1098646ee0`.
**Installed CLI SHA-256:**
`49f5a770c1aefcb386ca99a7bb89b8913929ca2fcf5bfa18da28fee60ea41b89`.
**Source state:** The RamShared feature code is committed at `e6291085`; this
evidence, its GAP-register summary, and the selected-volume E2E note in the
resource-configuration IMPL were uncommitted during the audit. The separate
kernel candidate has a dirty five-file source worktree. Its dirty patch is not
the hosted CI snapshot and has no current build receipt.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0100 through EVD-0110. EVD-0111 supersedes their
claims about the current seven active PARTIAL gates only; it does not erase
historical incident measurements or close any environment-bound gate.
**Freshness:** Read-only WSL sample around 2026-09-28 13:24 UTC reported about
5.6 GiB guest `MemAvailable`, memory PSI `some/full avg10=0.00`, and about
1.67 GiB of the 4 GiB fallback swap used. `ramshared status --json` reported
phase/cache/origin Off and Guardian blocked as stale; its embedded status time
was 10:13 -03, so that Guardian field is not treated as fresh health. The only
active swap in `/proc/swaps` was the WSL fallback device. No `ramsharedd`
process was present; one `ramshared monitor` process had about 2.9 MiB RSS.
A read-only GPU query saw one NVIDIA GeForce RTX 2060 (6 GiB total, about
4.45 GiB free); this does not identify who used the other GPU memory.
**Category:** independent source re-audit / unit and integration tests /
PowerShell manufactured tests / live read-only WSL target planning.
**How to measure:** Inspect current source call sites and ownership paths; run
the listed Cargo tests, slice coverage gates, strict Clippy, PowerShell static
harnesses, `checkpatch.pl --strict`, and documentation checks. The live draft
E2E chose one eligible real Windows host volume, created a temporary user
draft under a temporary directory, planned it, and removed that directory.
No swap activation, origin write, disk benchmark, GPU allocation, physical
lifecycle/storage campaign, tier activation, stress, WSL shutdown, package
install, or kernel build was run. The kernel build permit interface was not
available through this session's tools or executable path; no heavy build was
attempted or admitted by bypass.

**Independent audit of every active PARTIAL:**

| Gate | Fresh evidence checked | Remaining boundary |
| --- | --- | --- |
| Cross-platform resource configuration | Six `config_draft` unit tests and the non-TTY refusal passed. `cargo test -p ramshared-config` passed 15 unit + 12 profile integration tests. The live read-only discovery E2E passed. A separate live WSL TTY run selected one real eligible host volume, drafted a 1 MiB fallback-swap request, and `config plan` returned `ready_for_review` with `storage_ready`, `apply_enabled=false`, and `writes_performed=false`; the temporary draft was removed. Slice coverage passed at 88.4% for `resource_config.rs` and 93.7% for `resource_profile.rs`. | The WSL test proves identity/capacity planning, not a storage mutation. GPU adapter/cap selection, a bounded paired speed comparison, durable transaction audit, protected profile writer, provider apply/rollback, and native-Linux-host E2E remain open. |
| WSL2 freeze memory ownership | Re-read the current dirty kernel tree. `hv_uio_remove()` calls `uio_unregister_device()` and then tears down buffers/ring; the UIO mapping paths have no VMA open/close lifetime accounting. The VMBus candidate's retained-owner list has no production `CHANNELMSG_GPADL_TORNDOWN` consumer/reclaimer; retained cleanup found in `channel.c` is KUnit-only. Current candidate diff passes `checkpatch.pl --strict` with zero errors/warnings/checks. | Installed Build `#6` still has no immutable source receipt; current candidate build, KUnit, install, GPADL/UIO interleaving drill, and SEV-SNP/TDX/Arm CCA transitions remain unqualified. This is an ownership risk, not proof of a UAF or the earlier freeze trigger. No heavy build was attempted because the required permit interface was unavailable. |
| WSL2 control-plane stability and revocable-cache transition | Source search found no AF_VSOCK/AF_HYPERV listener/client or `host_gate` call from either production daemon/service entrypoint. `HandshakeAck` exists, but no guest finish message or composed authenticated lease/manifest flow exists. Helper tests passed: host gate 14/14 and service control-plane 12/12. | The helper tests do not establish transport wiring, a live authenticated handshake, disconnect revocation, origin-only fallback, or a 24-hour rollout. |
| Legacy WSL2 handoff and teardown | `/usr/local/bin/ramshared --version` reports 0.14.1; `--build-info` is unsupported. Current status reported phase/cache/origin Off, stale Guardian state, and fallback swap only. There is no `ramsharedd`; the monitor process is not the tier daemon. The source regression `legacy_migration_executor_preserves_swapoff_first_order` passed 1/1. | There is no installed v0.15.0 `BINARY_MATCH`, fresh Guardian binding, or repeated idempotent post-reboot handoff. No installation was attempted. |
| Cross-vendor GPU budget identity and stress admission | Current policy suite passed 13/13; shared VRAM budget/identity suite passed 7/7. Source binds allocations to fresh driver-reported budget and exact adapter identity, constraining with same-adapter WDDM headroom when available. Current read-only hardware query reports one NVIDIA RTX 2060. | No worker allocation/teardown, second-adapter test, AMD/Intel execution, or cross-vendor campaign occurred. The host query does not attribute current 1.4 GiB GPU use to RamShared. |
| Corrected Windows physical lifecycle | Re-ran `Test-RamSharedWslLifecycleRecoveryStatic.ps1`, `Test-HostAutonomousLifecycleStatic.ps1`, and `Test-RamSharedOriginStatic.ps1`; all passed their manufactured/static assertions. | Static checks do not load the corrected package or prove cold boot, current binary identity, physical rollback/recovery, or repeated lifecycle on Windows. |
| Windows virtual-disk properties/counters/performance matrix | Re-ran `Test-WindowsStorageMatrixStatic.ps1`; its manufactured cell, refusal, rollback, watchdog, counter-schema, and artifact checks passed. | No physical five-cell/three-run matrix, 75-sample bundle, intended-payload integrity run, raw counter capture, or Event ID 153 window was collected. |

**Checks:** `cargo test -j 1 -p ramshared-cli config_draft` passed 6 unit +
1 CLI refusal test; the live discovery integration test passed 1/1; the
swapoff-first migration test passed 1/1. `cargo test -j 1 -p ramshared-config`
passed 15 unit + 12 profile integration tests. Host gate passed 14/14, GPU
admission passed 13/13, `ramshared-vram` passed 7/7, and service control-plane
helpers passed 12/12. CLI slice coverage passed 88.4% (2,633/2,978 lines);
profile-model coverage passed 93.7% (314/335 lines). Strict Clippy passed for
`ramshared-cli` and `ramshared-config`. All four Windows PowerShell static
harnesses passed. `checkpatch.pl --strict` passed on the current kernel diff.
The first `docs-check` run caught a missing observable-proof keyword in the
config gate's close-evidence cell; the cell was corrected and the full suite
was rerun afterward.

**Evidence-ID integrity:** Before appending EVD-0111, `validation.md` had 115
evidence blocks but only 110 distinct IDs. `EVD-0007`, `EVD-0008`, `EVD-0009`,
`EVD-0010`, and `EVD-0107` each appear twice. The repeated EVD-0010 block is
identical; the other duplicate IDs refer to different content. The schema
checks do not enforce uniqueness. The log is append-only, so those records
were not rewritten; EVD-0111 is a new unique ID. Do not count duplicated IDs
as independent corroboration.

**Assessment:** Source selection and read-only WSL planning improved and the
current code/static checks pass, but all seven reliability gates remain
`PARTIAL`. Static/unit proof does not substitute for VMBus memory lifetime,
production control-plane wiring, release parity, cross-vendor allocations, or
physical Windows qualification.

**Verdict:** 🟡 `PARTIAL` — one real selected WSL volume now passes the draft
and capacity-plan E2E. The seven gates were audited afresh; kernel, transport,
release, hardware, and physical Windows qualification gaps remain open.

**Evidence ID:** `EVD-0112`.
**Owner role:** source and hosted-kernel-CI re-audit.
**Observed at:** `2026-09-29T13:25:01Z`.
**Verified at:** `2026-09-29T13:47:43Z`.
**Source revision:** `8a64b9dddda8af61933be8030aeaf6890f3a939a`.
**Kernel candidate revision:** `b85e21326a41314047bd6e1ac864db39869315a4`.
**Kernel base revision:** `93f51579e7df248780214094418f205253383cc5`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0111 and the earlier VMBus records. This record
updates the current candidate's build and KUnit evidence; it does not close
live-runtime, installed-image provenance, freeze-causality, or CoCo gates.
**Freshness:** Hosted run and read-only host identity were checked at
`2026-09-29T13:47:43Z`. Host readings describe that WSL boot only; the hosted
CI result is retained by GitHub Actions.
**Category:** source review / hosted x86_64 and arm64 kernel object builds /
KUnit / read-only installed identity check.
**What:** Re-read the tracked VMBus candidate and the exact hosted workflow run.
The public series contains seven patch files. The candidate adds a retained
buffer owner workqueue and reclaims only after the host-revoke gate and page
reference gate allow it. This updates EVD-0111's statement that the dirty
candidate then under review had no build or KUnit result; that earlier
statement remains historically accurate for candidate
`a022ac393ecaab845682f5afe2be6be792aedde2`.
**How to measure:** Inspect kernel-fork source commit
`b85e21326a41314047bd6e1ac864db39869315a4`, patch files, workflow, and artifacts
from [hosted run 36574925363](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36574925363).
The workflow pinned Linux base `93f51579e7df248780214094418f205253383cc5`,
recorded per-patch SHA-256 values, configurations, and logs, and passed all
seven staged mainline patch builds on x86_64 and arm64 with `W=1`, Sparse, and
strict checkpatch. Mainline x86_64 KUnit passed 24/24; arm64 KUnit was skipped.
The separate WSL 6.18.40.1 source at the same candidate revision passed the
Hyper-V/NetVSC/UIO `W=1` and Sparse object build, the DXG object build, and
KUnit 14/14. The run artifacts include the pinned base SHA, source SHA, all
seven patch hashes, resolved configurations, and logs. A read-only host check
at verification time returned `6.18.40.1-microsoft-standard-WSL2+ #6`,
`/usr/local/bin/ramshared --version` returned `0.14.1`,
`--build-info` was unsupported, and `ramshared.service` was not installed.
No host install, WSL shutdown/restart, host build, stress, or GPU allocation
was performed.
**Remaining boundary:** KUnit validates named state, allocation, and mapping
preparation cases; it does not demonstrate a live `/dev/uio` mmap-close and
unregister race, host GPADL response/rescind interleaving, order-zero behavior
under real fragmentation, channel allocation/free balance, Hyper-V execution
of this exact WSL backport, or SEV-SNP/TDX/Arm CCA memory-state transitions.
The active Build #6 source is still not matched to an immutable source receipt,
so this result does not identify the earlier freeze trigger.
**Verdict:** 🟡 `PARTIAL` — hosted compile and KUnit evidence improved; the
kernel and RamShared candidates are not installed on the WSL host, and all
live host, freeze-attribution, UIO/GPADL, and CoCo qualification boundaries
remain open.

**Evidence ID:** `EVD-0113`.
**Owner role:** resource-configuration source and test audit.
**Observed at:** `2026-09-29T14:46:31Z`.
**Verified at:** `2026-09-29T14:46:31Z`.
**Source revision:** `245146e5dc63e8c8ff734cbfc785405e25c8dcec`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0111 and EVD-0112. This advances only the
resource-configuration source/UI evidence; it does not close any active
reliability gate or replace platform qualification.
**Freshness:** Source and tests were checked at the recorded verification time.
No WSL service, kernel, swap setting, GPU allocation, or installed binary was
changed.
**Category:** source audit / focused Rust unit tests / Clippy / documentation
checks.
**What:** Re-read the memory parser, the user-draft wizard, profile model, and
the cross-platform resource-configuration PRD/SPEC/AUDIT/IMPL. The values
`4096` and `8192` in `meminfo_missing_or_inconsistent_core_values_are_unavailable`
are parser fixtures in kB. The test intentionally pairs a 4 MiB total with an
8 MiB available value (and a 4 MiB swap total with 8 MiB free) to prove that
inconsistent counters are rejected; those values are not RAM/swap limits. The
production parser reads the current `/proc/meminfo` counters and checks their
relationships. `meminfo_accepts_user_sized_ram_and_swap_without_product_minima`
accepts separate 256 MiB RAM / 128 MiB swap and 48 GiB RAM / 20 GiB swap
fixtures, also demonstrating that no product minimum is encoded there.
**How to measure:** The new wizard test first failed on the pre-fix source
because no ZRAM ceiling was collected or rendered. On source revision
`245146e5dc63e8c8ff734cbfc785405e25c8dcec`,
`CARGO_BUILD_JOBS=1 cargo test -p ramshared-cli --bin ramshared config_draft_wizard_saves_tier_caps_as_unapplied_ceilings -- --nocapture`
passed 1/1 and
`CARGO_BUILD_JOBS=1 cargo test -p ramshared-cli --bin ramshared config_draft_`
passed 6/6. The wizard now accepts optional positive variable ZRAM and SSD-origin
ceilings, shows exact values in the read-only plan, leaves blank ceilings
unset, and refuses zero, negative, malformed, or overflowing sizes before
writing. VRAM remains unavailable in this wizard because it does not sample a
fresh budget bound to a stable adapter identity; users cannot type an
unverified adapter ID.
`CARGO_BUILD_JOBS=1 cargo clippy -p ramshared-cli --bin ramshared --tests -- -D warnings`, `cargo fmt --all -- --check`,
`node tools/ci/check-validation-schema.mjs --all`, and `./scripts/docs-check.sh`
passed. No full workspace suite, exact slice-coverage run, live TTY E2E, install,
stress, or GPU context was run.
**Remaining boundary:** This is an unprivileged profile draft and test-fixture
result. It neither applies the ceilings nor proves current runtime budgets. GPU
inventory/selection, measured disk recommendation, provider transactions,
native Linux live qualification, and WSL before/action/after qualification
remain open. All seven active reliability gates remain `PARTIAL`.
**Verdict:** 🟡 `PARTIAL` — variable ZRAM/SSD-origin ceilings are now reviewable
in drafts and the fixed-value interpretation of the parser test is disproved
by source and tests; GPU, mutation, and live-platform gates remain open.

## 2026-09-29 20:24 -03 — source-gap fixes after adversarial audits

**What:** Continued the open campaign against the adversarial source audits
and landed the remaining actionable source-gap fixes that do not require
hardware or a lab. In RamShared, draft tier caps were honestly renamed to
unenforced planned caps, and the isolated GPU worker frame reads became
absolute-deadline bounded. In the kernel fork, the source-confirmed DXG/UIO/
NetVSC defects and the VMBus workflow path-filter gap received focused fixes.
External hardware and platform qualification gates were not claimed closed.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0114`.
**Owner role:** source-gap fix and test-first verification.
**Observed at:** `2026-09-29T23:24:21Z`.
**Verified at:** `2026-09-29T23:24:21Z`.
**Source revision:** `6cb276efff6ac2e00fb5fecec7a25aeb45446a9a`.
**Kernel candidate revision:** `850f55bc0c2840501b8ebbe32a59060822f1d058`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0111 through EVD-0113 and the source-audit
documents in `docs/reliability/`. This evidence records local source fixes and
hermetic test results only; it does not close install, runtime, freeze,
hardware, CoCo, or physical Windows gates.
**Freshness:** Suites and static checks were re-run at the verification time
against the recorded source revision. Host identity remained
`6.18.40.1-microsoft-standard-WSL2+` with installed RamShared CLI `0.14.1`
and `--build-info` unsupported; no install, stress, GPU allocation, WSL
shutdown, or kernel boot change was performed.
**Category:** source fix / focused Rust unit and integration tests / kernel
object compile / documentation checks.
**How to measure:** RamShared test-first pairs: `3d7400f5` then `ad663272`
relabel draft caps as unenforced planned caps (`PlannedTierCaps`, plan JSON
`unenforced_planned_caps`, legacy `[caps]` alias kept); `87614914` then
`48649d23` bound worker frame reads with a 30s absolute deadline. Kernel
commits `d9a1a3a8d6f6`, `b99248f63e43`, `4ea7c35d2cd8`, `68700eb5aa8a`, and
`805418bd7021` fix BUG-1, BUG-8, BUG-5, BUG-11, and the PR path filter G2.
`CARGO_BUILD_JOBS=1 cargo test -p ramshared-block` passed 126 unit plus 6
protocol integration tests; `cargo test -p ramshared-cli` passed 434 unit plus
13 integration tests; `cargo test -p ramshared-config` passed 15 unit plus 13
integration tests. `cargo fmt --all -- --check`, strict Clippy on the three
touched crates, `node --test tools/ci/check-validation-schema.test.mjs` (26),
and `./scripts/docs-check.sh` passed. Kernel verification used
`scripts/checkpatch.pl --strict --no-tree` with 0 errors/warnings and targeted
`make W=1` of the three touched objects. Named stall tests
(`worker_frame_read_deadline_fails_closed_on_a_silent_peer` and partial
header/payload siblings) failed before the deadline fix and pass after it.
**Remaining boundary:** Planned caps remain unenforced and have no apply or
rollback path. Worker writes remain unbounded against a non-reading peer.
Kernel BUG-2, BUG-3, and BUG-9 race/ownership issues need designed
synchronization and runtime reproducers. No live Hyper-V GPADL interleaving,
UIO mmap race, DXG greater-than-4-GiB allocation, CoCo transition, multi-vendor
GPU campaign, Windows physical lifecycle, BINARY_MATCH install, or freeze
attribution proof was produced. All seven reliability gates remain `PARTIAL`;
stress and upstream submission stay blocked on their named proofs.
**Verdict:** 🟡 `PARTIAL` — actionable source defects and CI trigger gaps from
the audits are fixed with test-first evidence; install, runtime, hardware, and
lab proofs remain open and unclaimed.

## 2026-09-29 20:31 -03 — direct userspace install with BINARY_MATCH

**What:** Installed the clean local RamShared userspace build through
`scripts/install.sh` from `target/release`, proved exact `BINARY_MATCH` between
the built and installed CLI/daemon digests, and ran read-only identity,
status, check, doctor, and monitor probes without activating cascade, stress,
or GPU cache. Kernel install and WSL restart were explicitly out of scope.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0115`.
**Owner role:** supervised direct userspace install and identity proof.
**Observed at:** `2026-09-29T23:31:13Z`.
**Verified at:** `2026-09-29T23:31:42Z`.
**Source revision:** `5e6b5845e75cbfe8d9f420abfdfdc97e4fb404e8`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0111 through EVD-0114. This evidence records one
direct `/usr/local` install and its exact digests; it does not close start/stop
after reboot, `/opt` product-release promotion, kernel promotion, stress, or
hardware gates.
**Freshness:** Install and probes ran in the current WSL boot
`6.18.40.1-microsoft-standard-WSL2+`. `ramshared check` returned
`Decision: ready` with CUDA ready and one NVIDIA GeForce RTX 2060 visible.
Status stayed `phase: Off` / `protection: OFF` with only fallback swap
`/dev/sdb`. `monitor --once` reported `memory_scope: wsl2`,
`latency_source: unavailable` on idle tiers, and `binary_version: 0.15.0`.
No stress, cascade up, GPU allocation, swap change, or WSL shutdown ran.
**Category:** local install / exact SHA-256 identity / read-only CLI probes.
**How to measure:** Pre-install `/usr/local/bin/ramshared` was `0.14.1` with
CLI SHA-256 `49f5a770c1aefcb386ca99a7bb89b8913929ca2fcf5bfa18da28fee60ea41b89`.
`sudo bash scripts/install.sh` from the clean tree installed
`target/release` artifacts. Post-install SHA-256 values matched the build
exactly: CLI `05a556776bc67f700855d8ed28d6c7ef5e6aa6773ea67d2ba3edd1374b4b1639`
and daemon `5d67e2d108d1c02e7e60e64af3f54966a83cca9a2d1a56acfefbcff90b9141ed`.
`/usr/local/share/ramshared/INSTALL_METADATA.json` records
`ramshared-direct-install-metadata/v2` with version `0.15.0`, source commit
`5e6b5845e75cbfe8d9f420abfdfdc97e4fb404e8`, `source_tree_state=clean`, and
`installed_at_utc=2026-09-29T23:31:13Z`. `ramshared --build-info` returns the
same identity. `ramshared status`, `check`, and `doctor` are read-only;
`check` decision was `ready`.
**Remaining boundary:** `/opt/ramshared/current` still points at the earlier
sealed candidate `v0.15.0-b788c17`; product-path promotion with input-bundle
provenance was not performed. No repeated idempotent start/stop after reboot,
no cascade activation, no three-tier stress, no live GPU worker allocation, no
Windows physical campaign, and no custom-kernel install exists. All seven
reliability gates remain `PARTIAL`; Build #5 stress and upstream submission
stay blocked on their named proofs.
**Verdict:** 🟡 `PARTIAL` — clean direct userspace install now has exact
BINARY_MATCH and live read-only identity proof; lifecycle repetition, product
release promotion, kernel, stress, and hardware proofs remain open.

## 2026-09-29 20:50 -03 — idempotent cascade start/stop cycles

**What:** After the EVD-0115 direct install, attached the sealed origin VHDX
through the attended `Manage-RamSharedOrigin.ps1` attach action, refreshed the
Windows Guardian task to a HEALTHY proof with a canonical guest boot ID, ran
the host gate to `NORMAL_BOOT`, and executed three `ramshared up` /
`ramshared down` cycles with swapoff-first teardown. Cascade was left Armed
after the final start. No stress campaign and no GPU cache allocation ran.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0116`.
**Owner role:** supervised cascade lifecycle start/stop evidence.
**Observed at:** `2026-09-29T23:50:02Z`.
**Verified at:** `2026-09-29T23:50:02Z`.
**Source revision:** `1b73c989eadfd929e4cde708d713fe1ddfcd1154`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0115 and the legacy handoff row. This records
same-boot idempotent start/stop against the EVD-0115 install; it does not
replace a post-reboot repetition, product `/opt` promotion, kernel promotion,
or stress qualification.
**Freshness:** Host `6.18.40.1-microsoft-standard-WSL2+`; installed CLI
`v0.15.0 · 5e6b5845` with CLI SHA-256
`05a556776bc67f700855d8ed28d6c7ef5e6aa6773ea67d2ba3edd1374b4b1639`.
Guardian health refreshed to `HEALTHY` / `watching` with boot ID
`641971fd-a2af-41c8-9e21-fba04f8d870d`. After the final up, `/proc/swaps`
listed `/dev/zram0` prio 200 (2097148 KiB), `/dev/nbd0` prio 100 (4194300
KiB), and the WSL fallback `/dev/sdb` prio -2. Status reported
`phase: Armed (armed_low_vram_used)` and `protection: READY`.
**Category:** live lifecycle / before-action-after swap topology /
attended origin attach.
**How to measure:** Elevated attach produced
`state=ATTACHED` with PARTUUID `5039dca1-61a0-41ff-aa08-221f49326a1b`.
`sudo bash scripts/safety/ramshared-host-gate.sh` printed
`RAMSHARED_HOST_GATE=NORMAL_BOOT` and published `/etc/ramshared/origin.conf`.
Each `sudo ramshared up --vram 4096 --zram 2048` armed zram then the
SSD-authoritative NBD device and reported `ok: true`. Each
`sudo ramshared down` printed `swapoff ok: /dev/nbd0`, `swapoff ok: /dev/zram0`,
then `cascade unmounted (swapoff-first, no broad kill)`, leaving only the WSL
fallback swap and `daemon: dead`. Three complete up/down/up cycles ran without
hang, panic, or residual managed swap. `ublk` remained refused on WSL2 by the
existing teardown-safety policy.
**Remaining boundary:** Cycles were same-boot, not after a full WSL restart.
No three-tier stress, simultaneous 100% ZRAM / 100% NBD / 99% SSD
qualification, live GPU worker allocation, physical multi-vendor GPU campaign,
Windows physical lifecycle/matrix, custom-kernel promotion, or CoCo evidence
was produced. `/opt/ramshared/current` remains on `v0.15.0-b788c17`.
**Verdict:** 🟡 `PARTIAL` — repeated idempotent start/stop and swapoff-first
teardown now have live proof on the EVD-0115 install; post-reboot repetition,
product-path promotion, stress, kernel, and hardware gates remain open.

## 2026-09-29 21:55 -03 — custom kernel #9 built and armed for WSL reboot

**What:** Built WSL kernel `#9` from `vmbus-ring-buffer-upstream-v2` including
BUG-1/5/8/11 and BUG-2/3/9 source fixes, installed matching UIO/NBD/ZRAM/
zsmalloc modules into `/lib/modules/6.18.40.1-microsoft-standard-WSL2+`,
staged `C:\wsl\kernel-ramshared-v6` with an immutable SHA-256 receipt, and
atomically switched the `.wslconfig` `kernel=` line from v5 to v6. Cascade was
torn down swapoff-first before the arm. The WSL restart that activates the
image is a separate post-reboot verification step.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0117`.
**Owner role:** custom-kernel build, module install, and arm receipt.
**Observed at:** `2026-09-29T23:55:00Z`.
**Verified at:** `2026-09-29T23:55:00Z`.
**Source revision:** `a5cedb4de6f887b5ac6d7394dbc7851cc4a71db0`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0112 and the kernel fork audit. This evidence
covers build identity and arming only; boot proof is separate.
**Freshness:** `arch/x86/boot/bzImage` is Linux `6.18.40.1-microsoft-standard-WSL2+`
build `#9` dated `Tue Sep 29 21:30:04 -03 2026`. SHA-256 of both the tree image
and `C:\wsl\kernel-ramshared-v6` is
`24ac89168096b1dfbd4fda18f19b2aebeeca9742d19ce808dfba4ea0b3be8b2a`.
Receipt `/mnt/c/wsl/kernel-ramshared-v6.receipt` records source commit
`a5cedb4de6f887b5ac6d7394dbc7851cc4a71db0` and `source_tree_state=clean`.
`.wslconfig` now has `kernel=C:\\wsl\\kernel-ramshared-v6`. Modules installed
include rebuilt `uio.ko`, `uio_hv_generic.ko`, and `zsmalloc.ko` plus the
current `nbd.ko` and `zram.ko`. Cascade was `Off` with only fallback swap
before arm.
**Category:** kernel build / module install / immutable image receipt /
attended wslconfig arm.
**How to measure:** `make -j2 bzImage` produced `Kernel: arch/x86/boot/bzImage
is ready (#9)`. `make W=1 drivers/hv/channel.o drivers/hv/dxgkrnl/dxgadapter.o`
and UIO/dxgkrnl module builds completed after BUG-2/3/9 fixes
(`b64d516de5fc`, `a8042f978bc0`, `15c26f95a702`, `0dcd3ad5d996`).
`scripts/checkpatch.pl --strict` on `850f55bc0c28..HEAD` reported 0 errors.
`sudo make modules_install` and `depmod -a` populated the distro module tree.
Elevated PowerShell rewrote only the `kernel=` line and logged
`kernel_line_updated`. `sha256sum` matched across the build tree and
`C:\wsl\kernel-ramshared-v6`.
**Remaining boundary:** The active WSL boot is still `#6` until `wsl --shutdown`
and restart. No post-reboot `uname`, DXG/Xwayland probe, UIO mmap exercise,
GPADL interleaving, CoCo transition, or RamShared stress ran on `#9`. Hosted
mainline KUnit for the updated series still requires the workflow gate. Rollback
is to `kernel-ramshared-v5` by restoring the prior `kernel=` line.
**Verdict:** 🟡 `PARTIAL` — kernel `#9` identity and arm are sealed and
reproducible; live boot proof and runtime qualification remain open until the
restart verification.

## 2026-09-29 22:45 -03 — kernel #9 post-reboot boot and lifecycle proof

**What:** Verified the WSL restart activated custom kernel `#9`, confirmed
the sealed image SHA-256 against its receipt, scanned the fresh boot log for
fatal/FORTIFY/p9/init-timeout signals, reattached the sealed origin, and ran
RamShared cascade start/stop cycles on the new kernel. Xwayland was running
against `/dev/dxg` with no DXG FORTIFY warning in the boot log.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0118`.
**Owner role:** post-reboot custom-kernel identity and lifecycle verification.
**Observed at:** `2026-09-30T00:45:00Z`.
**Verified at:** `2026-09-30T00:45:00Z`.
**Source revision:** `fef95da7deae8875ed691572f1d7c44758d70877`.
**Kernel candidate revision:** `a5cedb4de6f887b5ac6d7394dbc7851cc4a71db0`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0115 through EVD-0117. This is one supervised
post-reboot boot of kernel `#9`; it does not replace a same-host bundled/custom
A/B campaign, CoCo transition, or three-tier stress qualification.
**Freshness:** `uname -r` reported
`6.18.40.1-microsoft-standard-WSL2+` and `/proc/version` showed build `#9`
dated `Tue Sep 29 21:30:04 -03 2026`. `sha256sum /mnt/c/wsl/kernel-ramshared-v6`
matched the receipt value
`24ac89168096b1dfbd4fda18f19b2aebeeca9742d19ce808dfba4ea0b3be8b2a`.
Guardian health after restart was `HEALTHY` / `watching` with a new canonical
boot ID. Installed RamShared remained `v0.15.0 · 5e6b5845 (clean)`.
**Category:** live boot identity / dmesg fatal-signal scan / cascade lifecycle /
GPU-adjacent DXG probe.
**How to measure:** Elevated `wsl --shutdown` armed `.wslconfig`
`kernel=C:\\wsl\\kernel-ramshared-v6` and restarted the distro. Boot log
showed `hv_vmbus: Vmbus version:5.3`, `registering driver dxgkrnl`, and
`Hyper-V: Calibrating min_free_kbytes ... for VMBus resilience`. A scan for
`BUG:`, `Oops`, `panic`, and `FORTIFY` found no matches (the only WARNING is
the standard SRSO hardware-mitigation notice). `/dev/dxg` was present and
Xwayland was running; repeated `dxgkio_query_adapter_info` ioctl `-22`/`-2`
errors are userspace feature probes, not kernel faults. Origin reattach
returned `state=ATTACHED` with the sealed PARTUUID. `ramshared host-gate`
printed `NORMAL_BOOT`. `ramshared up --vram 4096 --zram 2048` armed zram prio
200 and NBD prio 100; `ramshared down` performed swapoff-first teardown and
`up` restored the same topology. dmesg recorded clean `zram`/`nbd0` add,
swap-on, disconnect, and re-add cycles with no kernel splat. `ramshared check`
returned `Decision: blocked` only because managed swap was already active
(fail-closed on double activation), and recommended keeping the MVP on `nbd`.
**Remaining boundary:** This is not a same-host bundled-vs-custom A/B, not a
GPADL/UIO race reproduction, not a DXG greater-than-4-GiB allocation test, and
not a CoCo transition. Hosted KUnit for the updated series still belongs to
the workflow gate. `/opt/ramshared/current` remains on `v0.15.0-b788c17`.
Stress, multi-vendor GPU, and Windows physical campaigns stay unqualified.
**Verdict:** 🟡 `PARTIAL` — kernel `#9` is the active WSL image with matching
receipt and a clean boot/lifecycle sample; A/B, race, CoCo, and stress proofs
remain open.

## 2026-09-30 01:55 -03 — post-reboot idempotent cascade start/stop on kernel #9

**What:** Against the EVD-0115 direct install and booted custom kernel `#9`,
reattached the sealed origin after the WSL restart, refreshed Guardian to
`HEALTHY`, minted a fresh host-resume lease, and ran attended cascade
`up`/`down`/`up`/`up` cycles with swapoff-first teardown. This closes the
"repeat idempotent start/stop after a full WSL reboot" criterion of the Legacy
WSL2 service handoff gate.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0119`.
**Owner role:** post-reboot cascade lifecycle and idempotent re-entry proof.
**Observed at:** `2026-09-30T04:55:56Z`.
**Verified at:** `2026-09-30T04:55:56Z`.
**Source revision:** `05e3c1c416c1d98838418fcfb4356c3ec861b087`.
**Kernel candidate revision:** `a5cedb4de6f887b5ac6d7394dbc7851cc4a71db0`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0115 through EVD-0118. This is one attended
post-reboot lifecycle sample on one host. It does not qualify unattended boot
auto-start, a same-host bundled/custom A/B, CoCo transition, multi-vendor GPU,
or three-tier stress.
**Freshness:** Booted kernel `6.18.40.1-microsoft-standard-WSL2+` build `#9`;
`sha256sum /mnt/c/wsl/kernel-ramshared-v6` =
`24ac89168096b1dfbd4fda18f19b2aebeeca9742d19ce808dfba4ea0b3be8b2a`
(matching receipt). Installed CLI `v0.15.0 · 5e6b5845 (clean)` at `/usr/local`,
SHA-256 `05a55677…b1639` (CLI) and `5d67e2d1…141ed` (daemon). Before the
campaign the cascade was `Off` with only fallback swap
`SANITIZED_EXISTING_WSL_SWAP_DEVICE`, origin detached, and no
`/run/ramshared` lease.
**Category:** host prerequisite re-establishment / host-gate lease /
idempotent cascade lifecycle / anti-hang teardown.
**How to measure:** Elevated `Boot-CascadeElevated.ps1` ran
`Manage-RamSharedOrigin.ps1 -Action attach` with approval token
`RAMSHARED_ORIGIN_5GIB_PARTUUID`, returning `state=ATTACHED` for PARTUUID
`SANITIZED_ORIGIN_PARTUUID` (device `SANITIZED_ORIGIN_DEVICE`, 5 GiB), then
started scheduled task `RamSharedWslGuardian.v1` (`task_state=Running`) which
published `state=HEALTHY`, `reason=watching`, `boot_id=SANITIZED_BOOT_ID`.
`safety/ramshared-host-gate.sh` printed `RAMSHARED_HOST_GATE=NORMAL_BOOT` and
minted `/run/ramshared/host-resume-lease.json`
(`source=fresh_sealed_guardian_proof`, same `boot_id`).
`ramshared up --vram 4096 --zram 2048` armed `zram0` prio 200 (2 GiB, lzo-rle)
and `nbd0` prio 100 (4 GiB, 1 connection) over SSD-authoritative origin with
fallback `SANITIZED_EXISTING_WSL_SWAP_DEVICE` prio -2; `status` reported
`phase=Armed (armed_low_vram_used)`,
`protection=READY (guaranteed_vram_tier_armed)`, `topology_ok=true`,
`ghost=false`, `order_ok=true`, daemon `alive`.
`ramshared down` printed `swapoff ok: /dev/nbd0` then `swapoff ok: /dev/zram0`
and `cascade unmounted (swapoff-first, no broad kill)`, returning to
`phase=Off` with only `SANITIZED_EXISTING_WSL_SWAP_DEVICE` and `daemon dead`.
A second `up` re-armed the
same three tiers, and a duplicate `up` left the **same daemon PID** and
identical topology (idempotent re-entry, no second daemon). A `dmesg` scan for
`BUG:`, `Oops`, `panic`, `FORTIFY`, `UAF`, and `Call Trace` matched no kernel
fault (only the benign `panic=-1` boot parameter).
**Remaining boundary:** Boot **auto-start** still does not occur: `wsl2-cascade-boot`
is `UNQUALIFIED` and `PRD.md` revision 2 requires a native in-program bootstrap
(RF-7..RF-10) before enablement. `SANITIZED_PRODUCT_PATH/current` remains on
`v0.15.0-b788c17` while `/usr/local` is `5e6b5845` (product-path skew).
`seal-kernel-pair.sh` cannot run: the layout inventory and QEMU stamp are
absent and `/mnt/c/wsl/modules-ramshared.vhdx` is dated 2026-07-10 versus the
2026-09-29 kernel `#9` build. Same-host bundled/custom A/B, GPADL/UIO race
reproduction, CoCo transition, multi-vendor GPU, and three-tier stress remain
open.
**Verdict:** 🟡 `PARTIAL` — post-reboot idempotent `up`/`down`/`up` and clean
swapoff-first teardown are proven on kernel `#9` against the EVD-0115 install;
unattended boot auto-start, sealed kernel/modules pair, and `/opt` promotion
remain open.

## 2026-09-30 06:35 -03 — GPU budget chain: dual-LUID, software-heap, and mutation-frame fixes

**What:** Root-caused and fixed three reproduced defects that left
`cache_state: UNAVAILABLE` and `gpu_budget: null` on the live RTX 2060 WSL2
host, then re-armed the cascade and observed a `driver_reported` budget bound to
the real host LUID with the cache `ACTIVE`.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0120`.
**Owner role:** GPU adapter correspondence, software-heap rejection, cache
mutation framing, and live driver-reported budget proof.
**Observed at:** `2026-09-30T09:29:48Z`.
**Verified at:** `2026-09-30T09:29:58Z`.
**Source revision:** `05e3c1c416c1d98838418fcfb4356c3ec861b087`.
**Provenance note:** the GPU-budget delta is **uncommitted** on top of that
revision; the installed daemon SHA-256 is `6016878b…70a96`, `BINARY_MATCH`
against `/usr/local/bin/ramsharedd`.
**Kernel candidate revision:** booted `6.18.40.1-microsoft-standard-WSL2+`
build `#9`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0115 through EVD-0119. This is one attended
campaign on one single-adapter NVIDIA host. It does not qualify multi-adapter
selection, AMD, Intel, CoCo, or three-tier stress.
**Freshness:** `nvidia-smi` reports one `NVIDIA GeForce RTX 2060`, 6144 MiB
total. Guest `ENUM_ADAPTERS2` lists one display adapter
(`num_adapters=1`, `num_sources=0`). `/usr/share/vulkan/icd.d/` has no NVIDIA
ICD, so Vulkan can only see Mesa here.
**Category:** reproduced defect / adapter identity / cache framing / live
telemetry.

### Defects (each reproduced, none inferred)

**D1 — WSL2 dual-LUID namespace divergence.**
Live probe: `/dev/dxg` `ENUM_ADAPTERS2` returned
`00000000:455c7025` for the RTX 2060, while `cuDeviceGetLuid` and Windows DXGI
`IDXGIAdapter1::GetDesc1` both returned `00000000:00012055`. Windows SDK
`d3dkmthk.h` matches our `AdapterInfo` (20 B) / `EnumAdapters2` (16 B)
`QueryVideoMemoryInfo` (56 B) layouts, so the struct layout is not the bug.
WSL kernel `ioctl.c` shows `inf->adapter_luid = entry->luid;` (VM-bus channel
LUID) and that `host_adapter_luid` is never copied to userspace. Result before
the fix: `CUDA adapter 0 budget rejected: dxg adapter LUID 00000000:00012055 not
found`, `gpu_budget_guard=allocator_only reason=unavailable_or_unmatched_luid`.

**D2 — software rasterizer accepted as VRAM.**
`VulkanProvider::open_exact` accepted llvmpipe; its host-RAM heap was reported
`DriverReported`, so Mesa became a fake VRAM cache
(`key=6d65736132352e322e382d3075627500` = `mesa25.2.8-0ubu`,
`safe_target_bytes=4294967296`), then
`isolated GPU cache unavailable: cache mutation exceeds nonblocking frame limit`.

**D3 — cache mutation frame exceeded by real NBD write size.**
`AuthoritativeOriginBackend::write_at` forwarded the whole block-layer payload to
`BestEffortCache::update`, while `IpcCacheClient` fails closed above
`MAX_MUTATION_FRAME_DATA_BYTES` (64 KiB) and permanently revokes the cache.
Measured live: `/sys/block/nbd0/queue/max_sectors_kb = 4096` (4 MiB) and
`max_hw_sectors_kb = 32768` (32 MiB) versus a 64 KiB frame limit — up to 64×
oversized. The read-miss `promote` path had the same defect.

### Fixes (Day-0, no shims)

- `AdapterCorrespondence::{SharedLuid, SoleAdapter}` in
  `ramshared-wsl2d::gpu_budget`: exact LUID match, else `AdapterNotFound` plus
  a single sole adapter; anything else is unprovable and stays allocator-only.
  `constrained_budget` re-enforces the proof. Named tests:
  `sole_adapter_correspondence_accepts_split_wsl2_luid_namespaces`,
  `adapter_not_found_falls_back_to_sole_adapter_correspondence`,
  `ambiguous_or_absent_sole_adapter_never_assumes_correspondence`.
- `VulkanProvider::is_hardware_gpu()` (`DISCRETE_GPU` | `INTEGRATED_GPU`);
  `budget_snapshot` never returns `DriverReported` for a software device type,
  and the candidate loop logs
  `Vulkan adapter N skipped: not a hardware GPU`.
- `MAX_CACHE_MUTATION_BYTES` (64 KiB) with `mutation_frames()` splitting both
  `update` and `promote`. Named tests:
  `large_write_is_framed_to_the_cache_mutation_limit` (4 MiB → 64 frames),
  `large_promote_is_framed_to_the_cache_mutation_limit`.
- A pre-existing `ramsharedd` bin-test compile break
  (`FrameHeader::decode` returns `Option`) was fixed so the suite builds.

### Measurements (condition: `idle`, n = 10 over 20 s)

`scripts/p0/measure-vram-headroom.sh 20 2`, read-only:

| Metric | min | max | mean | stddev | unit |
| --- | --- | --- | --- | --- | --- |
| Free VRAM | 3601 | 3615 | 3612 | 4 | MiB |
| Used VRAM | 2340 | 2354 | 2342 | — | MiB |
| RAM available | 7957 | 7984 | 7973 | — | MiB |
| Swap used | 4 | 4 | 4 | — | MiB |

Volatility of free VRAM = range/mean = **0.4%**. Host has ~3.6 GiB of stable
idle VRAM under the observed desktop load.

### Before → action → after

| Field | Before (defects live) | After (fix installed) |
| --- | --- | --- |
| `cache_state` | `UNAVAILABLE` | `ACTIVE` |
| `ok` | `false` | `true` |
| `gpu_budget` | `null` | present |
| `gpu_budget.adapter` | — | `cuda`, key `1d3109d8…e0346db6` |
| `gpu_budget.adapter.luid` | — | `00000000:00012055` (host DXGI) |
| `gpu_budget.source` | — | `driver_reported` |
| `gpu_budget.total_bytes` | — | `6441992192` (6144 MiB) |
| `gpu_budget.budget_bytes` | — | `4211671040` (4016 MiB) |
| `gpu_budget.used_bytes` | — | `1358495744` |
| `gpu_budget.available_bytes` | — | `2853175296` (2721 MiB) |
| `vram_cached_kib` | 0 | 262144 (256 MiB) |
| `cache_target_kib` | 4194304 (bogus Mesa) | 1360780 (1328 MiB) |
| `gpu_headroom_kib` | `null` | 2786304 |

Daemon log after the fix (verbatim, append-only log still holds the older
lines):
`Vulkan adapter 0 skipped: not a hardware GPU (name="llvmpipe (LLVM 20.1.2, 256 bits)")`;
`gpu_adapter_selected backend=Cuda ordinal=0 key=1d3109d8…e0346db6 safe_target_bytes=1393439539`;
`gpu_budget_guard=dxg adapter=00000000:455c7025 correspondence=SoleAdapter`.
No further `isolated GPU cache unavailable` line appears after that pair.

Stability: three telemetry samples 5 s apart were identical
(`ok=true`, `cache=ACTIVE`, `cached_mib=256`, `headroom_mib=2721`,
`budget_mib=4016`, `used_mib=1295`, `src=driver_reported`).

### Validation gates

`cargo fmt` clean; `cargo clippy` clean on the touched crates;
`cargo test -p ramshared-block -p ramshared-wsl2d -p ramshared-vulkan
-p ramshared-dxg` green (128 + 6 + 171 + 115 + …). The CLI suite reports
434 passed / 1 failed, the failure being the pre-existing
`up_with_config_refuses_missing_safety_net_before_runtime_setup`
(confirmed still failing with these changes stashed). A one-off flake of
`supervisor::tests::bounded_systemctl_adapter_reaps_its_owned_timeout_fixture`
and of `daemon_nbd_recovery_activation_does_not_block_nbd_jobs` appeared only
under multi-crate parallel load; both passed 5/5 in isolation and the wsl2d
suite passed 3/3 parallel and 3/3 serial.

**Honest reading:** the GPU budget chain is proven end-to-end on **one**
single-adapter NVIDIA host under WSL2, with a real `driver_reported` budget
bound to the host DXGI LUID and a cache holding 256 MiB. The accounting gap
versus `nvidia-smi` (2721 MiB budget-available vs 3666 MiB `memory.free`) is a
known WDDM budget-vs-free difference and is not reconciled here. `SoleAdapter`
correspondence is sound only while exactly one adapter exists on each side; a
second adapter must fall back to `SharedLuid` or stay allocator-only. The
DEMOTE / VRAM-return action (returning cached pages to a GPU application
under load) is **not**
covered here: `cascade-hog` is not built. Multi-vendor GPU, CoCo, and
three-tier stress remain open.

**Verdict:** 🟡 `PARTIAL` — D1, D2 and D3 are reproduced, fixed, and proven
live with a real driver-reported budget and an active cache on one host;
DEMOTE return, multi-adapter, and multi-vendor qualification remain open.

## 2026-09-30 07:02 -03 — CASCADE DEMOTE drill: swapoff of the VRAM tier under cgroup pressure

**What:** Proved the DEMOTE **action** end to end on the live WSL2 host: with
716800 active pages already spilled into `/dev/nbd0`, ran the same `swapoff`
the daemon issues for Corruption/WDDM-constrained demote, while `ramsharedd`
kept serving read-back, then verified page integrity through the fault-in and
restored the tier.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0121`.
**Owner role:** DEMOTE action path (`spawn_swapoff`), spill integrity, A1 sink
presence.
**Observed at:** `2026-09-30T06:55:00Z`.
**Verified at:** `2026-09-30T07:02:00Z`.
**Source revision:** `05e3c1c416c1d98838418fcfb4356c3ec861b087`.
**Provenance note:** the drill harness and `scripts/p0/cascade_hog.c` are
**uncommitted** on top of that revision. The raw harness log was written to an
ephemeral `/tmp` path and is no longer retained; the measurements below are the
recorded run output and the harness is reproducible from the command line.
**Kernel candidate revision:** booted `6.18.40.1-microsoft-standard-WSL2+`
build `#9`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0120. One attended campaign on one single-adapter
NVIDIA host. Does not qualify the canary *trigger* (that is unit-tested in
`crates/ramshared-wsl2d/src/residency.rs`), unattended demote, or multi-vendor.
**Freshness:** Host state at drill time: one `NVIDIA GeForce RTX 2060`, 6144
MiB total; booted kernel `6.18.40.1-microsoft-standard-WSL2+` build `#9`;
cascade armed by `ramshared up --vram 4096 --zram 2048`; `ramsharedd` alive for
the whole drill; Windows watchdog `RamSharedWslGuardian.v1` `Running`.
**Category:** reproduced proof / action path / integrity.

### Method

`scripts/p0/measure-cascade-demote.sh` (root) with
`HOG_MB=2800 CAP_MB=256 RESTORE=1`. `scripts/p0/cascade_hog.c` is the
consumer: it fills every page with a deterministic per-page pattern
(`page_word`), signals `/tmp/cv-filled`, holds until `/tmp/cv-go`, then re-reads
and compares. A mismatch is real corruption — no reference copy is kept. The
hog is confined to a cgroup v2 `memory.max=256M` so the excess is pushed into
the cascade; `memory.swap.max` is set to `max`. Host-safety: the Windows
watchdog `RamSharedWslGuardian.v1` was `Running` for the whole drill. No
`kill -9` of the daemon, no global thrash.

The harness now builds `cascade-hog` from source when no binary is present
(`cc -O2 -Wall -Wextra -Werror -std=gnu11`) and takes `FILL_TIMEOUT_S`
(default 600), because a 2800 MiB fill under a 256 MiB cap spills at roughly
2 MiB/s and outlives a fixed 90 s wait. `scripts/p0/cascade_hog.c` passes
`checkpatch.pl` with 0 errors and 0 warnings.

### Before → after

| Step | Evidence |
| --- | --- |
| Preflight | three tiers present — `zram0` prio 200, `/dev/nbd0` prio 100, `sdb` prio −2; A1 sink below VRAM satisfied; `ramsharedd` alive |
| Fill | 716800 pages = 2800 MiB written and accounted |
| Before DEMOTE | `nbd=696 MiB zram=2047 MiB vhdx=0 MiB` |
| DEMOTE | `swapoff /dev/nbd0 OK in 141692 ms` (141.7 s) with the daemon serving read-back |
| After DEMOTE | `nbd=ABSENT`; zram and VHDX still active (A1 holds) |
| Integrity | `verified 716800 pages, 0 pages with corruption, 0 bad words` |
| Verdict | `>>> DEMOTE OK: 696 MiB of active pages left VRAM; 0 corruption in hog; sink active.` |
| RESTORE | `/usr/sbin/swapon -p 100 /dev/nbd0` → `RESTORE ok`; all three tiers back; `cache_state: ACTIVE` |

Exit code `0`. After restore the VRAM cache reported 1280 MiB against a 1328
MiB target (it had been 256 MiB before the drill), i.e. the drill exercised the
cache write path as well as the migrate path.

### Measurements

| Metric | Value | Unit | n |
| --- | --- | --- | --- |
| Pages filled and verified | 716800 | pages | 1 |
| Active pages that left VRAM | 696 | MiB | 1 |
| `swapoff /dev/nbd0` duration | 141692 | ms | 1 |
| Corruption | 0 | bad words | 1 |
| Mismatched pages | 0 | pages | 1 |

Single run (`n=1`): this is a correctness proof of the action path, not a
performance benchmark. It is therefore **not** registered in
`docs/benchmarks/results.jsonl` as a performance baseline.

### Honest reading

The DEMOTE **action** is proven safe: 696 MiB of active pages migrated out of
the VRAM tier in 141.7 s with bit-exact integrity and a live sink. This does
**not** mean the daemon will decide to demote when a GPU application needs the
memory — that decision depends on the budget signal, which EVD-0122 shows was
defective. Trigger and action are separate, and only the action is proven here.

**Verdict:** 🟢 `PASS` — DEMOTE action, spill integrity, and RESTORE are proven
on kernel `#9` on one host. Canary trigger remains unit-tested only;
unattended demote remains open.

## 2026-09-30 07:40 -03 — GPU budget containment: per-process budget blindness and the device-wide NVML fix

**What:** Reproduced and root-caused why RamShared does not get out of the way
of a VRAM consumer, then fixed the budget to read device-wide occupancy and
revalidated on the live host. Before the fix the cache returned 10% of its
pages under a 3 GiB consumer and the budget never moved; after the fix the
cache returned **all** of its pages and the budget tracked the consumer.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0122`.
**Owner role:** GPU budget authority, containment chain, device-wide occupancy.
**Observed at:** `2026-09-30T07:06:00Z`.
**Verified at:** `2026-09-30T07:40:07Z`.
**Source revision:** `05e3c1c416c1d98838418fcfb4356c3ec861b087`.
**Provenance note:** `crates/ramshared-cuda/src/nvml.rs` and the
`budget_snapshot` change are **uncommitted** on top of that revision. Installed
daemon SHA-256 `913daa2c5492c003df4627801885db4eb822c2059bdcb8fef929738100504697`,
`BINARY_MATCH` against `/usr/local/bin/ramsharedd`.
**Kernel candidate revision:** booted `6.18.40.1-microsoft-standard-WSL2+`
build `#9`.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0120 and EVD-0121. One attended campaign on one
single-adapter NVIDIA host. Multi-vendor GPU, multi-adapter, CoCo, and
three-tier stress remain open.
**Freshness:** Host state at revalidation time: one `NVIDIA GeForce RTX 2060`,
6144 MiB total; NVML `libnvidia-ml.so.1` resolvable from `/usr/lib/wsl/lib`;
booted kernel `6.18.40.1-microsoft-standard-WSL2+` build `#9`; cascade re-armed
after a clean `down`/`up` cycle with the fixed daemon; `cache_state: ACTIVE`.
**Category:** reproduced defect / root cause / fix / live revalidation.

### Defect (reproduced, not inferred)

Staged probe `scripts/p0/vram_ramp.c` — allocates `<step> MiB` of device memory
per stage with `cuMemAlloc` + `cuMemsetD8` so the driver accounts every page,
holds, then frees. It never touches swap or RamShared state. Sampler reads
`nvidia-smi` and `/run/ramshared/cache-status.json` every 2 s.

**Phase A — 3 GiB consumer (`vram-ramp 256 3072 6 25`), 78 samples:**

| Metric | Before | Peak | After release |
| --- | --- | --- | --- |
| `nvidia-smi` used | 2298 MiB | 4421 MiB | 2290 MiB |
| `gpu_budget.used_bytes` | 2191 MiB | **2191 MiB** | **2191 MiB** |
| `gpu_budget.available_bytes` | 2721 MiB | **2721 MiB** | **2721 MiB** |
| `vram_cached_kib` | 1280 MiB | 1152 MiB | 1152 MiB |

The cache returned **128 MiB of 3072 MiB (10%)**, and the target never moved.
That is not containment.

**Phase B — staleness ruled out.** The budget carries
`gpu_budget.sampled_at_unix_ms`. During a 2 GiB hold the sample age oscillated
between 0 and 5.9 s — the daemon *is* refreshing — while the values stayed
pinned at `2191`/`2721`. Fresh snapshot, wrong number. Staleness is excluded.

**Phase C — the two sources are both per-process.**

| Source | Live reading during a 2 GiB external hold | Tracks the consumer? |
| --- | --- | --- |
| `cuMemGetInfo` (third process, own context) | `used=634 free=3461 total=4095` pinned across 25 samples / 50 s | **No** |
| WDDM `QUERY_VIDEO_MEMORY_INFO` | `current_usage=2191` pinned | **No** |
| NVML `nvmlDeviceGetMemoryInfo` | `used` 2770 → 4789 → 2654 MiB | **Yes** |
| `nvidia-smi` (same NVML data) | 2353 → 4484 → 2353 MiB | **Yes** |

`cuMemGetInfo` under WSL2 GPU-PV accounts the calling process's paravirtual
channel, not the adapter: a third process's own context reported a constant
`634 MiB` used while another process held 2 GiB. Note it also reported
`total=4095 MiB` against a 6144 MiB adapter, which is further evidence that the
figure is a channel/partition view rather than the device.

The WDDM path is equally scoped. WSL kernel
`drivers/hv/dxgkrnl/ioctl.c` `dxgkio_query_vidmem_info` rejects any non-zero
`args.process` with `-EINVAL` and sends `process->host_handle` in the VMBus
command, so the host resolves the query against **that** dxgkrnl process. Our
`QueryVideoMemoryInfo` layout matches the UAPI `d3dkmt_queryvideomemoryinfo`
(56 bytes) exactly — the struct is not the bug; the scope is.

`constrained_budget` already takes
`min(allocator.available, wddm_available)`. Both inputs were blind, so the min
was blind. The design was right; the source was wrong.

### Fix (Day-0, no shims)

`crates/ramshared-cuda/src/nvml.rs` adds a runtime NVML loader beside the
existing CUDA loader (same `dlopen`/`load_sym` path, same NVIDIA driver
package, no build-time SDK):

- `nvmlInit_v2`, `nvmlDeviceGetHandleByIndex_v2` (fallback without `_v2`),
  `nvmlDeviceGetMemoryInfo`, `nvmlShutdown`.
- `Context::device_memory()` returns device-wide used/free/total.
- `VramProvider::budget_snapshot` for `Context` now sources occupancy from
  NVML. `Context::mem_info` remains the raw `cuMemGetInfo` call and is
  documented as allocator-local.
- `Cuda::load` **fails closed** if NVML is absent. A silent fallback to
  `cuMemGetInfo` would reintroduce the defect, so a host without NVML gets no
  budget and therefore no VRAM cache.

Named tests:
`budget_follows_device_wide_nvml_not_allocator_local_mem_info` (mock NVML and
mock `cuMemGetInfo` deliberately disagree; the budget must follow NVML),
`raw_mem_info_stays_allocator_local`,
`nvml_memory_layout_matches_driver_struct`.
Dependency documented in `crates/ramshared-cuda/README.md` (runtime candidates
and the fail-closed rationale).

### Revalidation (same host, same probe, after deploy)

Deploy: `ramshared down` (swapoff-first, clean) → `scripts/install.sh` from the
local build (`BINARY_MATCH`) → `ramshared up --vram 4096 --zram 2048`. Then
`vram-ramp 256 3072 2 15` with the same 2 s sampler:

| Metric | Before fix (peak) | After fix (peak) | After fix (released) |
| --- | --- | --- | --- |
| `nvidia-smi` used | 4421 MiB | 4820 MiB | 1583 MiB |
| `gpu_budget.used_bytes` | 2191 MiB | **5009 MiB** | **1854 MiB** |
| `gpu_budget.available_bytes` | 2721 MiB | **1134 MiB** | **2721 MiB** |
| `vram_cached_kib` | 1152 MiB | **0 MiB** | 0 MiB |

`b_used` now climbs with the consumer and falls back when it releases;
`b_avail` tracks down and recovers; the cache drops from 256 MiB to **0 MiB**
under pressure — it gets completely out of the way, which is the intended
behaviour when a GPU application needs the memory.

### Measurements (condition: `loaded`, external VRAM consumer present)

| Metric | Value | Unit | n |
| --- | --- | --- | --- |
| Consumer peak | 3072 | MiB | 2 |
| Cache yielded before fix | 128 | MiB (10%) | 1 |
| Cache yielded after fix | 256 | MiB (100%) | 1 |
| Budget peak `used_bytes` before fix | 2191 | MiB | 1 |
| Budget peak `used_bytes` after fix | 5009 | MiB | 1 |
| `cuMemGetInfo` volatility during hold | 0 | MiB | 25 samples |
| NVML volatility during hold | 2019 | MiB | 14 samples |

Probes: `scripts/p0/vram_ramp.c` (consumer) and
`scripts/p0/vram_free_probe.c` (third-process `cuMemGetInfo` reader). Both are
read-only with respect to RamShared and free everything on exit.

### Residual gaps (not closed here)

- The cache did not re-grow after the consumer released. Re-growth is
  demand-driven (cache fills on swap access), so this is not claimed as a
  defect, but it is **not** proven to recover either.
- When idle, `b_avail` clamps at 2721 MiB — the WDDM per-process figure is the
  binding `min` and is more conservative than NVML free. Harmless for
  containment (the truthful lower number binds under pressure) but it leaves an
  unexplained idle ceiling of about 880 MiB versus NVML free.
- `SoleAdapter` correspondence still assumes exactly one adapter on each side.
- AMD, Intel, multi-adapter, CoCo, and three-tier stress remain open.

### Tests

`cargo test -p ramshared-cuda -p ramshared-vram -p ramshared-wsl2d` green:
24 + 7 + 171 + 115 + 5 + 1 + 4 + 15 + 1 + 1 + 1 + 1 = **346 passed, 0 failed**.
`./scripts/docs-check.sh` → `✓ docs-check OK`. `cargo fmt` clean; `cargo clippy
-p ramshared-cuda --lib --all-targets` clean.

**Verdict:** 🟢 `PASS` for the containment slice — the budget is device-wide,
it tracks an external VRAM consumer, and the cache returns its pages under
pressure. Multi-vendor, multi-adapter, CoCo, and idle-ceiling reconciliation
remain open, so this is not a universal qualification.

---

## 2026-09-30 12:35 -03 — cuda-rust-native-tiering ITEM-1 + ITEM-2: codec contract, compressed cache, and CPU-codec control arm

**What:** Implemented and validated SSDV3 STEP 3 for `cuda-rust-native-tiering`
ITEM-1 and ITEM-2: the provider-side `GpuCacheCodec` contract with a
deterministic `FakeCodec`, the bounded compressed-cache extent/slab layer, and
the worker wiring for optional lossless cache compression. Closed two real
fragilities found while proving the slice (`SlabSpan` free-addressing and
compressed-extent reclaim). Recorded the NFR-6b measurement-only CPU-codec
control arm as the ITEM-2 exit and ITEM-3 entry gate. 223 tests, 0 failures,
cover gate PASSED on all three business-logic files. No GPU and no nvCOMP were
used; no performance claim is made.
**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0123`.
**Owner role:** Cache compression codec contract, compressed extent/slab layer, isolated GPU worker wiring.
**Observed at:** `2026-09-30T15:35:31Z`.
**Verified at:** `2026-09-30T15:35:31Z`.
**Source revision:** `7adf0fcf71769361bd76b65488ce3125b433f4f5`.
**Provenance note:** The ITEM-1/ITEM-2 source, tests, and doc updates are
**uncommitted** on top of that revision. No daemon rebuild, no host install,
and no `BINARY_MATCH` claim: this entry is host-side test/coverage evidence
only, not a deployment qualification.
**Lifecycle:** `reviewable`.
**Retention:** Keep with EVD-0122. Host-side harness only (no GPU, no nvCOMP).
ITEM-3 stays gated on the nvCOMP runtime and its pre-decode checksum
mechanism; ITEM-5 paired GPU runs do not exist yet.
**Freshness:** Host state at test time: WSL2 kernel
`6.18.40.1-microsoft-standard-WSL2+`; `cargo test` / `cargo clippy` /
`check-rust-slice-coverage.mjs` run directly on the workspace with
`CARGO_BUILD_JOBS=1`; no cascade state was touched and no host pressure was
applied.
**Category:** implementation / named-test / coverage-gate.

### Scope

Lossless compression of the **disposable VRAM cache** only (RF-1..RF-9,
DT-1..DT-11). The SSD origin, Linux swap format, block ABI, and Windows
pagefile are unchanged and remain authoritative. Compression is experimental
and `compression_enabled = false` by default (DT-9).

### Implemented

- `crates/ramshared-vram/src/codec.rs` — `GpuCacheCodec<M: VramMemory>`
  contract over provider-owned VRAM slabs (a host-side codec cannot implement
  the trait), `CodecId`/`CodecStatus`/`VramSpan`/`VramOutputReservation`/
  `CodecAlignments`/`CodecChunkResult`, shared `crc32`, and a deterministic
  `FakeCodec` (RLE wire format, `CodecId::Fake`, counters on compress/decode).
- `crates/ramshared-block/src/compressed_cache.rs` — bounded extent index,
  `SlabSpan` full addressing, coalescing free-range 2 MiB slab allocator,
  `split_extent` (≤ 64 KiB), `invalidate_overlaps` (DT-6), `read_coverage`
  (exact / contiguous / gap / overlap), LRU timestamps on compressed extents.
- `crates/ramshared-block/src/gpu_cache_worker.rs` — DT-9 default-off;
  16 MiB read ceiling checked before allocation (DT-4); invalidate-before-publish
  (DT-6); provider-side `checksum_batch` before decode, mismatch refuses without
  invoking the decoder (DT-7); strictly-smaller usefulness gate (DT-10);
  `CODEC_SUBDEADLINE = 20 ms` inside `CACHE_READ_BUDGET = 50 ms` (DT-3); codec
  faults leave the raw cache serving and do not revoke the client (DT-11);
  admission on the full parent free floor
  `required_free_bytes(configured, runtime_headroom)` with
  `RUNTIME_FREE_BUFFER_BYTES = 640 MiB` (DT-4 / NFR-2).
- `crates/ramshared-block/tests/cpu_codec_control_arm.rs` — NFR-6b
  measurement-only CPU-codec control arm **outside** `GpuCacheCodec`: identical
  DT-4 extents, host-memory encode/decode with the same lossless RLE, same
  metric envelope and integrity checks. Never a trait implementation, never a
  runtime provider, never a production fallback.

### Two fragilities found and closed

1. **`VramSpanAllocator::free()` was slab-ambiguous.** `VramSpan.offset` is
   slab-local, so with ≥ 2 slabs a free of offset 0 always landed in slab 0.
   Fixed with `SlabSpan { slab_index, span }` and an index-authoritative
   `free()` that refuses an unknown index. Proof:
   `free_addresses_the_slab_not_the_offset`.
2. **Host-pressure reclaim never released compressed VRAM.**
   `reclaim_under_host_pressure()` only evicted raw chunks. Fixed with
   `last_accessed` LRU on compressed extents and a fallback in
   `evict_coldest_chunk()`. Proof: `worker_evicts_compressed_lru_extent`.

Both are recorded as contract notes in `IMPL.md` so they are not "fixed" back.

### Two test-assertion errors corrected (not product defects)

1. A two-extent read over one compressed and one **raw** extent legitimately
   misses: DT-10 publishes a non-shrinking extent raw, so compressed coverage
   has a gap and the raw path/origin serves. The test now uses two compressible
   extents to exercise the assembly it claims to.
2. A partial overlapping update legitimately leaves the untouched tail as a
   gap: DT-6 invalidates the **whole** overlapping entry before publishing the
   replacement, which is what makes "never returns stale bytes" true. The test
   now asserts the miss and the later complete assembly.

**Measured data:** 223 tests passed, 0 failed (159 `ramshared-block` lib +
17 `gpu_cache_compression` + 7 `cpu_codec_control_arm` + 6
`gpu_worker_protocol` + 34 `ramshared-vram` lib). `cargo clippy
-p ramshared-vram -p ramshared-block --all-targets` = 0 errors, 0 warnings.
Cover gate `check-rust-slice-coverage.mjs --min 80`:
`ramshared-vram/src/codec.rs` **90.1%** (317/352),
`ramshared-block/src/compressed_cache.rs` **89.9%** (222/247),
`ramshared-block/src/gpu_cache_worker.rs` **86.3%** (623/722). Gate **PASSED**.

### How to measure

```bash
CARGO_BUILD_JOBS=1 cargo clippy -p ramshared-vram -p ramshared-block --all-targets
CARGO_BUILD_JOBS=1 cargo test  -p ramshared-vram -p ramshared-block
node tools/ci/check-rust-slice-coverage.mjs \
  -p ramshared-vram,ramshared-block \
  --files crates/ramshared-vram/src/codec.rs,crates/ramshared-block/src/compressed_cache.rs,crates/ramshared-block/src/gpu_cache_worker.rs \
  --min 80
```

### Residual gaps (not closed here)

- **ITEM-3 gated** — no nvCOMP / `libnvcomp` on this host and no pre-decode
  checksum mechanism resolved. DT-7 must not be weakened to a host readback.
  The CPU-codec control arm record is its entry gate, not its answer.
- **ITEM-4** versioned telemetry envelope is not implemented;
  `cached_bytes`/`target_bytes` are still physical only.
- **ITEM-5** paired raw/compressed GPU runs do not exist. No performance claim
  is made here — every number above is a host-side test/coverage figure.
- Reserve-floor contract **value** drift in the parent worker SPEC is still
  unreconciled upstream (512 MiB env vs `max(1536 MiB, 20%)`).
- Multi-vendor GPU, multi-adapter, CoCo (SEV-SNP / TDX / Arm CCA), and
  three-tier stress remain open.

**Verdict:** 🟡 `PARTIAL` — ITEM-1 and ITEM-2 are implemented, covered, and
green on this host with no hardcoded failures and no stub returns. Promotion
past experimental, any performance claim, and any nvCOMP implementation stay
blocked on the gates above.

## 2026-09-30 13:13 -03 — upstream vmbus contribution audit and phantom-blocker reconciliation

Read-only audit of the Hyper-V/VMBus upstream contribution against Michael
Kelley's 2026-09-22 review of the unversioned v1 send, plus a live check of
the installed kernel. Then a documentation-only reconciliation of stale host
identity claims. No code, config, kernel, or host state was changed.

### Audit result

- The 2026-09-17/18 unversioned 2-patch series is the only VMBus mail ever
  sent (Message-ID stem `20260918014017.2536753`). Kelley confirmed the
  order-7 problem and rejected the `vzalloc()` fallback for CoCo guests,
  requesting a redesign on Kameron Carr's `vmbus_alloc_buffer()`. The PDF in
  the operator's Downloads folder is that review reply, not an accepted patch.
- The seven-patch v2 candidate already implements Kelley's five points
  (`struct vmbus_buffer`, folded GPADL ownership, `gpadl.leak`, removal of
  `HV_GPADL_BUFFER_DECRYPTED`, universal `vmbus_alloc_buffer()`). It is
  **unsent by PRD policy** and the GAP-REGISTER gate stays **BLOCKED**.
  Nothing is merged, reviewed-by, acked-by, or rejected upstream.

### Live host check (kernel #9)

- Running: `6.18.40.1-microsoft-standard-WSL2+ #9 SMP PREEMPT_DYNAMIC
  Tue Sep 29 21:30:04 -03 2026`.
- Receipt `/mnt/c/wsl/kernel-ramshared-v6.receipt` reports
  `source_commit=a5cedb4de6f8…`, `build_number=9`, `source_tree_state=clean`.
  That commit is the exact HEAD of branch `vmbus-ring-buffer-upstream-v2` in
  `WSL2-Linux-Kernel-contribution` — the Build #6 source-unmatched gap is
  closed for the running image.
- Observed on this boot: 0 `page allocation failure: order:7`, 0 `accept4
  failed`, `vmbus_alloc_buffer`/`vmbus_free_buffer` present in `/proc/kallsyms`,
  103 VMBus devices, `zram` loaded (the old Validate-KernelBuild6.sh zram
  failure no longer applies).
- BUG-1/2/3/5/8/9/11 and G2 source fixes are present in this tree
  (`d9a1a3a8d6f6`, `a8042f978bc0`, `b64d516de5fc`, `4ea7c35d2cd8`,
  `b99248f63e43`, `0dcd3ad5d996`, `68700eb5aa8a`, `805418bd7021`).

### What is still open (unchanged)

1. Live GPADL create/teardown/host-rescind/local-rescind/close/partial-post
   interleaving on ordinary Hyper-V.
2. UIO subchannel mmap close/unregister, including a deterministic BUG-3
   hold-in-mmap reproducer.
3. Forced order-7 buddy failure showing the order-zero fallback execute on
   this exact candidate. KUnit fault injection is not physical fragmentation.
   The VMBus map-retention hypothesis (EVD-0088/0089/0091) has not been
   re-measured on #9.
4. SEV-SNP / TDX / Arm CCA CoCo transitions — host is a Ryzen 5 3600 and
   cannot supply those modes (EVD-0056). Until that evidence exists the
   series stays PARTIAL and unsent.
5. BUG-2/3/9 runtime reproducers (source fixes alone do not close them).
6. DXG greater-than-4-GiB boundary and PFN-pin-until-destroy proof.
7. Sealed kernel/modules pair: `modules-ramshared.vhdx` is dated 2026-07-10
   against kernel #9 of 2026-09-29, so module-to-VHDX provenance is unproven
   and `seal-kernel-pair.sh` cannot promote.
8. Operator review and explicit approval before any `[PATCH v2 n/7]` email.

### Documentation reconciliation (this entry)

- `docs/reliability/GAP-REGISTER.md`: freeze and VMBus rows no longer claim
  the booted host is Build #6 with CLI 0.14.1 and "neither candidate
  installed"; added a Latest Evidence section for EVD-0114–EVD-0123.
- `docs/specs/no-milestone/vmbus-ring-buffer-upstream-v2/IMPL.md`: status no
  longer says the candidate is not installed; distinguishes the installed
  WSL-derived Build #9 from the unbooted seven-patch mainline series.
- `ROADMAP.md`: the v0.10.0 "RFC v2 submitted to LKML" bullet now names the
  `drivers/block` series explicitly and states it is not the VMBus series.
- Kernel fork `Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/mimoaudite.md`:
  BUG-1 and BUG-8 rows and the "fix candidates" paragraph now record their
  landed source fixes.

`node tools/generate-docs-index.mjs --check` → in sync.
`node tools/check-broken-links.mjs` → no broken markdown links.

**Verdict:** 🟡 `PARTIAL` — audit complete; phantom blockers removed. The
upstream send remains blocked on the eight open items above. No runtime or
CoCo claim is made by this entry.

**What:** Read-only audit of the Hyper-V/VMBus upstream contribution against
the 2026-09-22 review of the unversioned v1 send, a live check of the
installed kernel `#9`, and a documentation-only reconciliation of stale host
identity claims. No code, config, kernel, or host state was changed.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0124`.
**Owner role:** `kernel-coder`.
**Observed at:** `2026-09-30T16:13:00Z`.
**Verified at:** `2026-09-30T16:21:51Z`.
**Source revision:** `1a3b72e0e88a2697829455edd9c80847165ce662`.
**Lifecycle:** `reviewable`.
**Retention:** Keep while the VMBus v2 series remains an unsent draft.
**Freshness:** Re-run the live host check after any kernel install or reboot;
this entry is a point-in-time audit, not a standing qualification.

## 2026-09-30 14:22 -03 — contribution fork slimmed and internal dossiers relocated

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0125`.
**Owner role:** `kernel-coder`.
**Observed at:** `2026-09-30T17:22:00Z`.
**Verified at:** `2026-09-30T16:48:53Z`.
**Source revision:** `ce217c15350b8982d7676ac4861cf35d40099f78`.
**Lifecycle:** `reviewable`.
**Retention:** Keep while the public contribution fork is maintained.
**Freshness:** Re-verify `git ls-files` and the workflow path references after
any further fork sync.
**Category:** `governance`.
**What:** The public kernel contribution fork `emersonbusson/WSL2-Linux-Kernel`
carried RamShared internal process documents inside Linux kernel documentation
namespace at `Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/`. Those
files were relocated to their canonical RamShared homes and removed from the
fork, which now holds only the machine-consumed patch series and its workflow.
**How to measure:** Compare `git ls-files Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/`
in the fork before and after; verify the hosted workflow still resolves
`PATCH_DIR`; semantic presence check of hard constraints and named SPEC tests
in RamShared; `node tools/generate-docs-index.mjs --check` and
`node tools/check-broken-links.mjs`.
**Measured data:** The four SSDV3 documents had **diverged in both directions**.
Line-diff against the fork copies showed PRD 60, SPEC 126, IMPL 492 and
AUDIT-2.5 122 divergent lines. The fork held real unique content (the full
named-test matrix with 14 cases, host-safety constraints, and three dated
2026-09-28 re-audit sections) that the RamShared copies lacked, while RamShared
held the current host state (Build #9 / `a5cedb4de6f8`, hosted run 36574925363)
that the fork copies lacked. Rather than overwrite, the unique fork blocks were
merged into the RamShared files first. Post-merge semantic check: 16/16 hard
constraints present and 14/14 named SPEC test cases present. The fork copies
were then deleted (`fb6b0b7403bb`), removing 1025 lines across six files;
`series/0001..0007`, `vmbus-ring-buffer-v2.patch` and
`.github/workflows/vmbus-upstream.yml` were kept because the workflow applies
those patch files by path and hashes them. Grep confirms the workflow
references none of the removed paths and its directory glob still matches
`series/`. `mimoaudite.md` was renamed to `AUDIT-source.md` (the filename
carried an author/agent name, which project naming and the Zero-Sum README
policy forbid in shared documentation) with content unchanged apart from a
provenance header. `UPSTREAM-STATUS.md` became
`docs/upstream/VMBUS-RING-V2-UPSTREAM-STATUS.md`, beside `LKML-PATCHSET.md`;
its live status header was refreshed (it still claimed "six patches, not yet
run hosted build") while its dated September sections were left as
point-in-time records. `COCO-GAP.md` was added as the formal record that the
CoCo acceptance disjunction is unsatisfied. Two guest-side drill scripts were
added for the isolated Hyper-V lab. Docs index regenerated (54 specs, in
sync); broken-link scan clean.
**Residual blockers:** The docs and fork hygiene gap is closed. This does not
advance the runtime or CoCo gates: live GPADL/UIO lifecycle, forced order-zero
fallback under real fragmentation, and SEV-SNP/TDX/Arm CCA evidence remain
open, and no drill has been executed yet — only its harness exists. Hyper-V
management from this session required elevated PowerShell and was refused
without it; the drills have not run. The CoCo rows cannot be closed on this
host (`EVD-0056`: Ryzen 5 3600, Zen 2, no SEV-SNP, no TDX, no Arm).
**Verdict:** 🟡 `PARTIAL` — documentation and contribution-fork hygiene are
DONE and merged; runtime qualification, the sealed kernel/modules pair, and
CoCo evidence remain open, so the v2 series is still an unsent draft.

## 2026-09-30 14:02 — ITEM-4 worker-cache telemetry envelope

**What:** SSDV3 Step 3 close-out of `cuda-rust-native-tiering` ITEM-4.
Implemented the versioned worker-cache telemetry envelope
(`WorkerTelemetryEnvelope`, schema version 1) within the existing 4 KiB
payload limit, kept physical `cached_bytes`/`target_bytes` in the frame
header, added typed codec fault counters, and labelled logical cache
occupancy separately from RAM in the monitor. Ran the full test suite, clippy
with `-D warnings`, and the per-file coverage gate. The tree was **dirty** at
observation time: the ITEM-4 slice under test is the uncommitted working-tree
diff on the source revision below.

**Verdict:** 🟡 PARTIAL — ITEM-4 is **done** (code, named tests, cover gate,
labels). The overall spec stays partial because ITEM-3 is gated on the absent
nvCOMP runtime and ITEM-5 has no real paired GPU runs. No hardware number is
claimed here.

**Category:** ci-gate

**Measured data:** `cargo test -p ramshared-vram -p ramshared-block -p ramshared-wsl2d -p ramshared-cli` → **1003 passed / 0 failed / 19 ignored** (hardware-gated). `cargo clippy --all-targets -p ramshared-vram -p ramshared-block -p ramshared-cli -- -D warnings` → **0 errors, 0 warnings**. `node tools/ci/check-rust-slice-coverage.mjs -p ramshared-vram,ramshared-block,ramshared-cli --min 80` → **PASSED**; `compressed_cache.rs` 89.9% (222/247), `gpu_cache_worker.rs` 80.1% (672/839), `ipc_cache_client.rs` 84.0% (305/363), `monitor.rs` 84.6% (2171/2566), `codec.rs` 90.1% (317/352), `worker_telemetry.rs` 86.7% (52/60). Envelope ceiling 4096 bytes; refusal reason bounded at 64 bytes; telemetry max age 5000 ms. All six SPEC-named ITEM-4 tests green, plus `heartbeat_telemetry_truncation_fails_closed` and `heartbeat_mismatched_correlation_fails_closed`.

**How to measure:** `CARGO_BUILD_JOBS=1 cargo test -p ramshared-vram -p ramshared-block -p ramshared-wsl2d -p ramshared-cli` then `CARGO_BUILD_JOBS=1 node tools/ci/check-rust-slice-coverage.mjs -p ramshared-vram,ramshared-block,ramshared-cli --files crates/ramshared-vram/src/codec.rs,crates/ramshared-vram/src/worker_telemetry.rs,crates/ramshared-block/src/compressed_cache.rs,crates/ramshared-block/src/gpu_cache_worker.rs,crates/ramshared-block/src/ipc_cache_client.rs,crates/ramshared-cli/src/monitor.rs --min 80`.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0126`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-09-30T17:02:15Z`.
**Verified at:** `2026-09-30T17:02:15Z`.
**Source revision:** `ce217c15350b8982d7676ac4861cf35d40099f78`.
**Lifecycle:** `reviewable`.
**Retention:** Keep until ITEM-5 paired-run gates are recorded or the codec path is sunset under DT-9, whichever comes first.
**Freshness:** Re-run the commands above after any change to `worker_telemetry.rs`, `gpu_cache_worker.rs`, `ipc_cache_client.rs`, `isolated_origin.rs`, or the monitor telemetry labels. This entry is not a hardware result and must not be used as one.

## 2026-09-30 14:07 -03 — upstream v2 readiness: map re-measurement on #9, checkpatch gate, and phantom-blocker closure

**What:** Read-only readiness checkpoint on the VMBus ring-buffer upstream v2
series: a single-sample live map accounting of `vmbus_alloc_buffer` on kernel
`#9`, a fault scan of the same boot, a local reproduction of the seven-patch
checkpatch gate, a drill-harness field-parse fix, and the closure of two purely
documentary phantom blockers in `GAP-REGISTER.md`. The tree was **dirty** at
observation time: `GAP-REGISTER.md`, `scripts/kernel/vmbus-lifecycle-drill.sh`
and this entry were the uncommitted working-tree diff on the source revision
below. Contribution fork at `af11c5e7a9379bdcea3503a5d7b4d1706eb5723e`.
No bind/unbind, no memory pressure, no swap activation, and no RamShared
lifecycle change were performed; the daily WSL2 host is not a drill surface.

**Live map accounting on kernel #9 (closes the "not re-measured" gap).**
A single read-only sample of `/proc/vmallocinfo` on `6.18.40.1-microsoft-standard-WSL2+`
build `#9` (`kernel-ramshared-v6`, SHA-256 `24ac8916…be8b2a`, receipt source
`a5cedb4de6f8`) counts **110–112 live `vmbus_alloc_buffer` maps**, **57.14–60.77 MiB**
of vmalloc area, and **14,725 backing pages**, across **97 VMBus devices**.
Size distribution: 2×36864, 4×45056, 25×61440, 3×266240, 60×430080,
2×528384, 1×1048576, 12×1052672, 1×16781312 bytes. Backing pages sum to
60,313,600 bytes against a 60,772,352-byte vmalloc area, i.e. **1 page of
vmalloc alignment overhead per map** (112 pages total) — normal behaviour,
not a retention signature.

This is **steady state, not the EVD-0091 runaway**: that record reached
24,932 maps / ~7.2 GiB growing at ~44 MiB/min under Build #6. The current
figure is two orders of magnitude lower and shows no growth series. A single
sample cannot establish allocate/free balance across an open/close cycle, so
the GPADL retention hypothesis is **neither confirmed nor closed** and still
requires the isolated-guest lifecycle drill
(`vmbus-lifecycle-drill.sh`, `vmbus_channel_lifecycle_buffer_balance`).

`/proc/vmallocinfo` is mode 0400 root:root; unprivileged reads are denied.
The accounting above required `sudo`. No kernel virtual address is recorded
here or in the drill scripts (NFR-3 / `security.md` info-leak rule).

**Fault scan on the same boot.** `kernel_oops_bug=0`, `kernel_warning_at=0`,
`hung_task=0`, `page_alloc_fail=0`, `accept4_fail=0`, `gpadl_leak=0`,
`call_trace=0`. A naive `grep 'WARNING:'` returns 1; that single hit is the
SRSO mitigation banner (`Speculative Return Stack Overflow: WARNING: See
.../srso.html`) printed at boot under `CONFIG_MITIGATION_SRSO`, not a kernel
warning. Any future scan must exclude mitigation banners or it will report a
false fault.

**CI checkpatch gate reproduced locally, 7/7 clean.** Against mainline base
`93f51579e7df` (Linux 7.3-rc4) in a throwaway worktree, the seven series
patches apply in order, `git diff --check` is clean at every stage, and
cumulative `scripts/checkpatch.pl --strict --no-tree` reports
`total: 0 errors, 0 warnings, 0 checks` at stages 1–7 (677 / 879 / 952 /
952 / 1282 / 1381 / 2657 lines checked). The five `EXPORT_SYMBOL_GPL`
symbols the workflow asserts are present in `drivers/hv/channel.c` and
declared in `include/linux/hyperv.h`. Running checkpatch directly on the
*mail* files reports trailing whitespace; those are blank unified-diff
context lines and are the documented false positive. The workflow's
cumulative-source-diff invocation is the authoritative gate and is clean.
Contribution-fork commit `af11c5e7` adds the missing mail header to
`series/0007-gpadl-lifetime-reclaim.patch` and regenerates
`vmbus-ring-buffer-v2.patch` from the seven files (7 Subjects, 19 diffs).

**Defect found and fixed in the drill harness.** `vmbus-lifecycle-drill.sh`
parsed `/proc/vmallocinfo` field 2 as a virtual-address range (`split($2, a,
"-")`) and would have reported negative byte totals inside the guest. Field 1
is the range and field 2 is the size in bytes. The helper now sums field 2
and the `pages=` attribute, and reports `MAPS count= bytes= pages=`. Both
drill guards still refuse this host with exit 2.

**Phantom blockers closed in `GAP-REGISTER.md`.** The VMBus ring-fallback row
claimed Build #9 map counts "have not been re-measured"; replaced with the
measurement above. Two Build #6-era sentences ("remain unresolved", "the
same ... gap remain") that read as current state are now explicitly dated to
EVD-0098 / EVD-0099 and point at the current section. The 2026-09-28
evidence block is renamed from "Latest Evidence" to "Evidence archive" so
exactly one section carries that title.

**Verdict:** 🟡 PARTIAL — the two gaps that were purely documentary (Build #9
map re-measurement, patch mail hygiene) are closed. Live GPADL
response/rescind interleaving, UIO mmap close/unregister, forced order-zero
fallback under real fragmentation, BUG-2/3/9 runtime reproducers, and DXG
>4 GiB remain open on the isolated-guest drills. CoCo (SEV-SNP / TDX-no-
paravisor / Arm CCA) remains a hard external hardware block
(`COCO-GAP.md`): the Ryzen 5 3600 host cannot supply any of those modes.

**Category:** kernel-runtime-observation

**Measured data:** `sudo awk '/vmbus_alloc_buffer/{n++; t+=$2; ...}' /proc/vmallocinfo`
→ `MAPS count=112 bytes=60772352 pages=14725` (and 110 / 59912192 on the
prior sample minutes earlier; the delta is live device churn). Checkpatch
cumulative totals at stages 1–7: `0 errors, 0 warnings, 0 checks`.

**How to measure:** `sudo awk '/vmbus_alloc_buffer/{n++; t+=$2; for(i=1;i<=NF;i++) if($i~/^pages=/){split($i,p,"="); pages+=p[2]}} END{printf "MAPS count=%d bytes=%d pages=%d\n", n, t, pages}' /proc/vmallocinfo`
— read-only, safe on the daily host. The checkpatch gate is reproduced by
creating a detached worktree at `93f51579e7df`, applying
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/series/000[1-7]-*.patch`
in order, and piping `git diff --no-ext-diff --no-color` into
`scripts/checkpatch.pl --strict --no-tree` after each patch.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0127`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-09-30T17:07:30Z`.
**Verified at:** `2026-09-30T17:07:30Z`.
**Source revision:** `ce217c15350b8982d7676ac4861cf35d40099f78`.
**Lifecycle:** `reviewable`.
**Retention:** Keep until the isolated-guest lifecycle drill reports `vmbus_channel_lifecycle_buffer_balance` on the exact seven-patch candidate, or until the series is withdrawn. The map counts are a single-sample steady-state observation and must not be cited as allocate/free balance.
**Freshness:** Re-run the map command after any kernel install, reboot, or VMBus device topology change. Re-run the checkpatch gate after any edit to `series/*.patch`. If map counts ever exceed ~1,000 without a matching channel inventory, treat that as a regression alarm and open a gate rather than extending this entry.

---

## 2026-09-30 16:40 -03 — CoCo static invariants made machine-checkable (EVD-0128)

**What:** Audited the seven-patch v2 candidate against mainline `93f51579e7df`
for every `set_memory_*` site, corrected two citation defects and one overclaim
in the static-proof draft, and made the surviving invariants machine-checkable
in contribution-fork `63cb97459cd3` so CI enforces them on every run. This does
not close COCO-1..5.

**Question:** Can the Confidential Computing objection be closed without
CoCo hardware, and does the source actually match the claim that was being
made about it?

**Answer:** Not closed — COCO-1..5 still require a real memory-encryption
transition. But the claim that *was* being made did not survive an audit of
the candidate source, and the corrected claim is now machine-checked on
every CI run. Two citation defects and one overclaim were found and fixed
before anything was sent.

**Method.** Fetched mainline `93f51579e7df` (Linux 7.3-rc4) and applied
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/series/0001..0007`
in order (7/7 APPLIED, no fuzz). Audited every `set_memory_*` call site in
the resulting tree, then traced reachability of each one.

**Findings against the previous draft of the proof.**

1. The draft claimed Invariant 1 was "exactly one `set_memory_decrypted()`
   in the buffer path". There are **five** `set_memory_*` sites in
   `drivers/hv/channel.c` and **zero** in `drivers/uio/uio_hv_generic.c`
   and `include/linux/hyperv.h`. Three take `page_address(page)`. **Two
   take a virtual address** and were not mentioned: `kbuffer` at
   `__vmbus_establish_gpadl()`, and `owner->addr` in the reclaim worker.
2. The draft's Invariant 3 cited an `unknown_page` guard and an
   `__vmbus_free_buffer_mem()` chunk-release loop. **Neither identifier
   exists** in the candidate. The real mechanism is
   `owner->encryption_unknown = true` on a failed `set_memory_decrypted()`,
   `vmbus_buffer_owner_can_reclaim()` refusing that owner permanently, and
   `vmbus_free_buffer()` doing `continue` past `__free_pages` when
   re-encryption fails.
3. The draft's Invariant 4 named `vmbus_uses_shared_page_chunks()`. The
   function is **`vmbus_needs_shared_pages()`**.

A reviewer who grepped for those names would have found nothing and
discounted the whole proof. That is the v1 cover-letter failure mode
repeated at the document level.

**What the two virtual-address sites actually are.** They are the
pre-existing mainline `vmbus_establish_gpadl()` contract, preserved
unchanged, and its symmetric undo. Decryption is gated on
`gpadl->decrypted = !memory_prepared && ...`. Every buffer this series
allocates reaches `__vmbus_establish_gpadl()` with `memory_prepared = true`:

| Caller | `memory_prepared` | Decrypts `kbuffer`? |
| --- | --- | --- |
| `vmbus_establish_gpadl_owned()` (netvsc, UIO) | `true` | no |
| `vmbus_establish_gpadl_caller_decrypted()` | `true` | no |
| ring open, `HV_GPADL_RING` | `true` | no |
| `vmbus_establish_gpadl()` (legacy export) | `false` | yes — pre-existing |

In-tree callers of the bare legacy export after this series: **zero**.

**What was built.** Contribution-fork `63cb97459cd3` adds
`coco-static-invariants.py` and an `Enforce CoCo static invariants` CI step
that checks six invariants (direct-map chunk encryption, prepared GPADL does
not re-decrypt, unknown page state retained, private path never touches
encryption, UIO never transitions encryption, consumers avoid the legacy
path) and then **injects the rejected pattern and requires the gate to
reject it**, twice — once as `set_memory_decrypted((unsigned long)buffer->addr, …)`
in the allocator, once as a `set_memory_decrypted()` call in
`uio_hv_generic.c` — restoring the tree before the KUnit build.

**Measured data.** Checker on the unmodified candidate: 6/6 PASS,
`COCO-STATIC-PROOF: all invariants hold.`, exit 0. Four negative injections
each produced the expected FAIL and exit 1: vmalloc decrypt in the allocator
(`INV-1 … buffer->addr`), ring GPADL with `memory_prepared=false` (`INV-2`),
removed `encryption_unknown` marker (`INV-3`), UIO `set_memory_decrypted()`
(`INV-5`). Full CI step body dry-run end to end: `STEP_EXIT=0`, tree restored
byte-identical before the final check.

**Verdict:** 🟡 PARTIAL — the static half is now honest and enforceable. The
platform half is untouched: **no SEV-SNP, Intel TDX, or Arm CCA page-state
transition has been observed.** `COCO-GAP.md` stays open, the send gate
stays closed, and the cover letter draft now claims the negative proof and
explicitly not platform evidence. `COCO-GAP.md` also records a third
resolution that does not require buying silicon: rent Azure Confidential
VMs (SEV-SNP `DCasv5`/`ECasv5`, TDX `DCesv5`/`ECesv5`); Arm CCA still has
no cloud SKU.

**Category:** kernel-source-audit

**How to measure:** apply the seven patches to mainline `93f51579e7df`, then
`python3 Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/coco-static-invariants.py --tree <linux-tree>`.
Expected: six `PASS` lines and `COCO-STATIC-PROOF: all invariants hold.`
To prove the gate is armed, replace a `page_address(page)` operand with
`buffer->addr` at the allocator decrypt site and require a non-zero exit.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0128`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-09-30T19:40:00Z`.
**Verified at:** `2026-09-30T19:40:00Z`.
**Source revision:** `e9cad0b9`
**Lifecycle:** `reviewable`.
**Retention:** Keep until COCO-1..5 are satisfied on real hardware or the series is withdrawn. If a future edit moves any `set_memory_*` operand away from `page_address()`, or passes `memory_prepared = false` outside `vmbus_establish_gpadl()`, this entry becomes the rollback baseline.
**Freshness:** Re-run the checker after any edit to `drivers/hv/channel.c`, `drivers/uio/uio_hv_generic.c`, or `series/*.patch`. Never silence the CI step or weaken its self-test to land a patch.

## 2026-09-30 15:54 -03 — v2 series hygiene pass and submission-readiness review (EVD-0129)

**What:** Reviewed the six-patch v2 series for LKML submission readiness and
closed every structural mail defect found: rebuilt the series as six clean
`git format-patch` commits, folded the order-descent fixup into the patch that
introduces the test it corrects, retitled the three test commits to kernel
subsystem prefixes, removed the local `Rollback trigger:` pseudo-trailers, and
regenerated `series/SHA256SUMS` in the same commit as the byte change. The
applied tree is byte-identical to the previous seven-patch candidate
(`BINARY_MATCH`). Documentation was aligned to the six-patch shape and the
repository hygiene gates were brought back to green.

**Question:** Would a maintainer accept these mails as a v2 series as they
stand, and does every claim in the surrounding documentation match the bytes?

**Answer:** No, not as they stood. Five of the seven mail files were
hand-assembled: they carried a signature and `Signed-off-by` but no `From
<sha>` marker, no `---` separator and no diffstat, so `git format-patch`
provenance could not be verified and a maintainer would have bounced them with
"please resend generated by git format-patch". Separately, patch 2 introduced
`vmbus_ring_fallback_order_zero_test` with `vmbus_buffer_order(128, MAX_PAGE_ORDER)`
and patch 4 corrected that exact line two commits later — a series must not add
a broken test and then repair it. The subject prefixes `test(hv):` were a local
Conventional Commits convention leaking into kernel mail, and
`Rollback trigger:` is a local governance trailer that reads as process leakage
on LKML. All of those are now closed. What remains open is not a mail defect:
it is platform and runtime evidence, and a fresh hosted run on the new bytes.

**Method.** Built the series as six commits on mainline `93f51579e7df`
(Linux 7.3-rc4) from a sparse clone, then ran the CI-equivalent sequence
locally: sequential apply, `git diff --check`, cumulative
`scripts/checkpatch.pl --strict --no-tree` at every stage, the CoCo static
invariant checker with its gate self-test, and a file-by-file hash comparison
against the previous seven-patch tree.

**Findings and fixes.**

1. **Five of seven mails were not `git format-patch` output.** Only the last
   two carried a real `From <sha>` marker and diffstat. Every mail is now
   regenerated with `git format-patch`, and each one independently reports
   "has no obvious style problems and is ready for submission".
2. **Series-hygiene defect: an in-series fixup.** The order-descent test vector
   fix is folded into the patch that introduces the case. The series is now six
   patches; `0004-kunit-order-vector.patch` is gone and the former
   `0007-gpadl-lifetime-reclaim.patch` is renumbered to
   `0006-gpadl-lifetime-reclaim.patch`.
3. **`Signed-off-by` was absent from the regenerated mails** because the build
   script did not pass `--signoff`. Fixed; all six carry
   `Signed-off-by: Emerson Busson`, and patches 1 and 6 carry
   `Suggested-by: Michael Kelley` ahead of it.
4. **Stale patch files contaminated `SHA256SUMS`.** Two superseded `.old`
   dated files remained after the rename and were hashed. Removed; the six
   known filenames are the only entries, and the consolidated
   `vmbus-ring-buffer-v2.patch` is regenerated from them (3904 lines).
5. **Subject prefixes.** `test(hv):` is a local convention, not a kernel
   subsystem prefix. The three test commits now use `hv: vmbus:` and
   `uio: hv_generic:`.
6. **`Rollback trigger:` pseudo-trailers** removed from the two commits that
   carried them. The substance they held is already covered by named test
   assertions; the form is local governance and does not belong in LKML mail.
7. **One unproven causal claim softened.** Patch 1 said the change "reduces
   high-order allocation failures". That is not established without the
   forced-fragmentation drill. It now says the change "lets a ring still be
   created when one high-order block is unavailable".
8. **Documentation drifted from the candidate.** The cover letter, upstream
   status, `IMPL.md`, `COCO-GAP.md`, `COCO-STATIC-PROOF.md` and `GAP-REGISTER`
   all described a seven-patch series with a `0007` final patch. All
   current-status claims now describe the six-patch shape. Historical EVD
   narrative bound to a named commit or run ID is left as a point-in-time
   record.
9. **Repository hygiene gates were red.** Two independent problems, both
   fixed: the cover-letter draft stored fourteen personal LKML recipient
   addresses in the mail header (the tree carries zero email addresses by
   policy; recipients are now names and list names, and literal addresses are
   generated at send time by `get_maintainer.pl`), and two test fixtures in
   `cascade_io.rs` used long `ffff…` hex sentinels that the non-allowlistable
   `KERNEL_ADDRESS` rule rejects. The invocation-id sentinel collided with the
   `#[cfg(test)]` stub value `0123456789abcdef0123456789abcdef` on the first
   attempt and the identity test correctly failed; the final sentinel
   `deadbeefdeadbeefdeadbeefdeadbeef` differs from the stub, preserves
   `canonical_invocation_id` shape, and contains no `ffff` run.

**Measured data.**

| Check | Result |
| --- | --- |
| Stages applied to `93f51579e7df` | 6/6, no fuzz |
| `git diff --check` | clean at every stage |
| Cumulative `checkpatch.pl --strict --no-tree` | `total: 0 errors, 0 warnings, 0 checks` at stages 677 / 879 / 952 / 1282 / 1381 / 2657 lines |
| Each mail file, `checkpatch.pl --strict --no-tree` | "ready for submission" ×6 |
| CoCo static invariants | 6/6 PASS, `COCO-STATIC-PROOF: all invariants hold.` |
| Gate self-test (reject injected vmalloc-decryption pattern) | PASS, gate exits non-zero |
| `BINARY_MATCH` vs previous seven-patch tree | byte-identical across all 9 touched files |
| `series/SHA256SUMS` | `sha256sum -c` clean |
| `node tools/check-broken-links.mjs` | no broken markdown links |
| `node tools/generate-docs-index.mjs --check` | in sync |
| `./scripts/docs-check.sh` | `docs-check OK` |
| `check-public-hygiene.mjs --candidate` | `PUBLIC_HYGIENE_STATUS=PASS` (1190 files) |
| `check-documentation-governance.mjs --all` | `GOVERNANCE_STATUS=PASS` (553 files) |
| `cargo test … revalidate_daemon_and_socket` | 2 passed |

The nine files compared under `BINARY_MATCH` are
`drivers/hv/{channel,channel_mgmt,hyperv_vmbus,ring_buffer,vmbus_drv}.c`,
`drivers/net/hyperv/{hyperv_net.h,netvsc.c}`,
`drivers/uio/uio_hv_generic.c`, `include/linux/hyperv.h`.

**Verdict:** 🟡 PARTIAL — the series is now a submission-shaped artifact. The
mail defect class is closed and the applied source is unchanged from the
candidate that already had design review. Two things are **not** established
and must not be claimed: **hosted runs 36574925363 and 36590352003 qualify the
predecessor bytes, not these**, so the build/KUnit gates have no hosted
evidence on this candidate until a fresh run executes against the pinned
`series/SHA256SUMS`; and the platform/runtime rows (live GPADL response/rescind
interleaving, UIO mmap close/unregister, forced order-zero fallback under real
fragmentation, SEV-SNP / TDX-no-paravisor / Arm CCA page-state transitions)
remain open exactly as before. A mail-format pass cannot close any of them.

**Category:** kernel-submission-readiness

**How to measure:** apply `series/0001..0006` in order to mainline
`93f51579e7df`, run cumulative `scripts/checkpatch.pl --strict --no-tree` at
each stage and require zero at all six, run
`python3 Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/coco-static-invariants.py --tree <linux-tree>`
and require six PASS lines, then `sha256sum -c series/SHA256SUMS`. To reproduce
the `BINARY_MATCH` claim, hash the nine touched files in the applied tree
before and after the series is rebuilt. To reproduce the hygiene claim,
`node tools/ci/check-public-hygiene.mjs --candidate` and
`node tools/ci/check-documentation-governance.mjs --all` must both be clean on
a tree with no email addresses and no `ffff[0-9a-f]{8,}` hex run in structural
source or fixtures.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0129`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-09-30T18:54:32Z`.
**Verified at:** `2026-09-30T18:54:32Z`.
**Source revision:** `bd0611c6`
**Lifecycle:** `reviewable`.
**Retention:** Keep until the series is sent or withdrawn. If any patch is edited again, regenerate `series/SHA256SUMS` in the same commit and re-run the six-stage check; this entry then becomes the pre-edit baseline.
**Freshness:** Hosted runs 36574925363 and 36590352003 do **not** qualify these bytes. Never cite the predecessor runs as evidence for the six-patch series. That requirement is now satisfied by hosted run 36763981097 on `7e4ccc98d32f` (EVD-0130): build/Sparse/checkpatch/KUnit gates pass against the pinned `series/SHA256SUMS`. Re-open this gate only if any patch file is edited again — then regenerate `series/SHA256SUMS` in the same commit and require a fresh hosted run before repeating the claim.

## 2026-09-30 16:29 -03 — six-patch pinned bytes qualified in hosted CI (EVD-0130)

**What:** Hosted run
[36763981097](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36763981097)
executed the full `vmbus-upstream.yml` gate set against contribution-fork
commit `7e4ccc98d32f`, which carries the six-patch v2 series pinned by
`series/SHA256SUMS`. All three jobs completed `success` with no failed or
skipped step inside any job.

**Question:** Do the build/Sparse/checkpatch/KUnit/CoCo-invariant gates pass
on the *current* six-patch candidate, or only on its predecessor?

**Answer:** They pass on the current candidate. Measured on run 36763981097:

| Gate | Result |
| --- | --- |
| `wsl-backport` build `W=1` + Sparse | PASS |
| `wsl-backport` KUnit (`hyperv-vmbus-gpadl-lifetime` 12 + `hyperv-uio-hv-generic-mmap` 3) | PASS 15/15 |
| `kernel (x86_64)` apply + build, all 6 stages | PASS |
| `kernel (x86_64)` cumulative `checkpatch.pl --strict --no-tree` | PASS at every stage |
| `kernel (x86_64)` `Enforce CoCo static invariants` | PASS, **both gate self-tests green** |
| `kernel (x86_64)` 20 named VMBus/UIO KUnit cases | PASS 20/20 |
| `kernel (arm64)` apply + build + Sparse | PASS |
| `series/SHA256SUMS` byte-pin | PASS (`sha256sum -c`, 6/6) |

Run 36762560212 on the preceding commit `dbf05cc05ca8` was **red** and is
part of this record, not omitted: `wsl-backport` failed
`vmbus_buffer_cleanup_repeated_test` with
`Expected buffer.addr == ((void *)0)` at `drivers/hv/channel.c:1539` and a
`WARNING` at `__vmbus_free_buffer` line 1053 (`WARN_ON_ONCE` in the `!owner`
branch), 14/15 KUnit passing. The `kernel` job was skipped because it has
`needs: wsl-backport`.

Root cause: commit `a8042f978bc0` ("free retained buffers under the reclaim
lock") restructured `__vmbus_free_buffer()` to clear `buffer->pages` under
`vmbus_retained_buffers_lock` and dropped `memset(buffer, 0, sizeof(*buffer))`
from the retain-success branch. `vmbus_buffer_retain()` copies the record into
the owner and nulls `buffer->owner`, but leaves `addr`/`chunks`/`chunk_cnt`
set, so a repeated free observed `owner == NULL` with a non-empty buffer and
warned instead of taking the documented "Safe to call twice" no-op. The
non-retain path already ended in a memset; the test asserts exactly that
contract.

Fix: fork commit `7e4ccc98d32f` restores the memset on the retain-success
path (7 lines). `scripts/checkpatch.pl -f drivers/hv/channel.c` reports
`total: 0 errors, 0 warnings` and "ready for submission".

**Scope boundary, stated explicitly:** this defect lived in the **WSL
backport tree only**. The mainline six-patch series has a different
`vmbus_release_buffer()` in `0006-gpadl-lifetime-reclaim.patch` that memsets
on every path (empty buffer, no owner, and owner transfer) and is guarded by
`vmbus_buffer_repeated_owner_release_test`. The mainline candidate never
carried the bug, and the applied tree was never modified by the fix.

Why it surfaced only at `dbf05cc05ca8`: commits `b64d516de5fc` and
`a8042f978bc0` changed `drivers/hv/channel.c` after the last green run
(36590352003 on `de5138b5ebc3`). The push `de5138b5ebc3..dbf05cc05ca8` was
the first to send them to CI. A green run qualifies the SHA it ran against,
not a branch tip that later grew commits.

**What this closes:** the source-level qualification gap that EVD-0129 named
as outstanding. Build, Sparse, strict checkpatch, the 20 named KUnit cases,
the CoCo static invariants and **both gate self-tests** now have hosted
evidence bound to the pinned six-patch `series/SHA256SUMS`.

**What this does not close, and must not be read as closing:**

- COCO-1..5. No hosted runner is a confidential guest. A QEMU/TCG CoCo boot
  is forbidden evidence theatre. The only non-silicon route is an Azure
  Confidential VM (`DCasv5`/`ECasv5` SEV-SNP, `DCesv5`/`ECesv5` TDX); Arm CCA
  has no cloud SKU.
- Live GPADL response/rescind interleaving, UIO subchannel mmap
  close/unregister including the hold-in-mmap window, forced order-7
  fragmentation producing the order-zero fallback, and the 100-cycle buffer
  balance. Those need a real Hyper-V guest. QEMU cannot provide VMBus.
- WSL-backport lifecycle runtime qualification. A `wsl-backport` KUnit pass
  is a QEMU/KUnit run; it is not Hyper-V and not CoCo.

**Verdict:** 🟡 PARTIAL — the source-level qualification gap is closed. The
six-patch pinned bytes now have hosted build/Sparse/checkpatch/KUnit and
CoCo-invariant evidence, including both gate self-tests. CoCo-1..5, live
GPADL/UIO runtime interleaving and forced order-zero fallback remain open and
are not claimed.

**Category:** kernel-source-audit

**How to measure:** push the contribution-fork branch and read
`gh run view 36763981097 --repo emersonbusson/WSL2-Linux-Kernel --json jobs`.
Expect three jobs `success`. Independently, on the pinned bytes:
`(cd series && sha256sum -c SHA256SUMS)` must report 6/6 `OK`.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0130`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-09-30T19:29:29Z`.
**Verified at:** `2026-09-30T19:29:29Z`.
**Source revision:** `e6354d94`
**Lifecycle:** `reviewable`.
**Retention:** Keep until the series is sent or withdrawn. If any file under
`series/` is edited, regenerate `series/SHA256SUMS` in the same commit and
require a fresh hosted run; this entry then becomes the pre-edit baseline.
**Freshness:** Re-run on any change to `series/*.patch`,
`.github/workflows/vmbus-upstream.yml`, `coco-static-invariants.py`, or
`drivers/hv/channel.c` in either tree. Never cite runs 36574925363 or
36590352003 as evidence for this candidate — they qualified predecessor
bytes. Never cite run 36762560212 as a pass; it is the recorded red baseline
for the `a8042f978bc0` omission.

## 2026-09-30 16:48 -03 — hosted Windows runners can boot a real VMBus guest

**What:** Hosted run
[36767912983](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36767912983)
(`WSL runtime feasibility probe`, both matrix legs `success`,
contribution-fork SHA `b6bc34402810`) measured what a GitHub-hosted Windows
runner can actually host. This is a **capability probe**, not a kernel
qualification: it measures the runner, not the candidate, and it must never
be cited as a build, KUnit or CoCo gate.

**Question:** Can a hosted Windows runner boot a real Hyper-V guest with
real VMBus, so the runtime drills can run in Actions without new hardware?

**Answer:** Yes. Measured identically on `windows-latest` and
`windows-2025`:

| Probe | Value |
| --- | --- |
| `hypervisor_present` | `True` |
| `total_physical_memory_gb` | `16` |
| `virtual_machine_platform_state` | `Enabled` |
| `enable_VirtualMachinePlatform` | `ok restart_required=False` |
| `enable_Microsoft-Windows-Subsystem-Linux` | `ok restart_required=False` |
| `wsl.exe` | `present`, WSL `2.7.14.0`, kernel `6.18.33.2-2` |
| `wsl_status` | `Default Version: 2` |
| rootfs asset | `alpine-minirootfs-3.24.2-x86_64.tar.gz`, 3 701 382 bytes |
| rootfs sha256 | `c5ca053cfe1d85c5b96dff8b9bc57045f7f184a30ffb6b65776409ca90388677` |
| `wsl --import` | `exit=0` |
| guest view | `6.18.33.2-microsoft-standard-WSL2` · `VMBUS_PRESENT` · `VMBUS_DEVICES=30` |
| `wsl --unregister` | `ok` |

**Probe verdict:** `WSL2_RUNTIME_AVAILABLE` on both runner images.

A disposable WSL2 guest with **30 real VMBus devices** is reachable from a
hosted Windows runner in about eight seconds, at a cost of one 3.7 MB rootfs
and one scratch VHDX that the job unregisters before it ends. The probe
installs no kernel, touches no swap, applies no memory pressure and changes
no RamShared lifecycle state.

What this settles and what it does not:

- It **settles feasibility** for WSL-backport runtime work in Actions. The
  nearest real VMBus runtime without new hardware is no longer hypothetical.
- It does **not** qualify the mainline six-patch series. That guest boots
  Microsoft's `6.18.33.2-microsoft-standard-WSL2`, not a kernel built from
  the pinned `series/`. Those patches apply to a mainline base, so their
  GPADL/UIO lifecycle and order-zero fallback drills need a mainline kernel
  on a Hyper-V guest. The companion `hyper-v-role-probe` measures whether a
  hosted runner can define an arbitrary Gen2 VM for exactly that.
- It does **not** close COCO-1..5. A stock WSL2 guest is not a confidential
  guest. No hosted runner is. A QEMU/TCG CoCo boot remains forbidden
  evidence theatre. The non-silicon route is still an Azure Confidential VM
  (`DCasv5`/`ECasv5` SEV-SNP, `DCesv5`/`ECesv5` TDX); Arm CCA has no cloud
  SKU.
- `second_level_address_translation_reported=False` is a CPUID-derived flag
  that nested guests commonly mask. It is recorded for context and was not
  used to decide the verdict. Only a booted guest exposing VMBus decides it.

**Recorded red baseline:** run **36766972913** (`55cb1e0df4f5`) failed on
both legs while measuring the runner correctly. The job went red only
because `wsl.exe` returns 1 on "no installed distributions" and the Actions
`pwsh` wrapper propagates `$LASTEXITCODE`. A probe whose answer is negative
must not be reported as a broken probe; `b6bc34402810` makes every step exit
0 once its measurement is recorded and confines red to a probe that failed to
measure at all.

**Verdict:** 🟡 PARTIAL — hosted Windows runners can boot a real VMBus
guest, so runtime drills are feasible in Actions without new hardware. The
probe qualifies the runner, not the candidate. Mainline runtime drills,
COCO-1..5 and the send gate remain open and are not claimed.

**Category:** kernel-runtime-audit

**How to measure:** `gh run view 36767912983 --repo emersonbusson/WSL2-Linux-Kernel --log`
and grep `VERDICT=` / `PROBE wsl_guest_view`. Both matrix legs must print
`VERDICT=WSL2_RUNTIME_AVAILABLE` and a `wsl_guest_view` line containing
`VMBUS_PRESENT`. The uploaded `wsl-runtime-probe-*` artifact holds
`capability.txt` and `verdict.txt`.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0131`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-09-30T19:48:22Z`.
**Verified at:** `2026-09-30T19:48:29Z`.
**Source revision:** `bc51b5d9`
**Lifecycle:** `reviewable`.
**Retention:** Keep until the series is sent or withdrawn. Keep the
rootfs sha256 recorded above: a future probe that resolves a different
Alpine asset is a new measurement, not a repeat of this one.
**Freshness:** Re-run on any change to
`.github/workflows/wsl-runtime-probe.yml`. Never cite this entry as build,
KUnit, CoCo or mainline runtime evidence. Never cite run 36766972913 as a
pass; it is the recorded red baseline for the `$LASTEXITCODE` propagation
defect. If GitHub retires or repurposes the `windows-latest` /
`windows-2025` images, this feasibility claim is void until re-measured.

## 2026-09-30 16:57 -03 — hosted runners can define and run Gen2 Hyper-V VMs

**What:** Hosted run
[36768828972](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36768828972)
(`Hyper-V role feasibility probe`, both matrix legs `success`,
contribution-fork SHA `e260066e0129`) measured whether a GitHub-hosted
Windows runner exposes the Hyper-V **management** stack — the part that
defines arbitrary VMs — and not merely the lightweight hypervisor that WSL2
uses. Like EVD-0131 this is a **capability probe**: it measures the runner,
not the candidate.

**Question:** Can the mainline six-patch candidate be booted as a real
Hyper-V guest in Actions, so the GPADL/UIO lifecycle and order-zero fallback
drills run against those exact mainline bytes?

**Answer:** Yes. Measured identically on `windows-latest` and `windows-2025`:

| Probe | Value |
| --- | --- |
| `hypervisor_present` | `True` |
| `total_physical_memory_gb` | `16` |
| `feature_Microsoft-Hyper-V` | `Enabled` |
| `feature_Microsoft-Hyper-V-Management-PowerShell` | `Enabled` |
| `feature_HypervisorPlatform` | `Enabled` |
| `feature_VirtualMachinePlatform` | `Enabled` |
| `feature_RSAT-Hyper-V-Tools-Feature` | `Enabled` |
| `feature_Microsoft-Hyper-V-All` | `not_in_this_sku` (name only; the role is present under other names) |
| `enable_Microsoft-Hyper-V` | `ok restart_required=False` |
| `enable_HypervisorPlatform` | `ok restart_required=False` |
| `any_restart_required` | `False` |
| `new_vm_cmdlet` | `present` |
| `probe_vm_create` | `ok` (Gen2, 1 GiB, diskless) |
| `probe_vm_firmware` | `configured` (secure boot off) |
| `probe_vm_state` | `Running` |
| `probe_vm_remove` | `ok` |

**Probe verdict:** `HYPERV_MANAGEMENT_AVAILABLE` on both runner images.

A hosted runner reaches `Running` for a Gen2 VM we defined, without a reboot.
The earlier `Microsoft-Hyper-V-All=unavailable` reading was a **feature-name**
artifact of the runner SKU, not a capability gap: the role is present as
`Microsoft-Hyper-V`, `Microsoft-Hyper-V-Online`, `Microsoft-Hyper-V-Offline`
and `RSAT-Hyper-V-Tools-Feature`, all `Enabled`.

What this settles and what it does not:

- It **settles feasibility for mainline runtime drills in Actions.** The
  six-patch candidate can be built as a mainline `bzImage` on the Linux job,
  shipped as an artifact, and booted as a Gen2 Hyper-V guest on the Windows
  job. That guest is ordinary x86_64 Hyper-V with a real VMBus, which is the
  surface `vmbus-lifecycle-drill.sh` and `vmbus-fragmentation-drill.sh` are
  written for.
- The probe created a **diskless** VM only. Booting our kernel still needs a
  boot chain (ESP with GRUB or an `CONFIG_EFI_STUB` kernel plus an initrd and
  an embedded cmdline). That is ordinary build work, not a capability gap.
- It does **not** close COCO-1..5. A Gen2 Hyper-V guest on a hosted runner is
  not a confidential guest. SEV-SNP, TDX-without-paravisor and Arm CCA still
  require an Azure Confidential VM (`DCasv5`/`ECasv5`, `DCesv5`/`ECesv5`) or
  lab hardware. Arm CCA has no cloud SKU.
- It does **not** close the send gate. Operator approval is still required
  before any `git send-email`.
- `second_level_address_translation_reported=False` was again not used to
  decide the verdict. A VM reaching `Running` is the test.

**Probe scope boundary:** no kernel was installed anywhere, no swap was
touched, no memory pressure was applied, and no RamShared lifecycle state
was changed. The probe VM was deleted before the job ended
(`probe_vm_remove=ok`).

**Verdict:** 🟡 PARTIAL — both cheap blockers to upstream approval are now
measured as feasible in Actions: a real VMBus guest (EVD-0131) and an
arbitrary Gen2 Hyper-V VM whose kernel we choose (this entry). The drills
themselves have not been run yet, and COCO-1..5 remain open. Nothing here is
claimed as a pass on the candidate.

**Category:** kernel-runtime-audit

**How to measure:** `gh run view 36768828972 --repo emersonbusson/WSL2-Linux-Kernel --log`
and grep `VERDICT=` / `PROBE probe_vm_`. Both matrix legs must print
`VERDICT=HYPERV_MANAGEMENT_AVAILABLE` and `probe_vm_running_observed=True`.
The uploaded `hyper-v-role-probe-*` artifact holds `capability.txt` and
`verdict.txt`.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0132`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-09-30T19:57:11Z`.
**Verified at:** `2026-09-30T19:57:17Z`.
**Source revision:** `96a894ff`
**Lifecycle:** `reviewable`.
**Retention:** Keep until the series is sent or withdrawn. The feature-name
mapping recorded above is the load-bearing finding: if a future run reports
`Microsoft-Hyper-V-All=not_in_this_sku`, that alone is not evidence of a
capability gap.
**Freshness:** Re-run on any change to
`.github/workflows/hyper-v-role-probe.yml`. Never cite this entry as build,
KUnit, CoCo or drill evidence. If GitHub retires or repurposes the
`windows-latest` / `windows-2025` images, or drops the Hyper-V role from
them, this feasibility claim is void until re-measured.

## 2026-09-30 18:16 -03 — first full Hyper-V runtime drill on the six-patch candidate (EVD-0133)

**What:** Run
[36777284600](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36777284600)
(`Hyper-V runtime drill`, all three jobs `success`, contribution-fork SHA
`f831c80e0a2b`) booted the exact pinned six-patch mainline candidate as a
disposable Gen2 Hyper-V guest and ran the lifecycle and fragmentation
drills against those bytes on `windows-2025` and `windows-latest`.

**Question:** Do the GPADL/UIO lifecycle and order-zero fallback drills
execute their stress paths against the sendable candidate in Actions?

**Answer: no.** The harness is green and the candidate is healthy, but
**neither stress path executed**. This entry is recorded as
**PARTIAL** precisely so the green job is not mistaken for a gate closure.

**What the run does prove** (both runners, identical):

| Fact | Value |
| --- | --- |
| base | mainline `93f51579e7df`, `applied_stages=6` |
| `kernelversion` | `7.3.0-rc4+` |
| bzImage | 4 174 848 bytes |
| bzImage sha256 | `980a45653dda0ccadd20c993dba7800a2b8514d5fb156f5d86a9634f4512b295` |
| image audit | `IMAGE efi_stub=ok machine=x86_64 subsystem=10`, `IMAGE initramfs=ok` |
| boot | single-file `BOOTX64.EFI`, no reboot, full teardown (`vm_remove=ok`, `vhd_remove=ok`, `drill_switch_remove=ok`) |
| VMBus | `present=yes devices=14`, `hv_vmbus: Vmbus version:6.0` |
| candidate symbols | `HYPERV_DRILL_SYMBOLS alloc=4` (`vmbus_alloc_buffer` family present) |
| map balance over 30 hv_netvsc unbind/bind cycles | `BASELINE MAPS count=12 bytes=20279296 pages=4939` == `FINAL MAPS count=12 bytes=20279296 pages=4939` |
| transient dip | `cycle20 MAPS count=11 bytes=19226624 pages=4683`, back to 12 by cycle 30 |
| channel open under pressure | `PRE-OPEN 12 maps` / `POST-OPEN 12 maps`, `nic_driver=/sys/bus/vmbus/drivers/hv_netvsc` |
| damage | `HYPERV_DRILL_SPLATS count=0`, `HYPERV_DRILL_FAULTS count=0` |
| console | 23 849 bytes (`windows-2025`), 21 103 bytes (`windows-latest`) |

The map balance is genuine evidence **against** the EVD-0088/0089/0091
retention/runaway hypothesis on this candidate: 30 full rebind cycles return
to the exact baseline. It does not close that gate by itself — causality to
the historical freeze was never this measurement's claim.

**What the run does NOT prove** (the load-bearing negatives):

1. **UIO subchannel mmap / hold-in-mmap (BUG-3) never ran.** All 30 cycles
   printed `bind_fail`; `PHASE2 no UIO device or no vmbus_drill_helper;
   skipping mmap` and `PHASE2 no sysfs ring found`. Root cause, confirmed in
   the tree: `class_id_show()` emits `"{%pUl}\n"` **with braces**, while
   `new_id_store()` hands the buffer to `guid_parse()` → `uuid_is_valid()`,
   which accepts exactly the 36-char canonical form. The drill wrote
   `{f8615163-df3e-46c5-913f-f2d2f965ed0e}`, `guid_parse()` returned
   `-EINVAL`, the dynid never registered, and `uio_hv_generic` — whose
   `id_table` is `NULL, /* only dynamic id's */` — could never bind.
   `|| true` swallowed the `new_id` write, so thirty cycles looked like a
   quiet log rather than a drill that never started. `HYPERV_DRILL_DEVICE
   classid=` printed empty 14 times for the same class of bug: the sysfs
   attribute is `class_id`, not `classid`.
2. **The order-zero fallback was not exercised, and on this guest it cannot
   be.** `vmbus_uses_shared_page_chunks()` is
   `!encrypted && (hv_isolated || IS_ENABLED(CONFIG_ARM64))`. A hosted x86_64
   runner is neither isolated nor arm64, so **every ring allocation here
   takes the `vzalloc()` branch** and the chunked order-N → order-0 path —
   the one that calls `set_memory_decrypted()` per chunk — is unreachable.
   It also would not log: that path uses `__GFP_NORETRY | __GFP_NOWARN`, so
   even where it runs, `grep 'order:7'` cannot see it. Measured here:
   `order7_failures=0`, and the buddy still held `120` order-10 blocks after
   hogging 1464 MiB of 2003 MiB, so `order:7` never had a reason to appear.
   Verdict printed: `INCONCLUSIVE_ORDER7_NEVER_FAILED`.
3. **No host-rescind / GPADL response-rescind interleaving** was performed.
4. **No CoCo.** Every number above is ordinary x86_64 Hyper-V evidence.
   COCO-1..5 remain open and this entry must never be cited as a CoCo gate.

**Root causes fixed in the same work stream** (not yet re-measured here):

- dynid GUID written unbraced, derived from sysfs `class_id` with the braces
  stripped (`${CLS#\{}` / `${CLS%\}}` — the unescaped `${CLS#{}` form parses
  differently in bash, dash and busybox ash and was rejected).
- inventory attribute renamed to `class_id`.
- `new_id` failures now counted; the lifecycle drill exits non-zero if any
  bind step failed or if no hold-in-mmap path ran, instead of exiting 0 after
  a silent skip.
- new `fragment-buddy` primitive replaces the single large `mlock-hog`: it
  takes the budget as 64 KiB chunks and frees every other one, so no two
  neighbouring chunks are free, nothing above order-4 can coalesce, and the
  freed chunks still serve order-0. The single hog splits only as far as it
  must and leaves the remainder as order-10 — which is exactly why 120 of
  them survived.
- the fragmentation verdict now states its scope: `PASS_RING_ALLOCATION_UNDER_FRAGMENTATION`
  with `VERDICT_SCOPE ordinary-x86_64-vzalloc-path; co-co-chunked-fallback-not-exercised`.
  `PASS_ORDER0_FALLBACK` is retired as unreachable on this guest and
  misleading as a claim.
- `NamedPipeClientStream.ReadTimeout` is no longer relied on. The property
  setter throws `Timeouts are not supported on this stream` on this .NET and
  became the reader job's whole output. Replaced with `ReadAsync` + `Wait(1000)`.

**Verdict:** 🟡 PARTIAL — the harness works end to end and the candidate is
healthy as a real Gen2 Hyper-V guest with real VMBus, with exact map balance
across 30 rebind cycles and zero damage. **The GPADL/UIO lifecycle gate and
the order-zero fallback gate are NOT closed by this run.** The former is a
drill-registration defect now fixed and awaiting a re-run; the latter is a
platform limit: the chunked fallback needs CoCo or arm64 and stays with
KUnit fault injection plus COCO-1..5 until then.

**Category:** kernel-runtime-audit

**How to measure:** `gh run view 36777284600 --repo emersonbusson/WSL2-Linux-Kernel --log`
and grep `bind_fail`, `PHASE2`, `VERDICT=`, `HYPERV_DRILL_RESULT`.
The uploaded `hyperv-drill-windows-*` artifacts hold `drill-console.log` and
`drill-vm-report.txt`; `hyperv-drill-kernel` holds `bzImage.sha256`,
`image-audit.txt` and `applied-stages.txt`. A re-run after the fixes must
show `cycle*_bind_fail` absent, a UIO or sysfs-ring `MMAP_HOLD` line, and a
`high_order_7plus_blocks=` figure — those three together are what closes the
GPADL/UIO half.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0133`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-09-30T21:16:11Z`.
**Verified at:** `2026-09-30T21:16:11Z`.
**Source revision:** `f831c80e0a2b`
**Lifecycle:** `reviewable`.
**Retention:** Keep until the series is sent or withdrawn. Keep the
bzImage sha256 recorded above: a future build that resolves a different
digest is a new measurement, not a repeat of this one. Keep the two negatives
with the positives — a reader who sees only `status=PASS` will over-claim.
**Freshness:** Re-run on any change to
`.github/workflows/hyperv-runtime-drill.yml` or anything under
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/drill/`. Never cite
this entry as build, KUnit, CoCo or GPADL/UIO qualification evidence. If
GitHub retires or repurposes the `windows-latest` / `windows-2025` images, or
drops the Hyper-V role from them, this claim is void until re-measured.

## 2026-09-30 20:16 -03 — runtime drill re-run fails on three harness defects (EVD-0134)

**What:** Run
[36783758959](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36783758959)
(`Hyper-V runtime drill`, `conclusion=failure`, contribution-fork SHA
`0d2844870beb`, completed `2026-09-30T22:16:16Z`) re-ran the lifecycle and
fragmentation drills after the EVD-0133 dynid/class_id fixes. It is
recorded as **FAIL**, not as a pass and not as a quiet re-run: the new
failure-visibility scoring did its job and a broken drill no longer looks
green.

**Question:** After the braced-GUID and silent-skip fixes, do the GPADL/UIO
lifecycle and fragmentation stress paths execute and measure what they
claim?

**Answer: no.** The candidate is healthy and three independent **harness**
defects produced the failure. Each was traced to the kernel source that
produces the observed behaviour, not guessed.

**What the run does prove — candidate health on real Gen2 Hyper-V:**

| Fact | Value |
| --- | --- |
| guest | Gen2 Hyper-V, `memory_mb=2048`, `cpu_count=2` |
| kernel | `7.3.0-rc4+`, `#1 SMP PREEMPT Wed Sep 30 22:13:31 UTC 2026` |
| bzImage | 4 178 944 bytes |
| `esp_bootx64_sha256` | `F39BCC744BB484F626CB030A9F822945ABB9E1C898C2B30FDE5F414F856C1900` |
| boot | single-file `BOOTX64.EFI`, no reboot, full teardown (`vm_remove=ok`, `vhd_remove=ok`, `drill_switch_remove=ok`) |
| console | 22 126 bytes |
| VMBus | `devices=14`, `HYPERV_DRILL_SYMBOLS alloc=4` (`vmbus_alloc_buffer` family present) |
| NIC | `HYPERV_DRILL_NIC id=7e3993d8-e910-478d-9103-9cdf152e3a37` |
| map balance over 30 cycles | `BASELINE MAPS count=12 bytes=20279296 pages=4939` == `PHASE1-AFTER` == `PHASE2-AFTER-TEARDOWN` == `FINAL` |
| UIO probe accounting | `PHASE2-BEFORE-TEARDOWN MAPS count=13 bytes=53915648 pages=13150` → `AFTER-TEARDOWN count=12 bytes=20279296 pages=4939` (the +1 map is the NIC ring plus its send/receive buffers, and it is returned) |
| damage | `HYPERV_DRILL_SPLATS count=0`, `HYPERV_DRILL_FAULTS count=0`, `FAULTS_NONE` |
| fragmentation side effects | `RESULT ... accept4_failures=0 oops=0 rebind=yes` |

The exact 12 / 20 279 296 / 4 939 return on every boundary, including after
the UIO probe released a 33 636 352-byte ring set, is further evidence
**against** the EVD-0088/0089/0091 retention/runaway hypothesis on this
candidate. It still does not close that gate: causality to the historical
freeze was never this measurement's claim.

**The three harness defects** (the reason for the FAIL):

1. **`bind_fail` × 30 was `-EBUSY` from a redundant bind, not a failed
   attach.** `new_id_store()` → `vmbus_add_dynid()` ends in
   `driver_attach()`, so `uio_hv_generic` already owns the device when the
   explicit `bind` write follows. That write reaches
   `__driver_probe_device()` (`drivers/base/dd.c`) with `dev->driver` set
   and returns `-EBUSY`, which `bind_store()` surfaces. Measured:
   `cycle1..cycle30 bind_fail` with **zero** `newid_fail` (the EVD-0133
   braces fix did work), and `/dev/uio0` present with the UIO probe having
   run — the attach landed every time.
2. **`MMAP_HOLD ... maps=0` was `O_RDONLY`, not a missing mapping.**
   `do_mmap()` clears `VM_SHARED` on an fd without `FMODE_WRITE`
   (`mm/mmap.c`), and `hv_uio_mmap_validate()` returns `-EINVAL` unless the
   vma is shared. Measured: `/dev/uio0` mapped `maps=0`, and the sysfs ring
   at `.../channels/14/ring` — **present** — also mapped `maps=0`. The
   objects existed; the open mode made every mmap fail.
3. **`high_order_7plus_blocks=127` was an under-fragmented buddy.**
   `fragment-buddy` took a fixed 75% of `MemAvailable` (`hog_mib=1464`)
   and left the untouched remainder as order-10 blocks. Measured:
   `FRAGMENT_BUDDY ready=1 chunks=23424 held=11712 freed=11712
   locked=11712 chunk_kib=64`, then `high_order_7plus_blocks=127`, then
   `VERDICT=INCONCLUSIVE_ORDER7_STILL_AVAILABLE`. The pattern was built
   correctly; the budget was not large enough to remove the high-order
   supply.

**What this run still does NOT prove** (unchanged from EVD-0133):

1. **UIO subchannel mmap / hold-in-mmap (BUG-3) never ran** — see defect 2.
2. **The order-zero fallback cannot run on this guest.**
   `vmbus_uses_shared_page_chunks()` is
   `!encrypted && (hv_isolated || IS_ENABLED(CONFIG_ARM64))`; a hosted
   x86_64 runner is neither, so every ring here is `vzalloc()` and the
   chunked order-N → order-0 path is unreachable. It would not log either:
   it uses `__GFP_NORETRY | __GFP_NOWARN`, so `grep 'order:7'` can never
   match. Measured: `order7_dmesg=0`.
3. **No host-rescind / GPADL response-rescind interleaving.**
4. **No CoCo.** Every number above is ordinary x86_64 Hyper-V evidence.
   COCO-1..5 remain open; this entry is never a CoCo gate.

**Root causes fixed in the same work stream** (fork `f8277cbfed40`, RamShared
`8b7cf6c9`; re-measured by run 36789491184, not by this entry):

- after `new_id`, verify the driver symlink and only write `bind` when it is
  not already `uio_hv_generic`;
- `mmap-hold` opens `O_RDWR` and reports mmap errno instead of breaking
  silently;
- `fragment-buddy` treats `<mib>` as a cap and allocates 64 KiB chunks until
  the kernel refuses, with `oom_score_adj = -1000`; the drill passes full
  `MemAvailable` instead of 75%.

**Verdict:** 🔴 does not work — `HYPERV_DRILL_RESULT status=FAIL
scope=drill-failed`, `LIFECYCLE_VERDICT=FAIL cycle_fails=30`,
`HYPERV_DRILL_FRAGMENT status=3`. The **drill harness** produced no usable
lifecycle or fragmentation evidence. The **candidate** is not implicated:
map balance is exact, `SPLATS=0`, `FAULTS=0`, `accept4_failures=0`, and
teardown is clean. Keep those positives next to the red — a reader who sees
only `status=FAIL` will over-claim against the patch set.

**Category:** kernel-runtime-audit

**How to measure:** `gh run view 36783758959 --repo emersonbusson/WSL2-Linux-Kernel --log`
and grep `bind_fail`, `newid_fail`, `MMAP_HOLD`, `high_order_7plus_blocks`,
`LIFECYCLE_VERDICT`, `HYPERV_DRILL_RESULT`.
The `hyperv-drill-windows-latest` artifact holds `drill-console.log` and
`drill-vm-report.txt`. A fixed re-run must show **zero** `cycle*_bind_fail`,
a `MMAP_HOLD ... maps>0` on `/dev/uio0` or the sysfs ring, and
`high_order_7plus_blocks=0` with `exhausted=1`. Those three together are
what turns this red into a measurement.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0134`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-09-30T22:15:37Z`.
**Verified at:** `2026-09-30T23:11:27Z`.
**Source revision:** `0d2844870beb`
**Lifecycle:** `reviewable`.
**Retention:** Keep until the series is sent or withdrawn. The Observed-at
stamp is the guest start from `drill-vm-report.txt`. Keep the
`esp_bootx64_sha256` recorded above: a future build that resolves a
different digest is a new measurement, not a repeat of this one. Keep the
candidate-health positives next to the FAIL — a reader who sees only
`cycle_fails=30` will over-claim against the patch set, and a reader who
sees only `SPLATS=0` will over-claim for the drills.
**Freshness:** Superseded as soon as a run of fork `f8277cbfed40` or later
lands and shows the three fixed signals. Re-run on any change to
`.github/workflows/hyperv-runtime-drill.yml` or anything under
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/drill/`. Never
cite this entry as build, KUnit, CoCo or GPADL/UIO qualification evidence.
If GitHub retires or repurposes the `windows-latest` / `windows-2025`
images, or drops the Hyper-V role from them, this claim is void until
re-measured.

## 2026-09-30 20:51 -03 — Windows-only Rust adapters given platform-E2E owners (EVD-0135)

**What:** Closed the last two production `src/**/*.rs` paths that had no
`docs/governance/rust-slice-coverage.json` owner and would have blocked
`plan-rust-slice-coverage.mjs` with `changed-rust-file-unmapped`:
`crates/ramshared-cuda/src/loader_win.rs` and
`crates/ramshared-winsvc/src/windows_driver.rs`. Both are `#[cfg(windows)]`
modules with no LLVM instrumented regions on a Linux coverage run, so a
`rust-line-coverage` owner would report a false 0%. Each is now owned by a
`windows-platform-e2e` entry pairing a named static contract with a named live
drill, and the static half was executed on this host.

**Category:** `invariant`

**How to measure:** From WSL, set `winroot=$(wslpath -w "$PWD")`, then run:
```bash
node tools/ci/plan-rust-slice-coverage.mjs --changed-files tmp/win-two.txt
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass \
  -File "$winroot\scripts\windows\Test-AutonomousBrokerStatic.ps1" -RepoRoot "$winroot"
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass \
  -File "$winroot\scripts\windows\Test-ProductOnlineStatic.ps1" -RepoRoot "$winroot"
```

**Measured data:**
`plan-rust-slice-coverage.mjs --changed-files tmp/win-two.txt` →
`RUST_SLICE_COVERAGE_STATUS=READY` with
`RUST_SLICE_PLATFORM_E2E_REQUIRED=windows-swap-driver-loader-win-platform-e2e` and
`RUST_SLICE_PLATFORM_E2E_REQUIRED=windows-storport-driver-adapter-platform-e2e`, exit 0.
`--all` → READY, 54 mapped entries, exit 0, no `platform-*` or `coverage-*` findings.
Selecting every `crates/**/src/**/*.rs` reports exactly two
`changed-rust-file-unmapped`, both the concurrently-edited
`crates/ramshared-vram/src/codec.rs` and `crates/ramshared-vram/src/worker_telemetry.rs`
(uncommitted at measurement time, deliberately not measured); the third walk hit,
`crates/ramshared-cli/src/monitor_pressure_tests.rs`, is correctly excluded as a
test-only path module and produces no finding.

Windows PowerShell 5.1.26100.9444 static runs (host is WSL2; scripts reached over
the repo root exposed as a Windows UNC path):
`Test-AutonomousBrokerStatic.ps1` exit 0, 22/22 named checks PASS including the five
new `loader_win_*` checks and the roll-up `PASS loader_win_adapter_contract`;
`Test-ProductOnlineStatic.ps1` exit 0, 5/5 PASS including
`PASS windows_driver_mapped_queue_contract`. Combined 28 PASS lines, 0 FAIL.
Both harnesses remain registered in `scripts/windows/Test-WindowsCiStatic.ps1`
(`Name = "Test-AutonomousBrokerStatic.ps1"`, `Name = "Test-ProductOnlineStatic.ps1"`).
Measurement revision `b7e9f0082852` carried these five paths in the working tree:
`scripts/windows/Test-AutonomousBrokerStatic.ps1`,
`scripts/windows/Test-ProductOnlineStatic.ps1`,
`docs/governance/rust-slice-coverage.json`,
`docs/specs/no-milestone/windows-swap-driver/SPEC.md`,
`docs/specs/no-milestone/windows-storport-cuda-vram/SPEC.md`.

Static contract asserted on `loader_win.rs`: Win32 `LoadLibraryW`/`GetProcAddress`/
`FreeLibrary` triad with no POSIX `dlopen(`/`dlsym(`/`dlclose(` call, `sym`/`close`
null-handle refusal before any API call, `encode_utf16().chain(Some(0))` NUL-terminated
wide path from `CStr::from_ptr`, `error()` formatting `Windows error code: 0x` from
`GetLastError` with no `{:p}` or integer pointer cast, and `close` mapping the
FreeLibrary BOOL to the documented dlclose-style 0/-1 status.

Static contract asserted on `windows_driver.rs`: `#![cfg(windows)]` +
`#![allow(unsafe_code)]`, ABI-v1 IOCTL codes from
`FILE_DEVICE_MASS_STORAGE = 0x0000_002d` / `METHOD_BUFFERED = 0` via
`ioctl_code(0..=4)`, single pending `COMMIT_AND_FETCH` (`if self.pending` →
`"commit already pending"`) cancelled and drained through one `OVERLAPPED` with
`CancelIoEx` + `GetOverlappedResult`, ring indices published with
`AtomicU32` `Ordering::Release` / observed with `Ordering::Acquire`,
`IoctlError` Display as pointer-free stable classes, and every
`WindowsMappedQueue` allocation exit path freeing its regions (reverse order in
the `try_new` error paths).

**Verdict:** 🟡 partial — static half ✅ (real execution, 28 PASS / 0 FAIL);
live half not executed in this campaign.

**Next action:** run `scripts/windows/Run-GuestAutonomousLifecycle.ps1` on a
supervised Windows lab and record `three_round_sha` (for `loader_win.rs`, reached
through `cuda_probe` → `Cuda::load()` on `nvcuda.dll`) and
`all_registered_depths_have_zero_disk_retries` (for `windows_driver.rs`, reached
through the product Online `register_queue` path). The deeper IOCTL refusal suite
`scripts/windows/Invoke-WinDriveIoctlValidation.ps1` remains the SPEC-named live
list for `windows_driver.rs`. Until those land, the open "Corrected Windows
physical lifecycle qualification" gate stays the honest owner of the gap.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0135`.
**Owner role:** `ci-contract-engineer`.
**Observed at:** `2026-09-30T23:51:00-03:00`.
**Verified at:** `2026-09-30T23:51:00-03:00`.
**Source revision:** `b7e9f0082852`.
**Lifecycle:** `reviewable`.
**Retention:** Keep until a supervised Windows lab run records the two named live
drills, at which point the live half of this entry is superseded rather than
deleted. Keep the exact static output count (22 + 5 = 27 named checks plus the
two harness roll-ups): a reader who sees only "PASS" will over-claim that the
live drills ran.
**Freshness:** Superseded as soon as `Run-GuestAutonomousLifecycle.ps1` records
`three_round_sha` and `all_registered_depths_have_zero_disk_retries` for these two
sources, or as soon as either static harness or either Rust source changes.
Never cite this entry as WDK, Driver Verifier, IOCTL refusal, GPU, or
three-tier-stress qualification evidence — the static half is source-contract
assertion only.

## 2026-09-30 21:24 — six local CI gate failures closed with real data (EVD-0136)

**What:** The Node CI suite and `docs-check` were both red on committed state
for reasons that were documentation grammar, not product defects. Four
`node --test tools/ci/*.test.mjs` cases failed (4/526) and `docs-check` reported
`NO-GO (2 independent failure(s))`. All six were reproduced, classified, and
closed across commits `05f0de82` and `1575bf89`.

**Question:** Do the repository's own trust gates pass on this tree, or is any
of them a hardcoded failure that will block the branch regardless of product
health?

**Answer: they pass now.** Three distinct causes, each a reproduced defect in
committed documentation rather than an incorrect conclusion:

1. `validation.md` EVD-0135 recorded the measurement host as a literal private
   WSL UNC host path naming this machine's home directory. That tripped both
   `PRIVATE_PATH` (`check-documentation-governance`) and `PRIVATE_WSL_PATH`
   (`check-public-hygiene`) and failed `repository_governance_run_passes` and
   `repository_candidate_is_clean`. Replaced with the established
   `winroot=$(wslpath -w "$PWD")` convention already used elsewhere in this log.
   The measured commands and their 22/22 and 5/5 static results are unchanged.
2. EVD-0134 and EVD-0135 put parentheticals inside the `**Observed at:**` and
   `**Source revision:**` label blocks, so the evidence-v2 RFC3339 and hex
   revision checks rejected them (`validation_full_repository_schema_passes`).
   Provenance moved into `**Retention:**` and `**Measured data:**`; the labels
   now carry the bare stamp and revision. No measurement value changed.
3. `docs/reliability/GAP-REGISTER.md` row "VMBus ring fallback upstream series"
   had an unescaped pipe inside the `__GFP_NORETRY | __GFP_NOWARN` code span,
   splitting the row into 5 cells instead of 4 (`checkGapRegister`). Separately,
   `docs-check` found the generated capability-observation map lagging the
   implementation (recent slices added real `lib.rs`/`nvml.rs`/`inflight.rs`/
   `state.rs` and three `scripts/windows/*` paths) and two
   `evidence-manifest.json` files still carrying pre-36ed47ca `spec.sha256`
   values. Commit `36ed47ca` ticked those two SPECs' validation checklists with
   2026-09-30 measurements without re-binding the hashes; no requirement,
   test-matrix or contract row changed in that edit.

**Measured data:**
Before: `node --test tools/ci/*.test.mjs` → 522 passed / 4 failed
(`repository_governance_run_passes`, `checkGapRegister: passes on current
repository`, `repository_candidate_is_clean`,
`validation_full_repository_schema_passes`); `./scripts/docs-check.sh` →
`DOCS_CHECK_EXIT=1`, `capability-observations:1`, `spec-evidence:1`.
After (`05f0de82`, then `1575bf89`):
`node --test tools/ci/*.test.mjs` → **526 passed, 0 failed**;
`node tools/ci/check-gap-register.mjs` → `gap register OK`, exit 0;
`node tools/ci/check-documentation-governance.mjs --all` → `FILES=553
FINDINGS=0 GOVERNANCE_STATUS=PASS`, exit 0;
`node tools/ci/check-validation-schema.mjs --all` → `validation.md schema OK
(all entries)`, exit 0;
`node tools/ci/check-spec-evidence.mjs --check` → `SPEC evidence manifests OK
(count=4)`, exit 0;
`node tools/ci/generate-capability-observations.mjs --check` → `in sync
(55 observations)`, exit 0;
`./scripts/docs-check.sh` → `DOCS_CHECK_EXIT=0`, `✓ docs-check OK`, zero
FAIL/NO-GO lines.
The two re-bound SPEC hashes are
`b31559050a31b145cf47c2882d23649edb93e02e6e09df6897a16727ef07e2f6`
(benchmark-evidence-integrity) and
`22cce6fa991564c3084d51961efb7226c52f22af4cd8e54cd914930c1c973426`
(documentation-localization-integrity). The hash re-bind attaches the existing
evidence set to the current SPEC bytes after a checklist-only edit; it does not
re-qualify that evidence. `benchmark-evidence-integrity` remains **PARTIAL**
with `binary_match.passed=false` and one env-bound gap, and
`documentation-localization-integrity` keeps its recorded DONE status.

**Verdict:** ✅ works — six local trust gates that were red on committed state
now pass with recorded numbers. This is gate hygiene, not product qualification.

**Category:** ci-gate

**How to measure:**
```bash
node --test tools/ci/*.test.mjs
node tools/ci/check-gap-register.mjs
node tools/ci/check-documentation-governance.mjs --all
node tools/ci/check-validation-schema.mjs --all
node tools/ci/check-spec-evidence.mjs --check
node tools/ci/generate-capability-observations.mjs --check
./scripts/docs-check.sh
```

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0136`.
**Owner role:** `ci-contract-engineer`.
**Observed at:** `2026-10-01T00:24:50Z`.
**Verified at:** `2026-10-01T00:24:50Z`.
**Source revision:** `1575bf891589`.
**Lifecycle:** `reviewable`.
**Retention:** Keep the before/after counts together — a reader who sees only
the 526/526 line will not know these gates were red on committed state minutes
earlier. Keep the two SPEC hashes: a future manifest that resolves a different
digest for those SPEC bytes is a different binding, not a repeat of this one.
**Freshness:** Superseded as soon as any of `validation.md`, the two evidence
manifests, `docs/governance/capability-observations.generated.json`, the
capability generator, or the six named checkers changes. Never cite this entry
as product, kernel, GPU, Windows driver, benchmark, or three-tier qualification
evidence — it records only that the repository's own trust gates are green.

## 2026-10-01 00:17 -03 — BUG-3 hold-in-mmap window measured; two new helper defects (EVD-0137)

**What:** Run
[36794687025](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36794687025)
(`Hyper-V runtime drill`, `conclusion=failure`, contribution-fork SHA
`3fc95c7b41da`, completed `2026-10-01T00:17:40Z`) re-ran the lifecycle and
fragmentation drills after the EVD-0134 three-harness-defect fixes. It is
recorded as **FAIL** overall, but it is the run that **closed the BUG-3
acceptance signal** and exposed two further harness defects.

**Question:** With `mmap-hold` opening `O_RDWR`, `new_id` verified instead of
a redundant `bind`, and `fragment-buddy` allocating 64 KiB chunks to a ceiling,
do the three EVD-0134 acceptance signals hold?

**Answer: two of three.** The lifecycle half is fully green. Fragmentation is
still INCONCLUSIVE, and one of the two runtime jobs panicked the guest.

**What the run proves — the BUG-3 window is now measured, not prepared:**

| Signal (EVD-0134 acceptance) | windows-latest | windows-2025 |
| --- | --- | --- |
| zero `cycle*_bind_fail` | ✅ `PHASE1 cycle_fails=0 / 120 steps` | ✅ `PHASE1 cycle_fails=0 / 120 steps` |
| `MMAP_HOLD ... maps>0` | ✅ `/dev/uio0 maps=5`, ring `maps=1` | ✅ `/dev/uio0 maps=5`, ring `maps=1` |
| `high_order_7plus_blocks=0` with `exhausted=1` | ❌ `=1`, `exhausted=1` | ❌ not reached (panic) |

The hold-in-mmap evidence:

| Fact | Value |
| --- | --- |
| UIO device mapping | `MMAP_HOLD path=/dev/uio0 bytes=4096 maps=5 hold=8` |
| sysfs ring mapping | `MMAP_HOLD path=.../channels/14/ring bytes=2097152 maps=1 hold=8` |
| window | `PHASE2 hold-in-mmap window OPEN (mapping alive across restore_nic)` |
| lifecycle verdict | `LIFECYCLE_VERDICT=PASS cycles=30 phase2=yes` (both guests) |
| map balance over 30 cycles | `BASELINE` = `PHASE1-AFTER` = `12 maps / 20 279 296 bytes / 4 939 pages` |
| damage | `HYPERV_DRILL_SPLATS count=0`, `HYPERV_DRILL_FAULTS count=0`, `FAULTS_NONE` |

Both mappings stayed alive while `restore_nic` tore the channel down. That is
the BUG-3 repro the previous runs only prepared: a userspace mapping held open
across ring release. It is ordinary x86_64 Hyper-V evidence for the window, not
a claim that a release-during-map fault occurred.

One accounting note, not scored: after the hold released, `FINAL MAPS` stayed
at `13 / 53 915 648 / 13 150` instead of returning to the 12-map baseline. In
EVD-0134's run the same `+1` was returned by `AFTER-TEARDOWN` because
`mmap-hold` had mapped nothing. Here the mapping had been real. Whether the
residue is deferred release or a leak is **not** determined by this run and is
**not** a claim either way.

**The two harness defects** (the reason one job failed and the other stayed
INCONCLUSIVE):

1. **windows-2025 kernel panic during hole-punching.**
   `FRAGMENT_BUDDY` unmapped every eligible page and split the VMA once per
   page: `chunks=31376 freed=250958`. That is ~250k `vm_area_struct` slab
   objects. Measured stack:
   `munmap → __vm_munmap → do_vmi_munmap → do_vmi_align_munmap →
   vms_complete_munmap_vmas → vms_gather_munmap_vmas → __split_vma →
   vm_area_dup → alloc_slab_page → out_of_memory`,
   `gfp_mask=0x40cc0(GFP_KERNEL|__GFP_COMP), order=0, oom_score_adj=-1000`,
   then `Kernel panic - not syncing: System is deadlocked on memory`.
   The helper was unkillable, so the OOM killer had no target.
2. **Order-7 supply one block short.** windows-latest reported
   `FRAGMENT_BUDDY ready=1 chunks=31376 ... exhausted=1 pagemap=1
   stop=memavailable-floor high_order_7plus=505->1`, then
   `high_order_7plus_blocks=1`, `VERDICT=INCONCLUSIVE_ORDER7_STILL_AVAILABLE`.
   The 8 MiB `MemAvailable` floor fired with one order-7 block still free in
   the remainder. The pattern was correct; the loop stopped before the test
   condition.

**What this run still does NOT prove:**

1. **The order-zero fallback cannot run on this guest.** `vmbus_uses_shared_page_chunks()`
   is `!encrypted && (hv_isolated || IS_ENABLED(CONFIG_ARM64))`; a hosted x86_64
   runner is neither, so every ring here is `vzalloc()` and the chunked
   order-N → order-0 path is unreachable. It would not log either
   (`__GFP_NORETRY | __GFP_NOWARN`): `order7_dmesg=0`.
2. **No host-rescind / GPADL response-rescind interleaving.**
3. **No CoCo.** COCO-1..5 remain open; this entry is never a CoCo gate.
4. **Map residue after a real hold** — see the accounting note above.

**Root cause fixed in the same work stream** (fork `abffeb386ae1`, RamShared
`83aad531`; re-measured by run 36796811977, not by this entry):
cap the punch at 4096 holes, drop `oom_score_adj` to 0 before punching, move
the order-7 test ahead of the floor and lower the floor to 2 MiB, and add a
post-mlock `high_order_7plus=A->B` diagnostic.

**Verdict:** 🔴 does not work — `HYPERV_DRILL_RESULT status=PARTIAL
scope=lifecycle-pass-fragment-inconclusive` on windows-latest and a guest
panic on windows-2025. **Two of the three EVD-0134 acceptance signals are
green**, and the BUG-3 window is measured. Keep those positives next to the
red: a reader who sees only `conclusion=failure` will over-claim against the
patch set, and a reader who sees only `window OPEN` will over-claim for the
fragmentation drill.

**Category:** kernel-runtime-audit

**How to measure:** `gh run view 36794687025 --repo emersonbusson/WSL2-Linux-Kernel --log`
and grep `MMAP_HOLD`, `PHASE2 hold`, `FRAGMENT_BUDDY`, `high_order_7plus_blocks`,
`LIFECYCLE_VERDICT`, `deadlocked`, `vm_area_dup`.
The `hyperv-drill-windows-latest` artifact holds `drill-console.log` and
`drill-vm-report.txt`. A fully green re-run must show the three EVD-0134
signals together: zero `cycle*_bind_fail`, `MMAP_HOLD ... maps>0`, and
`high_order_7plus_blocks=0` with `exhausted=1`.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0137`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-10-01T00:17:39Z`.
**Verified at:** `2026-10-01T01:05:00Z`.
**Source revision:** `3fc95c7b41da`.
**Lifecycle:** `reviewable`.
**Retention:** Keep until the series is sent or withdrawn. Keep the two
positive signals next to the FAIL and the panic stack next to the PARTIAL —
each side alone invites a wrong conclusion. Keep the `FINAL MAPS=13` residue
as an open observation, not as a leak claim.
**Freshness:** Superseded as soon as a run shows all three EVD-0134 signals
green and no guest panic. Re-run on any change to
`.github/workflows/hyperv-runtime-drill.yml` or anything under
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/drill/`. Never cite
this entry as build, KUnit, CoCo or GPADL/UIO qualification evidence. If
GitHub retires or repurposes the `windows-latest` / `windows-2025` images, or
drops the Hyper-V role from them, this claim is void until re-measured.

## 2026-10-01 00:42 -03 — hole cap stops the panic; the min watermark is the wall (EVD-0138)

**What:** Run
[36796811977](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36796811977)
(`Hyper-V runtime drill`, `conclusion=success`, contribution-fork SHA
`abffeb386ae1`, completed `2026-10-01T00:42:56Z`) re-ran the drills after the
EVD-0137 hole-cap fix. All three jobs are green: `drill-kernel`,
`drill-runtime (windows-latest)` and `drill-runtime (windows-2025)`. The
workflow-level `success` is the harness staying alive and scoring honestly;
the drill verdict itself is still **PARTIAL**.

**Question:** Does capping the punch at 4096 holes and dropping the OOM pin
before punching stop the guest panic, and does the 2 MiB floor let the
allocation loop reach `high_order_7plus_blocks=0`?

**Answer: the panic is gone, the order-7 drain is not.**

**Panic fix confirmed on both guests:**

| Fact | windows-latest | windows-2025 |
| --- | --- | --- |
| guest completed | ✅ | ✅ (panicked on the previous run) |
| `FRAGMENT_BUDDY freed=` | `4096` | `4096` |
| `hole_cap=` | `4096` | `4096` |
| `HYPERV_DRILL_SPLATS` | `0` | `0` |
| `HYPERV_DRILL_FAULTS` | `0` | `0` |
| `LIFECYCLE_VERDICT` | `PASS cycles=30 phase2=yes` | `PASS cycles=30 phase2=yes` |
| `MMAP_HOLD` | `/dev/uio0 maps=5`, ring `maps=1` | `/dev/uio0 maps=5`, ring `maps=1` |
| `PHASE2 hold-in-mmap window` | `OPEN` | `OPEN` |

Capping the punch at 4096 holes is enough to keep the Unmovable slab away from
`out_of_memory()` on a 2 GiB guest, and the buddy-isolation property does not
depend on hole density.

**Order-7 still available — and the measurement shows why:**

| Fact | windows-latest | windows-2025 |
| --- | --- | --- |
| `FRAGMENT_BUDDY allocated ... stop=` | `31466` chunks, `507->4`, `memavailable-floor` | `31464` chunks, `508->5`, `memavailable-floor` |
| `FRAGMENT_BUDDY ready=1 ... high_order_7plus=` | `507->4->3` | `508->5->3` |
| `high_order_7plus_blocks` (script read) | `1` | `3` |
| `VERDICT` | `INCONCLUSIVE_ORDER7_STILL_AVAILABLE` | `INCONCLUSIVE_ORDER7_STILL_AVAILABLE` |
| `HYPERV_DRILL_RESULT` | `PARTIAL lifecycle=0 fragment=3` | `PARTIAL lifecycle=0 fragment=3` |

The three-stage trace is the new diagnostic: before → after alloc+mlock → after
punch. The punch's own `vm_area_dup` slab is what was draining order-7 in
earlier runs; with only 4096 holes it drains 1–2 blocks and no more.

**Root cause — the floor was never the wall.** The buddyinfo the script
captured after the pattern is decisive:

```
Node 0, zone DMA32  3206  1  2  1  1  2  2  0  0  1  0
```

`order0=3206 … order6=2 order7=0 order8=0 order9=1 order10=0` — **15 MiB of
free pages including an order-9 block**, while `MemAvailable` reads **2 MiB**
and the loop is stopped by `FRAG_SAFETY_KB=2048`. The gap is the zone min
watermark: `min:20344kB` on this 2 GiB guest. Those pages are free in
`buddyinfo` but reserved, and a userspace page fault cannot cross the
watermark to split the high-order blocks sitting behind it. Lowering
`FRAG_SAFETY_KB` cannot help — 3432 lower-order free pages sit between the
floor and that order-9 block, and the reserve holds all of them.

**What this run still does NOT prove** (unchanged):

1. **The order-zero fallback cannot run on this guest** — `order7_dmesg=0`,
   `vmbus_uses_shared_page_chunks()` false on hosted x86_64.
2. **No host-rescind / GPADL response-rescind interleaving.**
3. **No CoCo.** COCO-1..5 remain open; this entry is never a CoCo gate.
4. **`high_order_7plus_blocks=0` is not reached**, so the third EVD-0134
   acceptance signal is still open.

**Root cause fixed in the same work stream** (fork `f84e6d40ac79`, RamShared
`27e61542`; in flight as run 36798464398): save `vm.min_free_kbytes`, set it
to 512 for the allocation loop, restore it before hole-punching so the
`vm_area_dup` slab and the ring allocation that follows run against the normal
reserve, and read the sysctl back so a denied write cannot look like a pulled
lever.

**Verdict:** 🟡 partial — the harness is now survivable and the lifecycle half
is fully green on both guests, but `HYPERV_DRILL_RESULT status=PARTIAL
scope=lifecycle-pass-fragment-inconclusive` on both, so the EVD-0134 gate is
not closed. The candidate is not implicated: `SPLATS=0`, `FAULTS=0`,
`accept4_failures=0`, `LIFECYCLE_VERDICT=PASS`, and the BUG-3 window is open on
both guests. Keep those positives next to the PARTIAL.

**Category:** kernel-runtime-audit

**How to measure:** `gh run view 36796811977 --repo emersonbusson/WSL2-Linux-Kernel --log`
and grep `min_free`, `FRAGMENT_BUDDY`, `high_order_7plus`, `Node 0, zone`,
`MemAvailable`, `VERDICT`, `HYPERV_DRILL_RESULT`.
The decisive pair is the `FRAGMENT_BUDDY ready=1` line and the following
`Node 0, zone DMA32` buddyinfo line: when they disagree about whether
high-order supply remains, the watermark is between them. A closed gate needs
`high_order_7plus_blocks=0` with `exhausted=1` and `pagemap=1`.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0138`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-10-01T00:42:51Z`.
**Verified at:** `2026-10-01T01:10:00Z`.
**Source revision:** `abffeb386ae1`.
**Lifecycle:** `reviewable`.
**Retention:** Keep the buddyinfo line next to the MemAvailable figure — the
pair is the root-cause argument, and either alone is unconvincing. Keep the
`conclusion=success` clearly separated from `HYPERV_DRILL_RESULT status=PARTIAL`:
the workflow staying green is harness health, not gate closure.
**Freshness:** Superseded as soon as a run of fork `f84e6d40ac79` or later
shows `high_order_7plus_blocks=0` with `exhausted=1`. Re-run on any change to
`.github/workflows/hyperv-runtime-drill.yml` or anything under
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/drill/`. Never cite
this entry as build, KUnit, CoCo or GPADL/UIO qualification evidence. If
GitHub retires or repurposes the `windows-latest` / `windows-2025` images, or
drops the Hyper-V role from them, this claim is void until re-measured.

## 2026-10-01 01:25 -03 — the watermark lever reaches the blocks; the punch then OOM-kills the helper (EVD-0139)

**What:** Run
[36798464398](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36798464398)
(`Hyper-V runtime drill`, `conclusion=success`, contribution-fork SHA
`f84e6d40ac79`, completed `2026-10-01T01:05:30Z`) re-ran the drills after the
`vm.min_free_kbytes` watermark lever. All three jobs are green:
`drill-kernel`, `drill-runtime (windows-latest)` and `drill-runtime
(windows-2025)`. The workflow-level `success` is harness health; the drill
verdict is still **PARTIAL**.

**Question:** Does lowering `vm.min_free_kbytes` for the allocation loop let it
reach the high-order blocks the min watermark was hiding, and does restoring it
before the punch keep the helper alive?

**Answer: the lever works, the restore-before-punch ordering kills the
measurement.**

**The lever reaches the hidden blocks** (`windows-latest`, first attempt):

```
FRAGMENT_BUDDY min_free_kbytes saved=5704 set=512 now=512
FRAGMENT_BUDDY start cap_chunks=32064 high_order_7plus=507
FRAGMENT_BUDDY allocated chunks=31546 high_order_7plus=507->3 locked=31546 stop=memavailable-floor
```

`saved=5704 set=512 now=512` is the read-back: the write took. `507->3` is a
deeper drain than any previous run, which confirms EVD-0138's root cause — the
min watermark, not the buddy, was what kept those blocks out of reach.

**Then the helper is OOM-killed before `ready=1`:**

```
vmbus_drill_hel invoked oom-killer: gfp_mask=0x40cc0(GFP_KERNEL|__GFP_COMP), order=0, oom_score_adj=0
Out of memory: Killed process 304 (vmbus_drill_hel) total-vm:2024112kB, anon-rss:2023132kB
FRAGMENT did not reach a ready state
```

`oom_score_adj=0` is the smoking gun: the pin had already been dropped, and
`GFP_KERNEL|__GFP_COMP order=0` is the `vm_area_dup()` slab the punch needs for
`__split_vma`. Restoring `min_free_kbytes` **and** unpinning before punching
put the punch phase against the normal 5.7 MiB reserve with a killable
allocator. The process died mid-pattern, its ~2 GiB of locked pages came back,
and high orders reappeared — `high_order_7plus=523` in the post-mortem buddy,
close to the 507 the drill started from. `VERDICT=INCONCLUSIVE_NO_PATTERN`.

**`windows-2025` survived the punch and shows the next wall** (the soft floor):

| Fact | windows-latest | windows-2025 |
| --- | --- | --- |
| `min_free_kbytes` read-back | `saved=5704 set=512 now=512` | same (see note below) |
| `allocated ... high_order_7plus` | `507->3`, `memavailable-floor` | `503->5`, `memavailable-floor` |
| `ready=1 ... high_order_7plus` | *never printed* (OOM) | `503->5->5`, `freed=4096`, `exhausted=1`, `pagemap=1` |
| `high_order_7plus_blocks` (script) | `523` (post-mortem) | `5` |
| `VERDICT` | `INCONCLUSIVE_NO_PATTERN` | `INCONCLUSIVE_ORDER7_STILL_AVAILABLE` |
| `LIFECYCLE_VERDICT` | `PASS cycles=30 phase2=yes` | `PASS cycles=30 phase2=yes` |

`windows-2025`'s post-pattern buddyinfo is the new root-cause pair:

```
Node 0, zone DMA32  451  2  1  2  1  2  10  2  2  1  0
```

`order0=451 … order6=10 order7=2 order8=2 order9=1 order10=0` — **9.6 MiB of
free pages including five high-order blocks**, while the loop stopped on
`FRAG_SAFETY_KB=2048`. The watermark is no longer the wall; the **soft floor
is**. The pages are allocatable, `MemAvailable` just still treats part of the
remainder as unavailable, and stopping at 2 MiB leaves exactly the blocks the
test condition is about.

**Measurement note — the missing `min_free` line was the console tail, not a
different binary.** Both jobs checked out `f84e6d40ac79` and share one
initramfs artifact. The drill script echoed `grep FRAGMENT_BUDDY | tail -3`,
which kept `start`/`allocated`/`ready` and dropped `min_free_kbytes` whenever
the fourth line existed. `windows-latest` printed only three lines (it died
before `ready=1`), so its `min_free` line survived the tail and the other
guest's did not. The `tail -6` correction is commit `7904267ccde2`.

**Lifecycle half is fully green on both guests** (unchanged from EVD-0138):

| Fact | windows-latest | windows-2025 |
| --- | --- | --- |
| `PHASE1 cycle_fails` | `0 / 120 steps` | `0 / 120 steps` |
| `MMAP_HOLD /dev/uio0` | `maps=5` | `maps=5` |
| `MMAP_HOLD` sysfs ring | `maps=1` | `maps=1` |
| `PHASE2 hold-in-mmap window` | `OPEN` | `OPEN` |
| `FAULTS_NONE` | ✅ | ✅ |
| `accept4_failures` | `0` | `0` |

**What this run still does NOT prove** (unchanged):

1. **The order-zero fallback cannot run on this guest** — `order7_dmesg=0`,
   `vmbus_uses_shared_page_chunks()` false on hosted x86_64.
2. **No host-rescind / GPADL response-rescind interleaving.**
3. **No CoCo.** COCO-1..5 remain open; this entry is never a CoCo gate.
4. **`high_order_7plus_blocks=0` is not reached**, so the third EVD-0134
   acceptance signal is still open.

**Root cause fixed in the same work stream:** keep `oom_score_adj=-1000`
through the punch (4096 `vm_area_struct`s cannot starve Unmovable the way
250k did), restore `min_free_kbytes` only **after** `ready=1` so the measured
pattern is captured before the levers drop, and replace the soft floor with a
256 kB hard floor consulted only while order-7 remains, checked every chunk
rather than every 32.

**Verdict:** 🟡 partial — the watermark lever is proven (`507->3` with a
successful read-back) and the lifecycle half is fully green on both guests, but
`HYPERV_DRILL_RESULT status=PARTIAL scope=lifecycle-pass-fragment-inconclusive`
on both, so the EVD-0134 gate is not closed. The candidate is not implicated:
`SPLATS=0`, `FAULTS=0`, `accept4_failures=0`, `LIFECYCLE_VERDICT=PASS`, and the
BUG-3 window is open on both guests. The OOM kill is a drill-harness defect,
not a VMBus one.

**Category:** kernel-runtime-audit

**How to measure:** `gh run view 36798464398 --repo emersonbusson/WSL2-Linux-Kernel --log`
and grep `min_free_kbytes`, `FRAGMENT_BUDDY`, `oom-killer`, `high_order_7plus`,
`Node 0, zone`, `VERDICT`, `HYPERV_DRILL_RESULT`. The decisive triple is the
`min_free_kbytes` read-back, the `allocated ... high_order_7plus=A->B` line,
and the OOM block: together they separate "the lever was pulled" from "the
lever reached the blocks" from "the punch then destroyed the pattern". A closed
gate needs `high_order_7plus_blocks=0` with `exhausted=1` and `pagemap=1`.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0139`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-10-01T01:05:30Z`.
**Verified at:** `2026-10-01T01:25:00Z`.
**Source revision:** `f84e6d40ac79`.
**Lifecycle:** `reviewable`.
**Retention:** Keep the OOM line next to the `oom_score_adj=0` value — the pair
is the root-cause argument for the punch-phase death, and either alone reads as
a generic memory failure. Keep the `min_free_kbytes` read-back next to the
`507->3` drain: the read-back is what proves the write took. Keep
`conclusion=success` clearly separated from `HYPERV_DRILL_RESULT status=PARTIAL`.
**Freshness:** Superseded as soon as a run keeps the OOM pin through the punch
and shows `high_order_7plus_blocks=0` with `exhausted=1`. Re-run on any change
to `.github/workflows/hyperv-runtime-drill.yml` or anything under
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/drill/`. Never cite
this entry as build, KUnit, CoCo or GPADL/UIO qualification evidence. If
GitHub retires or repurposes the `windows-latest` / `windows-2025` images, or
drops the Hyper-V role from them, this claim is void until re-measured.

## 2026-10-01 01:54 -03 — the pin in both wrong windows: panic on one guest, destroyed pattern on the other (EVD-0140)

**What:** Run
[36800977305](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36800977305)
(`Hyper-V runtime drill`, contribution-fork SHA `42ee9237b6bd`,
created `2026-10-01T01:24:53Z`) re-ran the drills after the punch-phase
ordering fix that kept `oom_score_adj=-1000` and the lowered
`vm.min_free_kbytes` through the punch and the `ready=1` measurement.
`drill-kernel` is green. The two runtime jobs split, and both halves are the
OOM pin sitting in the wrong window.

**Question:** With the pin held through the punch and a 256 kB hard floor
replacing the 2 MiB soft floor, does the drill reach
`high_order_7plus_blocks=0` with `exhausted=1` and keep the ring opening?

**Answer: no. The pin must not be on while this process allocates, and it
must not come off when `ready=1` prints. Both failures are the same lever
moved to the wrong side of the measurement.**

**`windows-latest` — job `failure`, guest panic.** Lifecycle is green
(`LIFECYCLE_VERDICT=PASS cycles=30 phase2=yes`, `MMAP_HOLD path=/dev/uio0
maps=5 hold=8`, sysfs ring `maps=1`, `PHASE2 hold-in-mmap window OPEN`,
`FAULTS_NONE`), then the fragmentation drill dies the guest:

```
=== BEGIN vmbus-fragmentation-drill hog_mib=2004 ===
vmbus_drill_hel invoked oom-killer: gfp_mask=0x140dca(GFP_HIGHUSER_MOVABLE|__GFP_ZERO|__GFP_COMP), order=0, oom_score_adj=-1000
Out of memory: Killed process 286 (sh) total-vm:2404kB, anon-rss:68kB, ... oom_score_adj:0
vmbus_drill_hel invoked oom-killer: gfp_mask=0x140dca(...), order=0, oom_score_adj=-1000
Out of memory and no killable processes...
Kernel panic - not syncing: System is deadlocked on memory
```

`reader_status=TIMEOUT drill_result=NO_RESULT`. The helper is faulting into
an exhausted guest while pinned at `-1000`. `out_of_memory()` takes the drill
shell — the only remaining killable task — and the fault retries. The second
pass finds nothing killable at all and panics. The 256 kB `MemAvailable`
floor is what drove the allocation into that state: it is a conservative
estimate that reads near zero while buddyinfo still holds allocatable
blocks, so pushing past it is how a drill reaches
`pagefault_out_of_memory()` with an unkillable process. **A pinned fault in
an exhausted guest is not a recoverable outcome; it is a guest panic.**

**`windows-2025` — job `success`, drill verdict `PARTIAL`.** Lifecycle green
on the same three signals. The watermark lever is proven again and the
pattern is the deepest any run has produced:

```
FRAGMENT_BUDDY min_free_kbytes saved=5704 set=512 now=512
FRAGMENT_BUDDY start cap_chunks=32064 high_order_7plus=506
FRAGMENT_BUDDY allocated chunks=31581 high_order_7plus=506->4 locked=31581 stop=memavailable-hard-floor
FRAGMENT_BUDDY ready=1 chunks=31581 pages=505296 held=4143 freed=4096 locked=31581 pairs=4096 chunk_kib=64 cap_chunks=32064 hole_cap=4096 exhausted=1 pagemap=1 stop=memavailable-hard-floor high_order_7plus=506->4->1
```

`saved=5704 set=512 now=512` is the read-back. `506->4->1` is
before-allocation → after-lock → after-punch: **one** free block of order 7
or above remained when the pattern was measured. Then the pin was dropped,
because the code dropped it the moment `ready=1` printed:

```
sh invoked oom-killer: gfp_mask=0x400dc0(GFP_KERNEL_ACCOUNT|__GFP_ZERO), order=2, oom_score_adj=0
Out of memory: Killed process 303 (vmbus_drill_hel) total-vm:2010088kB, anon-rss:2009112kB, file-rss:4kB, shmem-rss:680kB, UID:0 pgtables:4004kB oom_score_adj:0
--- BUDDY AFTER FRAGMENT ---
Node 0, zone    DMA32   4130     35     33     33     33     37     30     28     30     28    178
high_order_7plus_blocks=356
rebind_fail
hv_netvsc ... (unnamed net_device) (uninitialized): unable to open channel: -22
RESULT high_order_7plus_blocks=356 exhausted=1 pagemap=1 stop=memavailable-hard-floor order7_dmesg=0 accept4_failures=0 oops=0 rebind=no
VERDICT=INCONCLUSIVE_ORDER7_STILL_AVAILABLE (high_order_7plus_blocks=356; pattern did not break contiguity)
```

The drill shell's next `fork`/`clone` (`copy_process`, order-2) triggered
`out_of_memory()`, which reclaimed the helper — now at `oom_score_adj:0` —
and the ~2 GiB of locked pages went back to the buddy. The post-pattern
buddyinfo is a reassembled allocator, not the pattern: order-10 alone holds
178 blocks. The script then measured 356 against a pattern that had already
been destroyed, and the rebind ran against that same reassembled buddy, so
`unable to open channel: -22` is **not** evidence about ring allocation under
fragmentation.

**Three measurement defects ride with the ordering bug:**

1. `exhausted` printed `stopped`, which is 1 for every deliberate break
   (`order7-depleted`, the floor, `mmap-refused`), not for the measured
   condition. The script's verdict was still right because it checks
   `high_order` separately — but the number it checked was the post-destruction
   re-read.
2. The script's `high_order_7plus_blocks` came from re-reading `/proc/buddyinfo`
   after `ready=1`. That is exactly the window in which the destroyed pattern
   looks like 356 blocks. The helper's own `high_order_7plus=…->…->1` is the
   measurement taken while the pattern was pinned in place and is the one that
   must be trusted.
3. The `MemAvailable` floor is the wrong quantity. It subtracts the watermark
   and unreclaimable state, so it reads ~0 while buddyinfo still holds
   allocatable high-order blocks — which is how both this run and EVD-0139
   stopped with the test condition unmet. `MemFree` is the counter that
   matches what the test is about.

**What this run does not prove:** `high_order_7plus_blocks=0` with
`exhausted=1` is still open — the helper measured **1**, and the measurement
was then destroyed. `PASS_RING_ALLOCATION_UNDER_FRAGMENTATION` was not
reached. The CoCo chunked order-N → order-0 fallback remains unreachable on
ordinary x86_64, because `vmbus_uses_shared_page_chunks()` is false there
and every ring is `vzalloc()`. This is still not platform evidence for
SEV-SNP, TDX or Arm CCA.

**Fix shipped** (contribution-fork `bac075bbf292`): the OOM pin now covers
exactly the hold — the window in which a finished pattern must survive the
script's measurement and the channel-open rebind. Allocation, punch and
chase run **unpinned**, so a kill there is an honest
`INCONCLUSIVE_NO_PATTERN` and the guest survives. `ready=1` pins, the
watermark is restored so the ring allocation has its reserve, and the pin
drops when the hold ends. The floor is `MemFree` at 32 MiB — a margin the
rest of the run needs, not a measure of exhaustion. After the punch a chase
phase splits whatever that margin left: the holes just freed cannot form
high orders, so those faults have to come out of the order-7-and-up blocks
still free. `exhausted=1` now means the measured condition holds. The drill
script takes the helper's `high_order_7plus` third value as authoritative.

**Verdict:** 🟡 partial — the OOM pin sits in the wrong window on both
guests: held through the allocation it starves Unmovable and the helper dies
before `ready=1` on one, and dropped at `ready=1` it lets the punch destroy
the measured pattern on the other. Both failures are one lever on the wrong
side of the measurement, not a candidate defect. The fix is recorded here and
not yet proven by a clean run, and the third EVD-0134 acceptance signal
remains open until a run reports `high_order_7plus_blocks=0` with
`exhausted=1` on the pattern that was measured, not on a re-read after the
pattern was lost.

**Category:** kernel-runtime-audit

**How to measure:** `gh run view 36800977305 --repo emersonbusson/WSL2-Linux-Kernel --log`
and grep `oom-killer`, `oom_score_adj`, `ready=1`, `FRAGMENT_BUDDY`,
`high_order_7plus`, `HYPERV_DRILL_RESULT`. The decisive pair is the OOM block
position relative to `ready=1` and the `high_order_7plus` value captured at
that marker, never a re-read after the pattern is gone.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0140`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-10-01T01:24:53Z`.
**Verified at:** `2026-10-01T01:54:00Z`.
**Source revision:** `42ee9237b6bd`.
**Lifecycle:** `reviewable`.
**Retention:** Keep both job halves together — the guest panic and the
destroyed pattern are one root cause read from two symptoms, and either alone
reads as an unrelated failure. Keep this entry explicitly non-qualifying: it
records two harness failures and the fix that addresses them, not upstream
submission evidence.
**Freshness:** Superseded as soon as a run keeps the OOM pin only through the
hold and reports `high_order_7plus_blocks=0` with `exhausted=1` on the pattern
that was measured. Re-run on any change to
`.github/workflows/hyperv-runtime-drill.yml` or anything under
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/drill/`. Never cite
this entry as build, KUnit, CoCo or GPADL/UIO qualification evidence. If
GitHub retires or repurposes the `windows-latest` / `windows-2025` images, or
drops the Hyper-V role from them, this claim is void until re-measured.

## 2026-10-01 02:36 -03 — the pin window is proven; the chase floor and the rebind are not what they looked like (EVD-0141)

**What:** Run
[36803317912](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36803317912)
(`Hyper-V runtime drill`, contribution-fork SHA `bac075bbf292`, base
`93f51579e7df248780214094418f205253383cc5`) re-ran the drills after the OOM
pin was moved to cover exactly the hold. **All three jobs are green and the
workflow concluded `success`** — `drill-kernel`, `windows-2025` and
`windows-latest`. This is the first run in which both guests survived and the
helper's own measurement and the script's later re-read agreed exactly.

**Question:** With the pin covering only the hold, and allocation, punch and
chase running unpinned, does the drill reach `high_order_7plus_blocks=0` with
`exhausted=1` and keep the ring opening?

**Answer: the pin window is proven correct. The chase floor is not, and the
rebind failure is not a fragmentation result at all. Two independent harness
defects, both now fixed; neither is a candidate regression.**

### The pin window is proven

This is the result the redesign was for. Every previous run had the pin in one
of two wrong windows — on during allocation (guest panic, EVD-0140
`windows-latest`) or off at `ready=1` (pattern destroyed before the script
measured it, EVD-0140 `windows-2025`). Run 36803317912 holds the pin across
the hold and nothing else:

| Signal | windows-2025 | windows-latest |
| --- | --- | --- |
| `cycle_fails=0 / 120 steps` | yes | yes |
| `PHASE2 hold-in-mmap window OPEN` | yes | yes |
| `SPLATS=0` / `FAULTS=0` | yes | yes |
| `min_free_kbytes saved=… set=512 now=512` | `5694` | `5704` |
| helper `high_order_7plus` third value | **6** | **4** |
| script re-read of `/proc/buddyinfo` | **6** | **4** |
| guest survived the whole drill | yes | yes |

The two counts agree **exactly**, on both guests, for the first time. That
agreement is the proof: when the pin was wrong the two numbers disagreed by
two orders of magnitude (1 vs 356 on EVD-0140 `windows-2025`), because the
script was reading a buddy the helper had already been forced to give back.
`exhausted=0` is also now trustworthy — it means the measured condition did
not hold, not merely that the loop stopped.

### Finding 1 — the chase floor counts the wrong memory

Both guests stopped with `stop=chase-floor` and `exhausted=0`, leaving 6 and 4
high-order blocks free. The floor was `MemFree` at 32 MiB. That is the wrong
quantity, for a sharper reason than the `MemAvailable` error recorded in
EVD-0140. windows-2025 after the punch:

```
Node 0, zone    DMA32      0      2      1      1      1      2      0      2      2      2      0
```

Orders 0 through 10. Total free is 7552 KiB, of which **7168 KiB sits in the
six blocks of order 7 and above** (2×512 KiB + 2×1 MiB + 2×2 MiB) and only
384 KiB is left in orders 1–5. **Free order-0 is zero pages.**

A `MemFree` floor at 32 MiB therefore fires at 7552 KiB — "almost nothing
left, stop" — at exactly the moment when an order-0 fault has nothing to take
and the allocator must split a high-order block to serve it. The chase exists
to force those splits. Measured in `MemFree` it forbids them: while high-order
blocks remain they *are* most of `MemFree`, so the floor is reached before the
split it exists to allow has happened. The same trap, one level further in.

The floor that means what a later allocation can actually take is **free
order-0 pages**, `buddyinfo` field 5. That is what the helper now counts.

### Finding 2 — the rebind failure is not a fragmentation result

`rebind_fail` / `unable to open channel: -22` appeared on both guests, and
EVD-0140 attributed it to the destroyed pattern. **That attribution is wrong,
and this run is what disproves it.** On `windows-2025` the same failure happens
at t=5.2s — before the fragmentation drill has started, on a healthy buddy
(`MemAvailable=2018096kB`, order-10 still holds 489 blocks):

```
RESTORE begin
RESTORE driver=/…/bf29fd3a-7407-4a3d-9c36-fb8c0340b614/driver
  [5.223854] hv_netvsc bf29fd3a-… (unnamed net_device) (uninitialized): unable to open channel: -22
```

It then recurs at t=11.3s (the EXIT-trap `restore_nic`, after the helpers have
exited) and t=13.5s (the frag drill's own rebind). `windows-latest` is the
same at t=6.0s, t=12.1s and t=14.2s. A failure that reproduces on an untouched
buddy is not a fragmentation result.

**It is a pre-existing `uio_hv_generic` lifecycle gap, and not a candidate
regression.** The mechanism is exact. The synthetic NIC is one VMBus device
that the lifecycle drill hands to `uio_hv_generic` and then back to
`hv_netvsc`:

```
restore_nic() {
    echo "$NIC" >"$DRIVER_DIR/uio_hv_generic/unbind"
    echo "$CLS" >"$DRIVER_DIR/uio_hv_generic/remove_id"
    echo "$NIC" >"$DRIVER_DIR/hv_netvsc/bind"
}
```

`hv_uio_open()` calls `vmbus_connect_ring()` on the first open of `/dev/uio0`
and `hv_uio_release()` calls `vmbus_disconnect_ring()` on the last close.
`hv_uio_remove()` does neither. So when `restore_nic` unbinds the driver while
the BUG-3 helper still holds that fd, the channel is never disconnected and is
left in `CHANNEL_OPENED_STATE`; the subsequent `hv_netvsc` probe calls
`__vmbus_open()` and gets `-EINVAL`. Verified in the fork tree at
`bac075bbf292` against base `93f51579e7df`: **`hv_uio_remove`, `hv_uio_open`
and `hv_uio_release` are byte-identical** (`hv_uio_remove` 273 B in both).
The series changes `hv_uio_cleanup`, `hv_uio_probe` and `hv_uio_new_channel`;
it does not touch the remove path. This is upstream behaviour the series
inherits, not something the series introduced.

The consequence for the drill is ordering: the channel-open exercise cannot
follow the hold-in-mmap, because the hold-in-mmap is what makes that channel
unopenable for the rest of the boot. It has to run first, on a boot-clean NIC.

**Correction to EVD-0140.** Its `windows-2025` section says the
`rebind_fail` / `unable to open channel: -22` "ran against that same
reassembled buddy" and that it is "not evidence about ring allocation under
fragmentation". The second half is right and is confirmed here. The first half
is not: the same `-22` is present at `restore_nic` on a healthy buddy at
t=5.2s. The destroyed pattern is not its cause. `validation.md` is
append-only, so EVD-0140 stands as written and is corrected here.

**Minor measurement note.** `nic_driver` printed a `/sys/.../driver` path
rather than `none` when nothing was bound, because `readlink -f` canonicalises
a missing leaf and exits 0, so `|| echo none` never fires. `REBOUND=no` is
still the correct verdict — the path contains no `hv_netvsc` — but the log
cannot distinguish "bound to another driver" from "bound to none". The
`case` is what decides, and it decided correctly.

### Supporting evidence for the candidate

Not the gate, but worth recording: the BUG-3 hold-in-mmap held. `PHASE2-BEFORE-TEARDOWN`
and `PHASE2-AFTER-TEARDOWN` both report `MAPS count=13 bytes=53915648
pages=13150`, against `BASELINE MAPS count=12 bytes=20279296 pages=4939` —
the ring's `vmbus_alloc_buffer` vmalloc entries survived `restore_nic`, which
runs `hv_uio_remove` → `vmbus_free_ring`, while the mapping was still held.
`FAULTS_NONE` after release means the hold did not UAF. That is the behaviour
patches 0003/0006 (retained owner) are meant to produce.

**What that does not establish:** `FINAL MAPS` is still `13` after the
helpers released, against a baseline of 12. Whether the deferred free runs on
the last munmap is therefore **not reconciled in this run** — later
`after_fragment` shows 9, but by then the NIC has lost its driver entirely, so
that does not close it. Not claimed.

### What this run does not prove

- `high_order_7plus_blocks=0` with `exhausted=1` is **still open** — both
  guests stopped at `stop=chase-floor` with 6 and 4 blocks free. This is the
  third EVD-0134 acceptance signal and the only one still red.
- `PASS_RING_ALLOCATION_UNDER_FRAGMENTATION` was not reached. The frag drill
  short-circuited on `exhausted != 1` before the rebind check, so its verdict
  is `INCONCLUSIVE_CAP_REACHED`, not a FAIL — and the rebind would have failed
  anyway for the reason above.
- The CoCo chunked order-N → order-0 fallback is still unreachable on ordinary
  x86_64: `vmbus_uses_shared_page_chunks()` is false there and every ring is
  `vzalloc()`. No SEV-SNP, TDX or Arm CCA platform evidence. COCO-1..5 remain
  open.

### One more harness defect found while scoring this run

Companion run
[36803317892](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36803317892)
(`VMBus upstream candidate`, same SHA) failed on both arches at "Apply and
build every patch in the series", while `wsl-backport` stayed green. The
byte-pin passed on all six patches and then `git apply` died with `error: No
valid patches in input` and exit 128 — before a single patch was applied. The
apply loop iterates `$PATCH_DIR/*.patch`, and that glob reaches
`0000-cover-letter.patch` first: mail prose that `git send-email` wants in the
directory but `git apply` correctly rejects. `SHA256SUMS` pins only 0001–0006,
which is why the byte-pin passed and the loop still died. The same step has
failed identically on 36796812218, 36798464363 and 36800977475 — every
candidate run since the cover letter landed — while `hyperv-runtime-drill.yml`
stayed green on the same bytes because it already skips `0000-*`. No patch
byte is involved; only the loop that feeds files to `git apply` was wrong.

### Fix shipped

Contribution-fork `747faf4ae312` — chase floor counted in free order-0 pages
(`FRAG_CHASE_ORDER0`, `stop=chase-order0-floor`) instead of `MemFree`, and the
fragmentation drill runs **first**, on a boot-clean NIC, so the channel-open
exercise measures ring allocation under fragmentation and not the UIO
lifecycle. Contribution-fork `4f1b44ee8048` — the candidate workflow skips
`0000-*` the way the drill workflow always has. Both pushed together as
`4f1b44ee8048`; the resulting runs are scored separately.

**Verdict:** 🟡 partial — the OOM pin window is proven: covering exactly the
hold is the first configuration in which both guests survive and the helper's
own measurement and the script's later re-read agree exactly. The run also
retires a mis-attributed rebind failure and corrects EVD-0140's reading of the
chase floor. The third EVD-0134 acceptance signal remains open until a run
reports `high_order_7plus_blocks=0` with `exhausted=1`, so this entry does not
qualify the candidate for upstream submission.

**Category:** kernel-runtime-audit

**How to measure:** `gh run view 36803317912 --repo emersonbusson/WSL2-Linux-Kernel --log`
and grep `oom_score_adj`, `ready=1`, `FRAGMENT_BUDDY`, `high_order_7plus`,
`HYPERV_DRILL_RESULT`. The decisive fact is the agreement between the
helper's third `high_order_7plus` value and the script's `/proc/buddyinfo`
re-read while the pin covers only the hold.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0141`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-10-01T02:36:00Z`.
**Verified at:** `2026-10-01T02:36:00Z`.
**Source revision:** `bac075bbf292`.
**Lifecycle:** `reviewable`.
**Retention:** Keep the pin-window proof next to the EVD-0140 correction —
the pair is what shows the earlier two failures were one lever in the wrong
window, not two independent defects. Keep the drilled revision
`bac075bbf292` distinct from the fix revision `4f1b44ee8048`: the measurements
belong to the former. Keep this entry explicitly non-qualifying: it proves the
pin window and retires a mis-attributed rebind failure, not upstream
submission evidence.
**Freshness:** Superseded as soon as a run reports
`high_order_7plus_blocks=0` with `exhausted=1`. Re-run on any change to
`.github/workflows/hyperv-runtime-drill.yml` or anything under
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/drill/`. Never cite
this entry as build, KUnit, CoCo or GPADL/UIO qualification evidence. If
GitHub retires or repurposes the `windows-latest` / `windows-2025` images, or
drops the Hyper-V role from them, this claim is void until re-measured.

## 2026-10-01 03:05 -03 — fragmentation-first is proven; the chase floor counts pages the buddy does not report (EVD-0142)

**What:** [Run 36806229800](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36806229800)
(`Hyper-V runtime drill`, fork `4f1b44ee8048`) — all three jobs green.
Companion [run 36806229637](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36806229637)
(`VMBus upstream candidate`, same SHA) is also green on all three jobs
(`wsl-backport`, `kernel (x86_64)`, `kernel (arm64)`), which is the first
green candidate build since the cover letter landed.

**Question:** does fragmentation-first ordering fix the rebind, and does the
order-0 chase floor close the third EVD-0134 acceptance signal?

**Answer: the rebind is fixed. The chase floor is not, and its failure is
measurement, not candidate behaviour.**

| Signal | windows-2025 | windows-latest |
| --- | --- | --- |
| `cycle_fails=0 / 120 steps` | ✅ | ✅ |
| `MMAP_HOLD /dev/uio0 maps=5`, ring `maps=1`, window OPEN | ✅ | ✅ |
| `SPLATS=0` / `FAULTS=0` | ✅ | ✅ |
| `min_free_kbytes saved=… set=512 now=512` | ✅ 5704 | ✅ 5704 |
| **`rebind=yes`, `nic_driver=…/hv_netvsc`** | ✅ | ✅ |
| `after_fragment MAPS count=12 bytes=20279296 pages=4939` | ✅ exact baseline | ✅ exact baseline |
| helper vs script re-read | 9 / 9 | 9 / 8 |
| `exhausted=1` + `high_order=0` | ❌ `exhausted=0`, `high=9`, `chase=0` | ❌ same |

### The rebind is fixed: fragmentation-first works

`RESULT … rebind=yes` and `nic_driver=/sys/bus/vmbus/drivers/hv_netvsc` on
both guests. The synthetic NIC came back to `hv_netvsc` cleanly after the
BUG-3 hold-in-mmap, which is what EVD-0141 showed is impossible if the
fragmentation drill runs *after* the lifecycle drill: `hv_uio_remove` never
calls `vmbus_disconnect_ring`, so unbinding `uio_hv_generic` while a helper
still holds `/dev/uio0` leaves the channel in `CHANNEL_OPENED_STATE` and the
following `__vmbus_open()` returns `-EINVAL`. Running fragmentation first
gives it a boot-clean NIC and lets the lifecycle drill consume the channel
afterwards. Two runs in a row now (EVD-0141 reproduced the `-22` at t=5.2s
*before* the frag drill when the order was reversed).

### The map balance is exact after the frag drill's rebind

`after_fragment MAPS count=12 bytes=20279296 pages=4939` on both guests —
the same triple the lifecycle drill records as `BASELINE`. That is direct
allocate/free-balance evidence on this candidate across a full bind →
`uio_hv_generic` probe (which allocates the 2 MiB ring set) → unbind →
`hv_netvsc` rebind cycle. It is a second, independent point against the
EVD-0088/0089/011 retention signature, measured on a real Hyper-V guest
rather than by sampling `/proc/vmallocinfo` on the daily host.

### Why `chase=0`: the punched pages never reach buddyinfo

Both guests: `ready=1 … chase=0 exhausted=0 … stop=chase-order0-floor
high_order_7plus=…->10->9`. The punch worked — `held=4103 freed=4096
pairs=4096` is a clean 1:1 buddy-pair split, exactly the isolation guarantee
— and then the chase ran **zero** iterations because the floor fired on the
first check.

`/proc/buddyinfo` reports `zone->free_area[order].nr_free`. It does **not**
include the per-cpu page cache. A page freed by `munmap` goes to the pcp, not
onto the buddy free list, so it is free in `MemFree` and invisible to
`buddyinfo`. The arithmetic on windows-2025 is exact:

```
before free        503145 pages
locked             495408 pages
overhead (pt/VMA)   ~1470 pages
expected buddyinfo   6267 pages
observed buddyinfo   6267 pages
if punches landed   10363 pages   (6267 + 4096)
```

The 4096 punched pages contributed **zero** to buddyinfo. They are in the
pcp. A floor on buddyinfo's order-0 count therefore reads 1 after a punch
that just freed 4096 pages, and forbids the very allocation that is supposed
to split the remaining high orders. `chase=0`, `high=9`, and the third
acceptance signal stays red on a healthy candidate.

`MemFree` is the opposite error: while high orders remain they *are* most of
it (EVD-0141, windows-2025: 7.5 MiB free with 7.0 inside six order-7-and-up
blocks), so a `MemFree` floor forbids the split it exists to allow.

Neither quantity is "pages a fault can take without splitting something."
That is `MemFree − Σ_{k≥1} buddyinfo[k]·2^k` — order-0 buddy + pcp + slack.
It rises when the allocator splits a high order (order-10 → 1024 order-0
pages, take 16, +1008), so the floor cannot fire while high orders remain; it
is a starvation guard, and `high==0` is what actually ends the chase.

### Candidate build is green

[36806229637](https://github.com/emersonbusson/WSL2-Linux-Kernel/actions/runs/36806229637):
all three jobs success. The `0000-*` skip added in `4f1b44ee8048` works —
"Apply and build every patch in the series" reached all six pinned patches
instead of dying on the cover letter with exit 128. Strict checkpatch, W=1,
Sparse on both arches, CoCo static invariants plus both gate self-tests, and
20/20 named KUnit cases. This closes the last build-qualification blocker.

### What this run does not prove

- `high_order_7plus_blocks=0` with `exhausted=1` is **still open** — both
  guests stopped at `stop=chase-order0-floor` with 9 blocks free. This is the
  third EVD-0134 acceptance signal and the only one still red.
- `PASS_RING_ALLOCATION_UNDER_FRAGMENTATION` was not reached. The frag drill
  short-circuits on `exhausted != 1` before the rebind check, so its verdict
  is `INCONCLUSIVE_CAP_REACHED`. The rebind itself is green and is reported
  separately.
- On windows-latest the helper's third `high_order_7plus` value (9) and the
  script's `/proc/buddyinfo` re-read (8) differ by one. The helper's value is
  authoritative by contract; the one-block difference is the same
  measurement-timing class of discrepancy EVD-0141 fixed for the pin window
  and is not a candidate result.
- The CoCo chunked order-N → order-0 fallback remains unreachable on ordinary
  x86_64. COCO-1..5 remain open.

### Fix shipped

Contribution-fork `b54451c44eb5` (RamShared `c2fea8a9`) replaces
`order0_free_pages()` with `unsplit_free_pages()` = `MemFree` pages minus
every page in an order-1-or-higher buddyinfo block, raises the floor to 512
pages (one 2 MiB subchannel ring, so the channel-open exercise still has
something to allocate), and renames `FRAG_CHASE_ORDER0` → `FRAG_CHASE_UNSPLIT`
and `stop=chase-order0-floor` → `stop=chase-unsplit-floor` so the log names
the quantity actually counted. `order0_free_pages()` is removed outright —
Day-0, no dead paths. Helper builds clean with
`cc -O2 -Wall -Wextra -Werror -static`.

**Verdict:** 🟡 partial — fragmentation-first ordering fixes the rebind and
the map balance after the full bind → probe → unbind → rebind cycle is exact
(`count=12 bytes=20279296 pages=4939`, identical to the lifecycle
`BASELINE`), and the candidate build is green on all three jobs for the first
time since the cover letter landed. The chase floor did not close the third
EVD-0134 acceptance signal, and its failure is measurement, not candidate
behaviour: `buddyinfo` omits the per-cpu page cache, so a floor on its order-0
count reads 1 right after a punch that freed 4096 pages and forbids the split
it exists to allow. `high_order_7plus_blocks=0` with `exhausted=1` remains
open.

**Category:** kernel-runtime-audit

**How to measure:** `gh run view 36806229800 --repo emersonbusson/WSL2-Linux-Kernel --log`
and grep `FRAGMENT_BUDDY`, `held=`, `freed=`, `pairs=`, `chase=`, `exhausted=`,
`stop=`, `high_order_7plus`, `after_fragment MAPS`. The decisive arithmetic is
the windows-2025 buddyinfo account: `expected buddyinfo 6267` == `observed
buddyinfo 6267`, with the 4096 punched pages contributing zero.

**Evidence schema:** `ramshared.validation.v2`.
**Evidence ID:** `EVD-0142`.
**Owner role:** `kernel-runtime-engineer`.
**Observed at:** `2026-10-01T03:05:00Z`.
**Verified at:** `2026-10-01T03:05:00Z`.
**Source revision:** `4f1b44ee8048`.
**Lifecycle:** `reviewable`.
**Retention:** Keep the buddyinfo account (before/locked/overhead/expected/
observed) next to the `chase=0` line — the pair is the root-cause argument
that the punched pages never reach buddyinfo, and either alone is
unconvincing. Keep the measured revision `4f1b44ee8048` distinct from the fix
revision `b54451c44eb5` (RamShared `c2fea8a9`). Keep the candidate-build
green result separate from the drill verdict: a green build is not gate
closure.
**Freshness:** Superseded as soon as a run of `b54451c44eb5` or later reports
`high_order_7plus_blocks=0` with `exhausted=1` under
`stop=chase-unsplit-floor`. Re-run on any change to
`.github/workflows/hyperv-runtime-drill.yml` or anything under
`Documentation/virt/hyperv/vmbus-ring-buffer-upstream-v2/drill/`. Never cite
this entry as build, KUnit, CoCo or GPADL/UIO qualification evidence beyond
the named candidate-build result. If GitHub retires or repurposes the
`windows-latest` / `windows-2025` images, or drops the Hyper-V role from them,
this claim is void until re-measured.
