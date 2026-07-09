# validation.md — RamShared

> Live log of **empirical** validations for RamShared — the single source of truth for "is this actually working right now?". Covers all manual, integration, and E2E validations; taxonomy is detailed in the **Categories** table below. Anchored on **Kahneman #13** (medir, não asseverar — existence ≠ execution, green-in-last-run ≠ green-now). Each entry registers raw, measured data, not qualitative impressions.

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
| `fail-safe`   | Resiliency/demotion logic under load (eviction thresholds, automatic teardowns, watchdog timeouts)  | recovery active             |
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
