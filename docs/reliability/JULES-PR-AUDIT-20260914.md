# Jules PR consolidation audit — 2026-09-14

## Scope

This audit covers pull requests #1726 through #1860. Numbers #1736 and #1858
do not identify pull requests. The resulting cohort contains 133 proposals:
132 were open against the v0.12.0-era base and #1859 was already merged.

Every proposal was reviewed against v0.13.4 rather than merged wholesale.
The review considered the actual diff, compatibility with the current source,
repository hygiene, the governing SPEC where applicable, and a proportionate
local validation path. Old branches were not used as an authority for current
behaviour.

## Result

| Disposition | Count | Meaning |
| --- | ---: | --- |
| Consolidated | 42 | Reimplemented or retained on the current branch with focused validation. |
| Already merged | 1 | Present on `main` before this consolidation. |
| Superseded | 5 | The current source already contains the necessary hardening; duplicating an old patch would add no behaviour. |
| Successor required | 34 | The problem can be worth solving, but the submitted patch is not a safe implementation of it on the current architecture. |
| Rejected | 51 | Incorrect, redundant, unconnected to production behaviour, or unsuitable for public history. |
| **Total** | **133** | Complete cohort. |

### Consolidated

| Source PRs | Current commit | Retained result |
| --- | --- | --- |
| #1760, #1761, #1762, #1763, #1767, #1768, #1808, #1809, #1812, #1814, #1815, #1818, #1830, #1831, #1832, #1833, #1835, #1836, #1837, #1838, #1839, #1840, #1841, #1842, #1845, #1846, #1847, #1848, #1849 | `789dba77` | Focused Rust coverage for configuration, tier, IPC, package, evidence, runtime, and service behaviour. Duplicate and non-asserting cases were removed. |
| #1738 | `9f2b6a44` | Correct constrained-state demotion transition. |
| #1810, #1816, #1817, #1819 | `9e35f855` | Remove stale error paths and unnecessary copies without changing the public workflow. |
| #1793 | `b0c1acd5` | Derive package timestamps from the source revision and normalize staged package mtimes. |
| #1799 | `8c78abe4` | Classify two independent one-bit mismatches as corrupted memory. |
| #1850 | `d5770984` | Validate broker slice layouts in sorted order instead of pairwise scanning. |
| #1752 | `8b6f9bb8` | Treat Windows `ERROR_NO_DATA` as an orderly named-pipe disconnect. |
| #1734, #1742 | `00abd559`, `83a6b505`, `7eb5bb2f` | Replace the narrow source fragments with an SSDV3-reviewed PCI BAR/capacity contract: early refusal, defence in depth at the map boundary, checked `size_t` conversion, and preserved PCI enable errno. The target-kernel/device gate remains partial. |
| #1753, #1804 | `139fc363`, `bd665627`, `9e039749`, `5a40bed9` | Replace immediate disconnect reuse and unwired expiry helpers with a shared, renewable monotonic lease lifecycle across WSL and Windows broker shells. Pure, core, loopback, and Windows cross-target checks pass; isolated agent lifecycle drills remain partial. |

### Already merged

| Source PR | Result |
| --- | --- |
| #1859 | Merged before this audit as the v0.13 Tier 3 qualification and stability work. It was not duplicated. |

### Reassessment of the deferred cohort

“Deferred” was a triage label, not a finding that an idea was invalid. This
reassessment separates work that current source has already absorbed from work
that deserves a new, owned design. It does not copy a stale patch merely to
increase the consolidation count.

#### Superseded by the current source

| Source PRs | Current evidence | Disposition |
| --- | --- | --- |
| #1726, #1729, #1731, #1739, #1744 | The current driver already has queue-depth bounds, checked capacity arithmetic, I/O range checks, `PAGE_SIZE` BAR alignment validation, and 64-bit/32-bit DMA-mask fallback. | No duplicate patch. The current implementation is the source of truth. |

#### Consolidated through a successor SPEC

