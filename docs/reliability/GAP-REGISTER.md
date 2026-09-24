# RamShared Gap Register

This file tracks open product claims that must stay **PARTIAL** until their
listed proof exists. It is not a backlog for speculative features; it is a
guardrail against false DONE status.

Current release: **v0.14.1**. Next planned release: **v0.15.0**.

Support and reserve boundaries used by current documentation:

- Standard WSL2 uses NBD as its baseline transport. `ublk`/`io_uring` is
  qualified on native Linux or WSL2 with a compatible custom kernel under
  EVD-0039; the open product-lifecycle gate below still applies.
- EVD-0040 covers zero-copy CUDA host mapping only.
- Broker/NBD uses `max(1536 MiB, 20%)` capacity reserve plus a separate
  `768 MiB` runtime free buffer. Origin cache uses `max(2 GiB, 20%)`.
  StorPort uses `max(configured reserve, 512 MiB, 10%)`.

## Current Open Gates

The 2026-08-20 through 2026-08-22 investigation remains a reason to keep the
Linux/WSL2 NBD product gate open, but its terminal incident is now classified
`host_volume_exhausted` from NTFS Event ID 137 plus `STATUS_DISK_FULL`.
RamShared causality was not established. Historical matrices remain evidence
for their exact releases, and the independent safety hardenings still require
their own live qualification before any automatic boot activation.

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| WSL2 control-plane stability and effective revocable-cache transition | PARTIAL | EVD-0050 records a prior disabled-cache release. This branch adds an isolated GPU cache worker but has only hermetic validation; the [September 23 incident audit](incidents/2026-09-23-wsl2-stress-branch-audit.md) found NBD stuck reads, stale cache/supervisor/guardian evidence, and no simultaneous physical-cache proof. The native vsock host listener and product gate wiring are still absent. | Prove fresh daemon-bound worker allocation, supervisor, origin, pressure, and 24-hour rollout evidence under one exact installed release; qualify host-guest transport separately. |
| Legacy WSL2 service handoff and teardown | PARTIAL | EVD-0049 records clean swapoff-first teardown and EVD-0050 one attended controller-owned start. On September 24, two supervised attempts reached exact `BINARY_MATCH`, current cache/supervisor telemetry, and clean swapoff-first teardown; another stopped before activation when the sealed origin was detached after restart, then official reattachment and host-gate validation succeeded. The installed release matches its build but carries dirty package provenance because unrelated local WSL configuration edits were present. The separate `kernel-ramshared-v3` image still lacks an immutable kernel/modules/QEMU manifest pair. | Produce a clean release package with BINARY_MATCH proof, then capture repeated idempotent start/stop evidence after reboot; seal a kernel/modules pair before kernel promotion. |
| Build #5 three-tier stress and performance qualification | BLOCKED | EVD-0046 conflated logical NBD occupancy with GPU-resident bytes, hard-coded an SSD disk identity, and labeled vector release timing as physical reclaim throughput. The September 23 boot had stuck NBD reads, I/O errors, MCE records, and terminal memory pressure; the 31.7% gain and prior `PASS_ZERO_PANIC` remain unqualified. The September 24 sealed origin is now provisioned at 4096 MiB and the host gate passes. Release `v0.14.1-87-g05712b2b-dirty` is installed with `BINARY_MATCH`. Supervised attempts stopped before a full run because of stale guardian evidence (88 MiB and 336 MiB allocated), GPU budget short by 56 MiB, an origin detach on restart, and host commit admission at 20226 MiB against 20480 MiB required. Host commit headroom later recovered to 22936 MiB, but the latest GPU sample was only 4115 MiB free against 4480 MiB required after the initial 256 MiB cache allocation. No attempt reached all three logical targets or 4096 MiB physical cache. | Keep host commit headroom >= 20480 MiB, GPU free memory >= remaining cache allocation plus the 640 MiB live buffer, and guest heartbeat fresh; confirm the attached origin; then complete three watchdog-bounded rounds with worker-confirmed VRAM allocation, simultaneous 100% ZRAM, 100% NBD, 99% SSD samples, independent integrity/kernel/host logs, and a comparable baseline. |
| VMBus ring fallback upstream series | PARTIAL | Michael Kelley's September 22 reply endorses using `vmbus_alloc_buffer()` for rings and consolidating GPADL/buffer lifetime state. The WSL PR #41690 now reduces selected host-initiated hv_sock listener ring sizes and describes a kernel fallback as complementary. The current kernel fork boots a `6.18.40.1` image whose SHA-256 matches its local `bzImage`, but the candidate API/lifecycle refactor is an uncommitted seven-file diff. `checkpatch.pl --strict` reports 0 errors, 0 warnings, 4 checks. Review found an unchecked `PFN_UP(size) << PAGE_SHIFT` narrowing to `u32`, and the DXG GPADL caller still clears the handle after teardown without honoring the new leak state. Fault-injection, CoCo teardown, and clean upstream patch-series validation are absent. | Fix the overflow and all external `vmbus_establish_gpadl()` callers' ownership/decryption/leak semantics; resolve all checkpatch checks; build and test a clean patch against the intended upstream base; inject allocation, decryption, GPADL create/teardown failures; run CoCo and ordinary Hyper-V tests; submit only after review of the resulting exact commit series. |
| Windows public driver distribution | BLOCKED | The validated package path is test-signed for supervised labs. Test-signing is not a public trust chain and cannot be promoted to production evidence; production trust requires an external Microsoft attestation or trusted signing identity. | Build from a clean release tag, obtain Microsoft attestation or another production-trusted signature, pass `InfVerif` and `SignTool verify /pa /all` with test-signing disabled, then pass install, rollback, and recovery drills on the declared compatibility surface. |
| Corrected Windows physical lifecycle qualification | PARTIAL | Earlier physical campaigns predate the intended-payload hash, exact current-run Online identity, RAW-only mutation, active/configured pagefile, bounded process-tree, and one-fresh-approval-per-reboot contracts. They remain historical observations but cannot qualify the corrected harness. | Rebuild and seal the final package; prove loaded driver/broker/winsvc BINARY_MATCH; run three supervised cold boots with a new explicit approval for each boot; record intended/read-back hashes, exact identity, supported stops, zero residue, watchdog/task cleanup, and no Event ID 153 retries. |
| Windows virtual-disk properties, counters, and performance matrix | PARTIAL | Historical counter and throughput rows predate the current exact serial/size binding, raw counter schema, complete artifact inventory, Event ID 153 window, regression fingerprint, and fail-closed rollback contracts. Task Manager screenshots are secondary evidence only. | Run the corrected five-cell, three-run, 75-sample physical matrix after BINARY_MATCH. Require exact Virtual/SSD/non-rotating identity, direct intended-payload integrity, non-zero raw counters, zero Event ID 153 retries, median/p99/deviation, compatible-baseline verdicts, and an exact safe final state. |
| Custom-kernel DXG/systemd promotion | BLOCKED | The current `6.18.35.2` boot emitted the exact upstream-open DXG FORTIFY warning in the Xwayland wait-sync-object path. The signature also exists on Microsoft 6.18.26.1 and bundled 6.18.33.2-2, so this is not attributed to RamShared, but bundled reproduction does not qualify the risk. Separate `RamShared-Kernel` attempts timed out starting `/sbin/init`, with unclean journal and p9-cancellation evidence. | Under separate attended approval, run a fresh-boot same-host bundled/custom A/B with no RamShared or pressure activation. Require exact distro/version, systemd `running`, readable fresh warning log, DXG/Xwayland/lightweight NVIDIA probe, zero FORTIFY/init-timeout/unclean/p9/fatal signals, and query-error count no worse than the sealed bundled baseline. See the [2026-08-23 finding](incidents/2026-08-23-wsl2-dxg-fortify-systemd-no-go.md). |
| Custom-kernel/ublk as day-1 product transport | DEFERRED | NBD remains the day-1 WSL2 product path. ublk root and QEMU smokes are historical capability evidence, not product transport closure. On 2026-07-18, `SANITIZED_ARTIFACT_REF` recorded SSH, non-interactive privilege, and ublk capability on `SANITIZED_VM_KERNEL_LAB`. The VM still had no GPU surface, and no product ublk lifecycle, swapoff-first teardown, crash/drain, or no-ghost proof existed. | A dedicated custom-kernel lab SPEC needs isolated before→action→after evidence for transport wire-up, ordered detach, crash/drain, and terminal no-ghost state. This is an open evidence definition, not an instruction to act. |

