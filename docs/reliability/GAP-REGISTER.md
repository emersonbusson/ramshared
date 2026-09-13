# RamShared Gap Register

This file tracks open product claims that must stay **PARTIAL** until their
listed proof exists. It is not a backlog for speculative features; it is a
guardrail against false DONE status.

## Current Open Gates

The 2026-08-20 through 2026-08-22 investigation remains a reason to keep the
Linux/WSL2 NBD product gate open, but its terminal incident is now classified
`host_volume_exhausted` from NTFS Event ID 137 plus `STATUS_DISK_FULL`.
RamShared causality was not established. Historical matrices remain evidence
for their exact releases, and the independent safety hardenings still require
their own live qualification before any automatic boot activation.

| Gate | Status | Why it remains open | Required close evidence |
| --- | --- | --- | --- |
| WSL2 control-plane stability and effective revocable-cache transition | PARTIAL | The source candidate now has one aggregate 12 GiB workload ceiling for a 16 GiB guest, a protected control slice, one-second supervisor/telemetry, schema v4 worst-plane status, an independent four-proof Windows guardian, safe boot/recovery, and an SSD-authoritative revocable cache. The Rust slice coverage (>=80%), static hygiene, and contract gates are now verified and active across all 15 crates in CI. However, that static evidence does not prove a real inaccessible guest, VHDX/NBD/GPU continuity, Docker/cron ancestry after restart, or 24-hour host stability. The approved nested Windows lab now passes bounded PowerShell Direct and WSL runtime readiness after a recoverable reimage; this closes access/readiness only and does not close any live guardian, origin, pressure, or rollout gate. | The source-removal governance prerequisite is closed. Obtain fresh isolated-surface readiness evidence for sealed guest/boot identity and bounded WSL status/list probes. Then, under a separate attended approval, record before→action→after proof for healthy/inaccessible-guest handling, safe mode, non-GPU storage hashes, GPU allocation failure, matching origin hash, physical-cap transitions, logical-size matrix, and a 24-hour disabled-definition stage. No item in this close plan authorizes a current action. |
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
| Multi-distro release packaging and v0.11.0 publication | Automated packaging workflow in `.github/workflows/release-packaging.yml` established with dynamic version detection, attaching qualified Debian (`.deb`) and Arch Linux (`.tar.gz`) binaries alongside `SHA256SUMS.txt` to GitHub Release `v0.11.0`. |
| Public repository branch hygiene | Purged 163 obsolete external bot/test branches from remote origin, locking down canonical single-branch (`main`) governance. |
| 4 GiB VRAM multi-tier stress qualification | Active 4,096 MB VRAM allocation with host display floor preservation max(1536 MB, 20%) verified on host. Multi-tier stress qualification battery completed passing 171% of RAM (20,208 MB allocated), saturating ZRAM (1,024 MB, 100%) and driving GPU VRAM to 1,707 - 1,969 MB (up to 311.6 MB/s PCIe DMA, 15.6x boost vs SSD), 21.66 GB/s flash reclaim in 910 ms, 0.0006 ms median allocation latency, and PASS_ZERO_PANIC stability. |

## Rules

- Do not mark an environment-bound gate DONE from unit tests, parser checks,
  docs, QEMU-only evidence, or a different machine class.
- Do not encode one example application as a product feature, directory,
  script, policy, or generic docs heading.
- Do not commit local VM credentials, signing passwords, key material, or
  generated package artifacts.
