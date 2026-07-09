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
**How to measure:** Run recursive `grep` searches for local host paths `/home/emdev/` and workstation hostname `EMEDEV` across the workspace.
**Measured data:**
- Comments translated to English across all 10 workspace crates (47 files modified).
- Local hostname `EMEDEV` replaced with `dev-workstation` in `docs/BENCHMARKS.md`.
- File paths `file:///home/emdev/` in specs rewritten to relative directories (`../../`).
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

**RAW:** `/home/emdev/fase0/CASCADE-DEMOTE-20260709-163527.txt`

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

RAW: `C:\Users\emedev\ramshared-drill\agent-item8-pagefile-kpd.log`, artifacts-item8/