| Source PRs | Current evidence | Disposition |
| --- | --- | --- |
| #1734, #1742 | `docs/specs/no-milestone/kernel-pci-bar-capacity-contract/` and `7eb5bb2f` establish an exact mapping invariant and preserve PCI errors without the original patch's unchecked shape. | Consolidated locally; target-tree and device evidence remain **PARTIAL**, not a finished hardware claim. |
| #1753, #1804 | `docs/specs/no-milestone/broker-lease-lifecycle/` plus `139fc363`, `bd665627`, `9e039749`, and `5a40bed9` establish holder-only heartbeat renewal, monotonic expiry, tick-owned reclamation, and bounded Windows named-pipe scheduling. | Consolidated locally; isolated broker/agent lifecycle evidence remains **PARTIAL**, not a host or device claim. |

#### Successor required — protocol, lifecycle, and allocator proposals

| PR | Why the supplied patch is not safe to merge | What a successor must prove |
| --- | --- | --- |
| #1747 | Changes broker framing without a negotiated, end-to-end compatible wire transition. | One protocol SPEC covering every producer/consumer and downgrade/refusal behaviour. |
| #1748 | Treats `BrokenPipe` as retryable although the peer is disconnected. | A lifecycle SPEC with a terminal disconnect state and bounded retry only for transient errors. |
| #1750 | Rejects adapters using arbitrary fields rather than a capability contract. | A versioned capability model with real adapter fixtures. |
| #1751 | Adds an arbitrary 4 GiB dispatcher limit without production wiring. | An allocation-policy SPEC with telemetry and all callers wired. |
| #1755 | Starts timeout accounting after a blocking read, so it cannot bound that read. | A cancellable I/O design with a test that proves the deadline. |
| #1757 | Adds a hardware-facing representation without a production path or device proof. | A device-format SPEC, target build, and hardware compatibility matrix. |
| #1758 | Replaces state with timestamps but does not connect it to production or a clock policy. | A wired expiry model with deterministic time tests. |
| #1764 | Changes a verdict API from `bool` to `Result` without callers or recovery semantics. | A compatibility plan and callers that consume the diagnostic result. |
| #1766 | Introduces an unconnected state machine. | A lifecycle owner, transition matrix, and integration tests. |
| #1769 | Adds an unconnected handshake whose timeout cannot interrupt a blocking read. | A cancellable handshake and wire-compatibility SPEC. |
| #1771 | Adds a ring reader not used by the live protocol. | A complete producer/consumer integration with malformed-frame tests. |
| #1772 | Replaces the production CUDA surface with a test mock. | An injected test seam that preserves the production hardware implementation. |
| #1776 | Has the same production-mock substitution problem as #1772. | An isolated fixture architecture and target-hardware validation. |
| #1801 | Adds an incompatible 16-byte prefix to the NBD wire format. | A versioned NBD protocol design with interoperable clients. |
| #1803 | Implements a token bucket that no serving path actually uses. | Back-pressure wired into the real serving loop with measurable limits. |
| #1805 | Adds arbitrary starvation policy that conflicts with the existing never-zero contract. | A measured fairness policy and compatibility decision. |
| #1806 | Broadly rewrites protocol schema and accepts unknown data fail-open. | A schema migration that fails closed and exercises all consumers. |
| #1811 | Adds explanatory fields with no authoritative producer. | A defined producer, schema ownership, and output fixtures. |
| #1821 | Treats metadata existence as swap readiness and accepts `/dev/null` fixtures. | Exact block-device identity and live-state validation. |
| #1825 | Performs fallible cleanup in `Drop` while discarding errors. | An explicit lifecycle API with observable cleanup failure handling. |
| #1826 | Adds validation functions unused by any boundary. | Boundary integration and accepted/refused tests. |
| #1827 | Requires power-of-two slices, incompatible with the established aligned capacity model. | A compatibility/migration decision with real allocator callers. |
| #1829 | Adds unconnected persistence with non-unique temporary files and no durability boundary. | An atomic, owned persistence design with recovery tests. |

#### Successor required — Windows, benchmark, and security documentation

