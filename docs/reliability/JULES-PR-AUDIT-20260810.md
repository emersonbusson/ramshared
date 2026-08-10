# Automated PR audit — 2026-08-10

This audit compares every open Jules-authored pull request and the open
Dependabot pull request with the current RamShared specifications and the
consolidation branch. Historical CI success is not treated as semantic proof.
Scratch artifacts, duplicate tests, stale patches, and changes that weaken an
identity or teardown boundary are not imported.

## Verdicts

| PR | Raw proposal | Verdict | Consolidated disposition |
| --- | --- | --- | --- |
| [#158](https://github.com/emersonbusson/ramshared/pull/158) | `toml` and `base64` updates | Valid with dependency-policy correction | Update `toml`; update `base64` with default features disabled and `std` enabled so the default `simd-unsafe` feature is not admitted for a control-plane encoder; record the decision in `docs/LIBRARIES.md`. |
| [#160](https://github.com/emersonbusson/ramshared/pull/160) | More swap-path canonicalization cases | Raw tests invalid | Do not bless malformed paths such as `/dev/dev/nbd0`; add a refusal test and tighten the exact managed-device allowlist instead. |
| [#161](https://github.com/emersonbusson/ramshared/pull/161) | `utc_ms` wall-clock test | Redundant and timing-sensitive | Retain deterministic event-transition timestamp tests; do not add a flaky wall-clock assertion. |
| [#162](https://github.com/emersonbusson/ramshared/pull/162) | Duplicate `utc_ms` test | Duplicate | Same disposition as #161. |
| [#163](https://github.com/emersonbusson/ramshared/pull/163) | Oversized JSON-line write test | Valid | Add exact `InvalidData` refusal and prove that no partial bytes are written. |
| [#164](https://github.com/emersonbusson/ramshared/pull/164) | Ghost swap filtering test | Valid but overlapping | Existing ghost tests already cover managed/live distinctions; retain them and add no duplicate fixture. |
| [#165](https://github.com/emersonbusson/ramshared/pull/165) | RAII field cleanup | Partly valid, stale | Rename only truly lifetime-only `buffers`/`char_dev` fields; retain `product_online.target`, which is now used for exact identity revalidation. |
| [#166](https://github.com/emersonbusson/ramshared/pull/166) | Non-Windows memory-sampler fallback test | Valid | Add the platform-gated `None` fallback test. |
| [#167](https://github.com/emersonbusson/ramshared/pull/167) | Test-directory cleanup helper | Invalid | Canonicalizing a non-existent deletion target makes cleanup unreliable and adds no product safety. Existing unique test fixtures remain the correct boundary. |
| [#168](https://github.com/emersonbusson/ramshared/pull/168) | Oversized JSON-line read test | Valid | Add exact `InvalidData` refusal. |
| [#169](https://github.com/emersonbusson/ramshared/pull/169) | Swapoff allocation micro-optimization | Invalid package | Exclude committed `.orig`/patch/description scratch files and the unbenchmarked lifetime churn. The current drain-based cleanup already avoids cloning the active map. |
| [#170](https://github.com/emersonbusson/ramshared/pull/170) | Unix mode on evidence files | Wrong layer/platform | Windows evidence ACL ownership remains an installer/SYSTEM boundary. A Unix-only `0600` call neither secures existing Windows files nor proves ACLs. |
| [#171](https://github.com/emersonbusson/ramshared/pull/171) | VRAM safety-net boundary tests | Valid | Add just-below-RAM and VHDX-precedence cases. |
| [#172](https://github.com/emersonbusson/ramshared/pull/172) | Empty pagefile-path refusal | Valid | Add configured/active empty-path fail-closed tests. |
| [#173](https://github.com/emersonbusson/ramshared/pull/173) | DEMOTE explanation test | Valid after cleanup | Add the meaningful non-floor-breach case; exclude PR-refresh comments. |
| [#174](https://github.com/emersonbusson/ramshared/pull/174) | Duplicate pagefile test | Duplicate | Covered by the consolidated #172 test. |
| [#175](https://github.com/emersonbusson/ramshared/pull/175) | Duplicate safety-net tests | Duplicate | Covered by the consolidated #171 cases. |
| [#176](https://github.com/emersonbusson/ramshared/pull/176) | Watchdog touch test | Already present | `touch_resets_the_clock` is the stronger existing invariant. |
| [#177](https://github.com/emersonbusson/ramshared/pull/177) | PowerShell value hardening | Valid finding, stale patch | Keep current RAW-volume/cardinality/deadline logic and bind every dynamic storage value through an explicit child environment allowlist instead of script interpolation. |
| [#178](https://github.com/emersonbusson/ramshared/pull/178) | `BrokerCoreConfig` | Valid refactor | Use one named typed config and preserve all current lease/reconciliation behavior. |
| [#179](https://github.com/emersonbusson/ramshared/pull/179) | `SparseVramConfig` | Valid with compatibility correction | Add the named safety config and retain existing convenience constructors as compatibility wrappers. |
| [#180](https://github.com/emersonbusson/ramshared/pull/180) | Percentile boundary tests | Valid | Add negative/zero and 100/over-100 clamp cases. |
| [#181](https://github.com/emersonbusson/ramshared/pull/181) | Drain active DEMOTE map | Already present | Current agent cleanup uses `active.drain()` and does not clone the map. |
| [#182](https://github.com/emersonbusson/ramshared/pull/182) | Evidence redaction | Valid security finding, invalid package | Exclude scratch scripts/plans. Token heuristics cannot prove short secrets safe, so retain stable class/code and discard free-form detail completely. |
| [#183](https://github.com/emersonbusson/ramshared/pull/183) | RAII field subset | Duplicate/stale | Covered by the valid subset of #165. |
| [#184](https://github.com/emersonbusson/ramshared/pull/184) | Remove `HostGates.target` | Invalid regression | The target is now consumed by exact current-run identity revalidation; removal would weaken BINARY_MATCH/storage binding. Exclude scratch files. |
| [#185](https://github.com/emersonbusson/ramshared/pull/185) | Avoid borrowed slice over kernel-mutated `mmap` | Critical valid finding, unsafe raw fix | Replace the borrowed slice API with a bounds-checked owned fixed-array snapshot and propagate `Result`; exclude panic-based bounds and scratch scripts. |
| [#186](https://github.com/emersonbusson/ramshared/pull/186) | Cascade command hardening | Raw patch stale/misdiagnosed | Direct `Command` argv does not invoke a shell. Remove the remaining read-only `pgrep` fallback and trust only the owned PID record plus exact `/proc/<pid>/comm`. |
| [#187](https://github.com/emersonbusson/ramshared/pull/187) | ublk teardown completion | Valid finding, partly already present | The runtime already attempts stop/join/delete before returning. Fix composite handles so ring failure cannot skip the worker join, with manufactured both-attempts tests. |

## Close protocol

These PRs remain open until the consolidation PR is merged and its required
checks pass. After merge, each superseded PR is closed with a link to the
consolidation commit/PR. This ordering preserves recoverability and avoids
claiming that rejected raw patches were merged.