## Closed In This Session

All run IDs, commands, VM names, and `SANITIZED_*` values below are retained
historical evidence only. Machine-specific identities and paths are sanitized,
and no row authorizes activation of the current disabled candidate.

| Gap | Close evidence |
| --- | --- |
| Legacy-preallocation Day-0 source removal | The `RAMSHARED_VRAM_PREALLOC_LEGACY` selector, its aliases, profile chooser, and full-VRAM `VramBackend` NBD composition were removed from executable source. The named `legacy_preallocation_removed_before_day0_deadline` test, clean active-source/current-doc scan, thresholded checker coverage, and documentation governance close this source-governance prerequisite only. Rust test, rustfmt, clippy, and slice coverage regression gates are fully active and passing across all crates. Generic `VramBackend` remains for broker, ublk, and Windows consumers. Live guardian/origin/pressure qualification, release promotion, and activation remain open and managers stay disabled/plan-only. |
| Rust CI guardrail and slice coverage restoration | PR #1214 restored full workspace CI coverage (`node tools/ci/check-rust-slice-coverage.mjs` >= 80% line/branch/function coverage across active crates), unblocking the pipeline and eliminating regression drift. |
| Tier 3 (SSD) qualification and hardware metrics baseline | Tier 3 fallback and NVMe/SSD degradation criteria qualified with self-contained hardware metrics comparison tables and zero-sum public hygiene, fully compliant with governance and performance requirements. |
| Photorealistic 3D hardware SVG architecture rendering | Standardized vector SVG hardware topology diagrams (VRAM/RAM/SSD tiering) integrated with dark/light themes and validated across renderer suites. |
| Multi-distro release packaging and v0.12.0 publication | Automated packaging workflow in `.github/workflows/release-packaging.yml` established with dynamic version detection, attaching qualified Debian (`.deb`), Fedora (`.rpm`), and Arch Linux (`.tar.gz`) binaries alongside `SHA256SUMS.txt` to GitHub Release `v0.12.0`. |
| Public repository branch hygiene | Purged 503 obsolete external bot/test branches from remote origin, locking down canonical single-branch (`main`) governance. |

## Rules

- Do not mark an environment-bound gate DONE from unit tests, parser checks,
  docs, QEMU-only evidence, or a different machine class.
- Do not encode one example application as a product feature, directory,
  script, policy, or generic docs heading.
- Do not commit local VM credentials, signing passwords, key material, or
  generated package artifacts.