| PR | Why the supplied patch is not safe to merge | What a successor must prove |
| --- | --- | --- |
| #1774 | Derives benchmark concurrency from host CPU count, breaking run comparability. | An explicit workload contract and Windows evidence across comparable cells. |
| #1778 | Adds backoff without a reset-on-progress rule. | A bounded readiness state machine and deterministic PowerShell tests. |
| #1779 | Makes Admin/Hyper-V checks global rather than operation-specific. | A Windows privilege matrix with non-admin read-only cases. |
| #1780 | Starts a watcher only when WSL already exists, which can prevent startup. | Startup and recovery tests on a Windows lab. |
| #1781 | Alters console-cancellation ownership without deterministic process tests. | A cancellation/lifetime design with Windows child-process evidence. |
| #1787 | Invokes WSL/fio/NBD tooling from configuration assertions and uses stale platform assumptions. | A bounded lab controller with read-only/configured test seams. |
| #1822 | Removes `#![forbid(unsafe_code)]` to read Windows memory status. | A minimal, reviewed unsafe boundary plus Windows static/runtime validation. |
| #1823 | Accepts zero watchdog thresholds without a defined semantic. | A threshold contract with zero, boundary, and timeout tests. |
| #1834 | Whitelists only one executable while nearby direct process launches remain outside it. | One complete command-execution boundary and Windows proof. |
| #1791, #1798 | State security controls that are not consistently implemented or mapped to owners. | A threat-model SPEC that links each public claim to a concrete control and test. |

### Rejected

| Area | Source PRs | Reason |
| --- | --- | --- |
| No demonstrable behavioural gain | #1727, #1728, #1730, #1732, #1740, #1743, #1746, #1756, #1813, #1828, #1844, #1851 | Pure reshaping or extraction added maintenance surface without a correction, measurement, or missing test obligation. |
| Incorrect or unsafe behaviour | #1733, #1735, #1737, #1745, #1749, #1773, #1775, #1777, #1800, #1802, #1807, #1820, #1824, #1843, #1860 | Examples include a fixed VM name replacing a parameter, incompatible trait changes, misleading host checks, an unbounded or externally dependent path, deletion of public-hygiene history, and tests that do not assert the stated condition. |
| Packaging | #1782, #1783, #1784, #1785, #1786, #1788, #1789, #1790, #1792, #1794, #1795, #1796, #1797 | The proposals either add unavailable tooling, weaken verification, introduce unpinned dependencies, contradict packaging policy, or contain no effective change. #1793 is the sole accepted packaging change. |
| Hygiene residue or finding-only material | #1741, #1754, #1759, #1765, #1770, #1852, #1853, #1854, #1855, #1856, #1857 | Raw patch helpers, generated output, PR text, or finding-only reports do not belong in the product history. Reliability findings remain subject to their own evidence lifecycle. |

## Public-documentation correction

The English and Portuguese READMEs had retained a v0.12.0 release badge and
status statement after v0.13.4 was published. This branch updates both to the
current published release and keeps the Tier 3 `PASS_ZERO_PANIC` statement as a
v0.13 qualification record. No internal audit name is introduced into either
public README.

## Verification record

Completed local checks while consolidating:

- `cargo test -p ramshared-config -p ramshared-tier -p ramshared-winsvc`
- `cargo test -p ramshared-agent -p ramshared-cli -p ramshared-tier -p ramshared-winsvc`
- `cargo test -p ramshared-integrity`
- `cargo test -p ramshared-broker`
- `cargo test -p ramshared-winbroker`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace -- --test-threads=1`
- `cargo deny check`
- `node tools/ci/check-ci-contract.mjs --check-local`
- `node --test --test-reporter=dot tools/ci/check-ci-contract.test.mjs`
- `bash -n scripts/package/build-deb-package.sh`
- `git diff --check`

The broker-lease successor additionally passed its RED→GREEN unit, pure-core,
and loopback tests plus the 80% Rust slice coverage gate: `lease.rs` 97.9%
(237/242), `winbroker/lib.rs` 92.4% (376/407), and `broker_srv.rs` 87.8%
(1357/1546). The Windows broker also passed its
`x86_64-pc-windows-gnu` cross-target compilation. Isolated broker/agent
lifecycle drills remain environment-bound.

The kernel PCI BAR successor also has a local RED→GREEN source-contract test:
`node --test tools/ci/kernel-probe-contract.test.mjs` failed as intended before
the implementation and then passed 2/2. `./scripts/docs-check.sh` passed after
the final generated documentation artifacts were synchronized. The
target-tree checkpatch/sparse/kselftest and isolated PCI-device drill remain
environment-bound; this audit does not promote them into a hardware claim.
