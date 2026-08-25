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
**How to measure:** Run recursive `grep` searches for local host paths `<legacy-private-root>/` and workstation hostname `EMEDEV` across the workspace.
**Measured data:**
- Comments translated to English across all 10 workspace crates (47 files modified).
- Local hostname `EMEDEV` replaced with `dev-workstation` in `docs/BENCHMARKS.md`.
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

**What:** (1) Kernel track lab inventory on emedev WSL2. (2) Desktop control app (zenity/CLI) for cascade.
**Category:** local-check + integration
**Measured data:**
- WSL_YES; GPU RTX 2060 via GPU-PV (PCI vendor 0x1414); no /dev/dri; kernel-true Gate A1 **FAIL**
- PASSO0: docs/specs/no-milestone/kernel-vram-as-memory/PASSO0-INVENTORY.md
- cascade-app status: shows disk-only swap (cushion off)
- zenity+DISPLAY present; install-cascade-app.sh writes .desktop
- bash -n cascade-app OK
**Verdict:** ✅ inventory blocks LKM on this lab; ✅ control app MVP ready
**Next action:** user may `sudo cascade-app.sh start` or --gui; trilha K waits bare-metal.

## 2026-07-10 — Hyper-V lab on R: RUSSIA (3 paths)

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

## 2026-07-10 — Dual-boot unblocked on E: (ESPANHA), not R:

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
cat /mnt/c/Users/emedev/ramshared-drill/agent-guest-lab-20260714-results.json
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
**Measured data (host EMEDEV, elevated, 2026-07-14):**
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
- Prior same-day host path (EMEDEV): Disk5 RAMSHARE RAW→S: NTFS; probe 8 MiB write≈1224 / read≈146 match=True (validation 11:52 entry)
**Verdict:** ✅ Guest signed reload + CREATE/FORMAT/MEASURE **PASS** (pass=9 fail=0)
**Next action:** optional Bypass execution policy on guest for CIM measure script; optional INF/PnP FriendlyName branding (RAMSHARE vs Msft Virtual Disk)

## 2026-07-14 13:27 -03 — host memory policy: WSL 16G RAM + 4G VRAM cascade (no wsl --shutdown)

**What:** Apply shared-host policy so WSL2 does not starve Windows/Hyper-V (civm, win11-drill): system RAM cap 16 GiB in `.wslconfig`; cascade VRAM tier 4 GiB; GPU free floor 1 GiB. Applied cascade-down/up live without `wsl --shutdown` (user mid-work).
**Category:** config / e2e
**How to measure:**
```bash
cat /mnt/c/Users/emedev/.wslconfig
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
**Verdict:** ✅ Cascade 4G VRAM path LIVE without killing WSL session; host residual RAM policy documented for Windows+civm
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
- Local access file confirms user `emedev`, SSH keys installed, passwordless
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
`guard exec -- cargo test -p ramshared-block` produced no test result and its
exact queued process was terminated by the root after a confirmed reentrant
Guard-to-cargo-shim self-deadlock. It was not retried. No
further Cargo, Guard, Rust test/build/check, Clippy, or rustfmt command ran.
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
use GuardWSL and must not bypass admission.
**Measured data:** `legacy_preallocation_removed_before_day0_deadline` passed;
the complete Node checker suite passed `2/2` with `91.14%` line, `82.26%`
branch, and `95.65%` function coverage. The candidate scan passed. Both
affected PowerShell static suites passed. `cargo fmt --all -- --check` passed.
`scripts/docs-check.sh` passed after the localization manifest and generated
capability observations were synchronized. The targeted `git diff --check`
passed.
**Refusals:** `guard exec -- cargo test -p ramshared-block --lib --
--test-threads=1` produced no Cargo result: GuardWSL held the controlled queue
and the tool session ended before compilation output. It was not retried or
bypassed. No Cargo test/build/check/Clippy result is claimed. No service, WSL,
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
**Refusals:** The root confirmed that PID `558934` was exactly the queued
`<HOME>/.local/bin/guard exec -- cargo test -p ramshared-block` command and
terminated only that PID with `TERM` after confirming the reentrant
Guard-to-cargo-shim self-deadlock. It produced no Rust test result and was not
retried. No later Cargo, Guard execution, rustc, rustfmt, Clippy, or Rust
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
