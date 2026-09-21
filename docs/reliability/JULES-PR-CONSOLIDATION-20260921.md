# Local PR consolidation audit — 2026-09-21

This is a read-only GitHub snapshot plus locally tested source consolidation.
It is not a merge approval or a claim that the full queue is qualified. No PR
was merged, closed, commented on, or pushed during this audit.

## Queue inventory

The open queue was contiguous from #1888 through #2081: **194 PRs**. GitHub's
changed-file API returned a file list for every PR. The primary-area groups
below are a routing heuristic based on changed paths, not a code-quality
verdict; mixed-surface PRs are counted once.

| Primary area | PRs |
| --- | ---: |
| Windows services and lab scripts | 44 |
| No changed files against current `main` | 33 |
| Policy crates | 24 |
| Kernel drivers | 20 |
| Daemon and transport | 19 |
| Packaging | 18 |
| Other source and tests | 17 |
| Block and origin | 14 |
| Documentation and governance | 5 |
| **Total** | **194** |

The 33 empty-diff PRs are #1894, #1903, #1906, #1909, #1932, #1937,
#1949, #1959, #1969, #1970, #1979, #1980, #1982, #1983, #1991,
#2000, #2005, #2006, #2011, #2013, #2020, #2022, #2033, #2035,
#2042, #2048, #2054, #2057, #2058, #2063, #2065, #2066, and #2076.
They offer no source delta to integrate locally. Closing or retargeting them
remains a separate maintainer action.

## Batch 1 — block/origin boundary

All 14 PRs routed to the block/origin group have an initial local disposition
below. Two supplied usable deltas; the others require no integration, redesign,
or a separate qualification batch.

| PR | Local disposition | Reason / evidence |
| --- | --- | --- |
| #2072 | Selected and hardened locally; not merged | The physical `VramMemory::len()` boundary is now checked before Live-chunk reads and writes. The PR's temporary files and unrelated `Cargo.lock` churn were excluded. `physical_bounds_refuse_provider_io` was RED before the fix and GREEN after it. |
| #2051 | Selected tests locally; not merged | Four distinct NBD handshake refusal/continuation tests were retained. The existing invalid-magic test was not replaced, and unrelated lockfile churn was excluded. |
| #2074 | Not integrated | Its timeout reaper removes an inflight range without proving the underlying I/O completed. If wired into a concurrent path, that would allow a conflicting operation to proceed while the first may still run. The current range model is not wired into the daemon worker. |
| #2044 | Not integrated | The proposed “queue full rejection” test inserts 2,048 distinct ranges and then accepts another one; it does not test a queue-depth limit. |
| #2046 | Not integrated | Most assertions duplicate existing range tests. Wrapping the model in an external `Mutex` does not establish a production concurrency contract; the zero-length insertion case also does not represent a request. |
| #1921 | Not integrated | The proposed five-second deadline is checked only *after* blocking reads. Its own slow-reader test sleeps for six seconds before returning, so it does not prove bounded handshake latency. |
| #1922 | Not integrated | Its cancellation callback is invoked after deleting the inflight range, without a completion acknowledgement. This has the same conflicting-I/O risk as #2074; adding the model to coverage configuration does not supply a runtime owner. |
| #1962 | Deferred | The proposed standalone fuzz target exercises `parse_request`, but has no corpus, bounded CI job, or recorded fuzz result. It also brings a new tool dependency; review with the continuous-fuzzing documentation batch. |
| #1974 | Not integrated | It prepends a custom magic/timestamp before the standard NBD greeting when enabled, changes the public handshake signature, and does not wire a caller. A public magic value and second-resolution timestamp are not an authentication secret; monotonic seconds also reject distinct valid clients in the same second. |
| #2003 | Not integrated | Zeroing from `Drop` discards CUDA errors, may block teardown, and is bypassed by `into_inner`; it cannot establish the advertised wipe guarantee. A separate ownership and failure contract is required. |
| #2028 | Rejected for correctness | It removes invalidation after partial origin writes and failed sync. The changed test then expects stale cached bytes instead of the partially updated origin bytes after recovery, violating the authoritative-origin contract. |
| #2034 | Not integrated | Adds an unused global request-ID allocator to a protocol that already carries client handles; `fetch_add` also wraps without a uniqueness policy. No consumer or new requirement is shown. |
| #2047 | Not integrated | Most new cases restate command decoding, and one explicitly tests a checksum error that `parse_request` does not produce. The current parser and reply tests already cover the meaningful wire boundaries. |
| #2079 | Deferred | The large `isolated_origin` file split changes coverage configuration and moves roughly 900 lines. It needs equivalence review and the full origin test/coverage matrix before being adopted; no behavior gap justifies mixing it into this safety batch. |

The block crate README and module docs now state the actual architectural
boundary: `Inflight` is a mutable range-conflict model, not lock-free runtime
tracking, request idempotence, or teardown proof. The NBD worker's own
synchronous dispatch remains the current execution boundary.

Batch 1 validation: `cargo test -p ramshared-block` (95 tests),
`cargo fmt --all -- --check`, `cargo clippy -p ramshared-block --all-targets
-- -D warnings`, and the `sparse_vram.rs` line-coverage gate (93.1%) passed.
The sparse component is reusable but is not the origin-backed product NBD
backend; no live GPU or swap qualification was run. The source change therefore
does not close a product lifecycle gate.

## Batch 2 — security documentation

All five PRs routed to the documentation/governance group were compared with
the actual source/configuration. None is safe to import literally:

| PR | Local disposition | Reason |
| --- | --- | --- |
| #1960 | Deferred | Inserts IPC authenticity, replay resistance, and monotonic counters as if they were uniform controls. The current threat model explicitly covers evidence/governance rather than product IPC; each concrete transport needs its own verified boundary. |
| #1963 | Deferred | States an unconditional WSL2 `CAP_SYS_ADMIN`/ublk control policy without a matching, qualified product transport. Standard WSL2 remains NBD; a custom-kernel ublk privilege decision belongs in its transport SPEC. |
| #2073 | Not integrated | Claims duplicate Cargo dependency versions are forbidden, but `deny.toml` sets `multiple-versions = "warn"`. It also overstates when advisory data is refreshed. |
| #2077 | Not integrated | Labels an unimplemented generic host hardening checklist mandatory for production, including MAC profiles and a dedicated non-root runtime that are not established for every supported surface. It could mislead operators into changing host-wide `sysctl` settings. |
| #2078 | Not integrated | Claims continuous fuzzing, CI corpus minimization, and `allocate_vram`/`protocol_parser` targets that are not present. PR #1962 only proposes one `parse_request` target. |

The existing threat model's scope and the live product/host authorization
boundary were preserved. A future security document must distinguish a
verified current control from a proposed hardening task.

## Batch 3 — distro packaging

All 18 packaging PRs were inspected against the current scripts and release
version. None can be imported verbatim as a qualified release pipeline.

| PR | Local disposition | Reason |
| --- | --- | --- |
| #1947 | Deferred | Input guards are useful, but it requires unused `fakeroot`, changes staging semantics, and retains an obsolete v0.12 fallback. |
| #1948 | Not integrated | Runs host-wide `udevadm trigger` in `postinst` and masks reload failures; package installation must not silently touch every device. |
| #1951 | Rejected | Recursively removes `/run/ramshared` and `/var/log/ramshared` on purge, including operator-owned logs. |
| #1952 | Not integrated | Makes `ublk`/DKMS mandatory despite standard WSL2's NBD baseline; RPM names a Debian `libudev1` dependency. |
| #1954 | Superseded | Hardcodes an old v0.9 beta tarball hash while the current package version is v0.14.1. |
| #1955 | Deferred | Adds unused `spectool`/`createrepo` prerequisites and an unqualified free-space threshold. |
| #1956 | Not integrated | Appends persistent SELinux fcontext policy on each install and ignores errors; removal and distro policy are unspecified. |
| #1958 | Not integrated | Replaces the RPM license with Apache-2.0 although workspace binaries declare MIT and packaged udev rules declare GPL-2.0-only; tarball hash validation is conditional. |
| #1961 | Not integrated | Mutates raw `.deb` ar headers after package creation using fixed offsets, without a demonstrated reproducibility contract. |
| #1966 | Deferred | Commits generated SBOMs and temporary planning files with unrelated lockfile churn; the generator is not tied to a deterministic CI verification gate. |
| #2002 | Deferred | Verifies GPG against whatever key happens to be trusted locally, not a pinned maintainer/release identity. Its generated-key test proves parser mechanics, not release provenance. |
| #2059 | Not integrated | Adds a Debian `dh` rules path unused by the current direct `dpkg-deb` build, and a test that terminates its own shell on failure. |
| #2060 | Rejected | If release binaries are missing, it creates executable empty dummy files and packages them as a successful portable release. It also defaults to v0.12. |
| #2061 | Deferred | Deriving the version from Cargo is useful, but its generated RPM changelog claims hardware DMA/ublk qualification for every build and inserts a placeholder maintainer identity. |
| #2067 | Not integrated | Reports a consolidated package-build success even though the Arch branch only copies a PKGBUILD; it defaults to v0.12 and does not verify that a package artifact was produced. |
| #2068 | Deferred | Removes ad hoc `/run/ramshared` creation, but the auto-deploy script and direct service path may run outside packaged tmpfiles setup. `packaging/tmpfiles.d/ramshared.conf` also names a user/group that must be proven to exist. |
| #2069 | Rejected | Suppresses `invalid-license` and missing-signature lint findings, and signals the current shell with TERM on lint failure. |
| #2071 | Not integrated | Adds placeholder maintainer identity, strips installed binaries without qualifying symbols, and uses shell self-termination for lint failure. It also labels an evolving package an “Initial release.” |

The current `build-rpm-package.sh` path also had a verified false-success gap:
it ignored a failed release `cargo build`, reported a spec-only result as
`(PASS)` when `rpmbuild` was absent, and accepted a successful `rpmbuild` exit
without checking for an RPM. These paths now refuse packaging, with three
regression tests in `tools/ci/build-rpm-package.test.mjs` (two RED before the
fix, all GREEN after). The script consumes prebuilt release binaries; it does
not launch a hidden release build. RPM/Arch metadata still says
GPL-2.0-only/GPL2 while the root and workspace package license is MIT and
packaged udev rules declare GPL-2.0-only. Licensing and artifact provenance
must be reconciled before any public package qualification; this audit does
not guess a legal expression or bless a release.

## Batch 4 — daemon and transport

All 19 PRs in this group have an initial source-level disposition. The ublk
teardown and IPC lease cases require a lifecycle proof across owners before
code from a PR is imported.

| PR | Local disposition | Reason |
| --- | --- | --- |
| #2075 | Deferred | Adds a CAP_SYS_ADMIN check before opening ublk control, but a capability check alone is not the complete authorization/host policy and changes missing-device refusal to permission refusal in tests. This is not the standard WSL2 NBD path. |
| #2041 | Not integrated | TCP socket keepalive may be useful but the test only connects and never inspects server socket settings or detects a dead peer. The claimed 15-second bound ignores socket option failures; it also brings an executable patch helper into the tree. |
| #2036 | Deferred | Adds a root-only, ignored ublk recreate test. It is useful platform qualification only after a controlled device namespace and cleanup evidence; it does not simulate reboot recovery. |
| #2032 | Not integrated | Spawns an extra scoped thread per worker attempt yet leaves the same panic-restart policy and no proof that partial worker state is safe to restart. |
| #2031 | Deferred | Breaks the reader after enqueueing NBD DISC, but its test uses a five-second sleeping reader instead of proving peer shutdown and worker completion under real bidirectional close. |
| #2030 | Not integrated | Halves a 200 ms polling interval to 100 ms; this is not an immediate signal wakeup and its global `SHUTDOWN` test can race other tests. |
| #2019 | No functional delta | Replaces a lexical submission-queue scope with explicit `drop(sq)`; the guard and error path are unchanged. |
| #1998 | Architecture gap; deferred | Correctly exposes that a failed `stop_device` currently skips `server.join`. Its proposed solution attempts join/delete even after stop failure and discards teardown errors on the start failure path. A live-server/device ownership state machine and bounded failure tests are required before adopting it. |
| #1987 | Not integrated after test audit | Exhaustively enumerates `backend_release_allowed`, but derives every expected value from the exact production expression. The new assertions are not an independent oracle or a RED reproducer; existing targeted cases already cover the release/refusal boundaries. No lifecycle qualification follows from duplicating the expression in a test. |
| #1986 | Deferred | Adds mock `serve_request` cases, but treats zero block size as an accepted arbitrary-alignment backend and describes Trim as a no-op without showing a product discard contract. Needs alignment with backend invariants. |
| #1985 | Not integrated | Extracts a two-line CUDA test helper only; no behavioral or evidence gap is closed. |
| #1984 | Not integrated | Replaces one test `unwrap` with `if let`/`panic` while the adjacent test still uses `unwrap`; no runtime path changes. |
| #1936 | Rejected | Retries `BrokenPipe` as transient, even though it denotes a broken peer pipe. The test mutates the prior fatal-error fixtures to accommodate the new behavior. |
| #1925 | Deferred | Replaces targeted user-data cancellation with all-requests-on-fd cancellation. This changes ownership scope, and the unbounded completion wait in its test does not prove cancellation safety or bounded teardown. |
| #1920 | Not integrated | Reformats existing origin CLI guards and adds process tests for already-covered refusal paths; it also carries unrelated lockfile churn. |
| #1915 | Not integrated | Its “crash during write” test invokes only broker lease events, not an interrupted write or socket cleanup. The existing TTL behavior is not new. |
| #1912 | Not integrated | Refactors writer chaining but drops the malformed-request error detail and includes an ephemeral PR description file. No behavior improvement is demonstrated. |
| #1900 | Rejected | Replaces the 16 MiB write-buffer bound with a nominal 4 GiB bound that a `u32` request length can never exceed. It would permit near-4-GiB allocations on untrusted requests. |
| #1897 | Rejected for lifecycle | Immediately unleases slices when the tenant is absent from a snapshot, bypassing the deliberate disconnect lease-TTL quarantine and its outstanding-I/O protection. |

## Batch 5 — policy crates

All 24 PRs routed to agent, broker, config, and tier-policy crates have an
initial source-level disposition. The parser hardening from #1971 was selected
as a narrow, tested delta; its unrelated newline rule was not adopted.

| PR | Local disposition | Reason |
| --- | --- | --- |
| #2081 | Not integrated | A Mermaid diagram in `Tier` rustdoc presents a linear ZRAM→VRAM→VHDX transition and “memory pressure” trigger without modelling the actual admission/refusal state machine. It also includes planning scratch files. |
| #2080 | Deferred | Splits the large N3 pure-state module and changes generated evidence/coverage files. An equivalence and coverage run is required; no runtime gap is identified by the split alone. |
| #2027 | No functional delta | Pure guard-clause rewrite of swap command error mapping. |
| #2026 | No functional delta | Pure guard-clause rewrite of config error span extraction; no new parser cases. |
| #2024 | No functional delta | Pure watchdog guard-clause rewrite plus an ephemeral regex edit script. |
| #2016 | Rejected | Assumes slice IDs equal array positions and changes the public `UnknownSlice` error to `IndexOutOfRange`, although lookup is by ID; it also includes a `.orig` backup file. |
| #2004 | Selected in part, tested locally | The proposed malformed-PSI recovery and scratch scripts were excluded. A new regression first proved that repeated `avg10`, `avg60`, or `total` fields were accepted; the parser now rejects those ambiguous samples, including a malformed-first duplicate. This is local source consolidation, not PR merge or live broker qualification. |
| #1999 | Not integrated | “Absolute path” helper falls back to the original command and PATH lookup if no standard-directory match exists, so it does not enforce the advertised security boundary. |
| #1997 | Not integrated | Base64-encoding a fixed PowerShell command does not authenticate or sandbox it; `-ExecutionPolicy Bypass` weakens local policy without a demonstrated need. |
| #1990 | No runtime delta | Replaces test panics with `Result` in the N3 model tests only; not a production error path. |
| #1978 | Deferred | Moves roughly 865 lines of agent command code into a module. Needs equivalence validation for reconnection, watchdog, and swap completion; no isolated behavior fix is shown. |
| #1977 | Rejected as policy | Adds an arbitrary `+10` PSI aging bonus and per-tenant metrics cardinality without calibration, lifecycle evidence, or a demonstrated fairness invariant. |
| #1976 | Deferred | Persists lease identity locally, logs checkpoint state, and changes tenant from disk on restart. An atomic file rename alone cannot establish broker authority or lease validity; restart reconciliation is missing. |
| #1975 | Deferred | Adds user-supplied tier/capacity strings to demotion text with no source-of-truth or verification that the values correspond to observed capacity. |
| #1973 | Deferred | A failure threshold changes watchdog check into a state-mutating timer reset on each missed interval. No runtime caller policy or tests for long silent broker sessions are supplied. |
| #1972 | Not integrated | Preflight checks local device metadata before `nbd-client` attaches and bypasses file-type validation in tests; it treats file mode bits as an effective permission proof. The existing activation workflow needs an ordered device-state contract. |
| #1971 | Selected in part, tested locally | Nonfinite/negative `avg10` or `avg60` is now rejected; the test was RED before the fix and GREEN after. Its trailing-newline requirement was excluded because the parser accepts complete in-memory strings without needing a procfs framing promise. |
| #1968 | Rejected | Requires power-of-two slice bytes without a cited hardware contract, reports `CapacityExceeded` for that case, and calls the free-slice fraction “fragmentation.” Metric emission is not asserted by its test. |
| #1967 | Deferred | Changes lease expiry by a fixed 15-second grace period and accepts renewal after the original deadline. This alters the protocol contract and cleanup timing without a peer/restart qualification. |
| #1965 | Deferred | A heuristic message redactor may hide device names and paths that operators need while leaving unknown secret shapes exposed; it cannot establish a general “sensitive data redaction” guarantee. |
| #1953 | Rejected as incompatible | Replaces the existing newline-delimited JSON IPC wire format with a 12-byte binary header without negotiation or a version transition. A truncated header is treated as clean EOF. |
| #1923 | Rejected for lifecycle | Frees all tenant slices on disconnect, including `Active` and `Draining`, without swapoff, I/O drain, or zeroing. |
| #1917 | Deferred | Adds stream resynchronization after an oversized IPC line. Product connection policy currently fails closed on protocol violation; continuing on the same peer needs an explicit threat-model decision. |
| #1907 | No functional delta | The guard-clause rewrite moves the N3 generation-history capacity check to the “new lease identity” branch, but the current implementation already checks capacity only after the known-lease branch returns. No renewal fix is supplied. |

Batch 5 validation for the selected PSI fixes: `cargo test --locked -p
ramshared-agent` (57 library, 16 main, 7 CLI tests), strict Clippy, formatting,
and the `psi.rs` line-coverage gate (98.2%) passed. This is parser hardening,
not a claim of live broker or Windows-driver qualification.

## Batch 6 — Linux kernel block driver

All 20 driver PRs have a source-level disposition. No kernel code was imported
without a matching failure-path SPEC, multi-kernel static/build checks, and
platform qualification. A changed errno or cleanup order is not, by itself,
evidence of safe device teardown.

| PR | Local disposition | Reason |
| --- | --- | --- |
| #2023 | Rejected | Changes load-time-only capacity/queue module parameters from `0444` to writable `0644` without reconfiguring the live disk, DMA mapping, or tag set. Sysfs values could diverge from the actual device. |
| #2018 | Deferred | Adds a version-dependent `blk_cleanup_disk` shim and changes tag-set ownership tests to `ops`; kernel API compatibility and double-free behavior need the targeted version matrix. |
| #2017 | Rejected | Removes tag-set freeing after failed `device_add_disk`, leaving the probe failure path without the already-allocated queue cleanup. |
| #2015 | Not integrated | Adds a pre-4.1 `devm_ioremap_wc` fallback, outside the documented kernel-support matrix and not checked on that kernel. |
| #2014 | Deferred | Adds per-segment checks after a pointer has already been calculated, but the outer bio bounds check exists; “zero-copy memcpy” is a misdescription. Needs adversarial multi-segment tests before any change. |
| #2010 | Not integrated | Adds IOCTL command numbers with no handler or userspace consumer; publishing an unimplemented ABI is premature. |
| #2009 | Deferred | Adds `__packed __aligned(8)` to existing ABI structs without `sizeof`/offset assertions or 32/64-bit compatibility evidence. |
| #2008 | Rejected | Handles discard/secure erase by zeroing mapped VRAM without a matching origin-durability or advertised feature contract; flush is reduced to `dma_wmb`. This could acknowledge data loss. |
| #2007 | Deferred | Moves telemetry atomics from per-segment to per-bio, but includes no counter-equivalence test or evidence for the claimed queue contention improvement. |
| #1910 | Deferred | Moves DMA mask setup into the BAR mapping function and returns generic `-EFAULT` on failure; resource ordering and version-specific fallback need a full probe unwinding test. |
| #1905 | No semantic improvement | Replaces the existing errno-to-`blk_status_t` helper with `BLK_STS_IOERR` at every shown call; the advertised semantic distinction is not added. |
| #1904 | Rejected | Changes queue-depth clamping to refusal while probe still clamps; creates inconsistent policy and brings an ephemeral patch script. |
| #1899 | No functional delta | Flattens a cleanup conditional before setting both fields to zero. |
| #1895 | No functional delta | Replaces the existing [16, 1024] conditional clamp with `clamp_val`. |
| #1893 | Rejected | Silently truncates the discovered BAR aperture at an arbitrary 1 TiB instead of validating the requested capacity against the real resource. |
| #1892 | Deferred | Splits streaming/coherent DMA masks with a 32-bit coherent fallback; this changes mapping policy and needs an actual DMA API/platform matrix. |
| #1891 | Deferred | Reorders PCI teardown, clears driver data early, and destroys a mutex. Exact queue/DMA/region ownership and kernel-version behavior need a failure-path qualification. |
| #1890 | Rejected | Converts the underlying `pci_enable_device_mem` error to generic `-ENODEV`, discarding actionable failure semantics. |
| #1889 | No selected delta | Mostly rewrites error goto layout and the existing queue clamp. It does not add a new failure test or fix an evidenced leak. |
| #1888 | No functional delta on 4 KiB pages | Adds a 4096-byte alignment check alongside the existing `PAGE_SIZE` check; for the qualified 4 KiB-page environment these are identical. Other page sizes require an explicit BAR contract. |

This is not an upstream patch review verdict. The driver remains subject to
the existing kernel-panic mitigation, exact hardware identity, and LKML
validation gates. No `trovaldo.md` qualification log was appended because no
new kernel build or live hardware qualification was run.

## Batch 7 — cross-cutting source and tests

All 17 PRs in this group have an initial disposition. The useful `.wslconfig`
escape-detection cases from #1993/#1995 were consolidated into one local
parity-based implementation with a failing-then-passing selftest.

| PR | Local disposition | Reason |
| --- | --- | --- |
| #2062 | Not integrated | Adds a docs gate that reports PASS when `namcap` is absent, so CI would advertise PKGBUILD linting without running it. |
| #2045, #2043, #1933, #1924, #1919, #1918, #1914, #1911 | No source delta | These PRs change only `Cargo.lock` against current `main`; their titles promise tests or code not present in their changed-file list. No lockfile churn was imported. |
| #2025 | No functional delta | Guard-clause rewrite of Vulkan instance/allocation cleanup and range checking; resource ownership is unchanged. |
| #2012 | No functional delta | Guard-clause rewrite of exact NBD sysfs owner checks; no new invariant or test. |
| #1995, #1993 | Selected in part, tested locally | Both identify single-backslash cases the old regex missed. The new scanner rejects odd runs before any character or at end and accepts even runs. The selftest was RED on special characters/triple slash and GREEN after the fix. No host `.wslconfig` was written. |
| #1989 | Not integrated | Rewrites two device-kind passes into one pass plus a temporary vector. The ZRAM-before-NBD teardown order remains the same; no measured benefit. |
| #1988 | Not integrated | Wraps a SHA-256 hasher as a `Write` adapter solely to call `std::io::copy`, adding an adapter without a measured or correctness gain. |
| #1964 | Deferred | Constant-time digest comparison is useful only with a specified secret/attacker timing model; the integrity table compares public block hashes and no timing threat or benchmark is supplied. |
| #1934 | Not integrated | Adds a global `/proc/self` precheck before parsing CLI arguments and a chmod-based permission test that can behave differently as root; it loses exact I/O failure context. |

Batch 7 validation: `bash scripts/safety/wslconfig-ctl.sh selftest` passed.

## Batch 8 — Windows services, lab scripts, and mixed driver surface

All 44 PRs in this routing group have an initial source-level disposition.
No Windows service, driver, VM, destructive lab, or benchmark action was run
from Linux. Pure mock tests are not a substitute for Win11/WDK lifecycle
evidence.

| PR | Local disposition | Reason |
| --- | --- | --- |
| #2070 | Not integrated | Throws `PSCustomObject` values as exceptions but supplies no consumer proving structured fields survive PowerShell exception wrapping; includes an ephemeral `finish.sh`. |
| #2064 | Deferred | Requires `wt.exe` even for launchers that do not use Terminal; optional signature validation checks only `Valid`, not an expected publisher or pinned identity. |
| #2056 | Deferred | Global physical/logical performance-counter preflight needs a plan/live-mode and actual counter-read test; object count alone does not prove usable measurements. |
| #2055 | Deferred | Enumerating named-pipe names for up to six seconds does not prove server identity, ACL, or a successful client handshake; readiness can race after the enumeration. |
| #2053 | Not integrated | Hardcodes two VS2022 BuildTools paths, excluding other supported editions/versions; assigns `$msbuild` without using it. |
| #2052 | Rejected | A top-level trap calls `Environment.Exit(74)`, bypassing normal `finally`/permit cleanup paths. |
| #2050 | Deferred | `CloseMainWindow` can be a no-op on console processes, then adds ten seconds before kill. The exact process-instance and cleanup contract needs timing tests. |
| #2049 | Rejected | Treats any VM with the requested name as a successful idempotent creation without verifying its configuration or ownership. |
| #2040 | Test-only candidate | Adds useful pure `post_boot_smoke` input combinations, but the “timeout” case is only default booleans, not an observed timeout. |
| #2039 | Test-only candidate | Exercises manifest TOML parse/refusal with synthetic artifact hashes; does not prove signatures, files, or install-time integrity. |
| #2038 | Test-only candidate | Broadens mocked runtime failure/teardown cases; needs Windows execution and equivalence review before promoting any lifecycle gate. |
| #2037 | Not integrated | “Valid” pagefile-size tests merely expect an API error or `NotWindows`, so they do not validate size calculation or successful Windows behavior. |
| #2029 | Deferred | Breaks a one-second SCM monitor sleep into 50 ms chunks, but the new test reimplements the monitor loop rather than exercising production code. |
| #2021 | No functional delta | Consolidates four registration refusal branches and adds assertions for existing refusal behavior. |
| #2001 | Not integrated | Replaces distinct exceptions with a single `StorageMatrixFailure` ErrorId throughout the script; no distinct semantic classification or caller test is shown. |
| #1996 | Deferred | Reworks Windows-only service imports/exports for Linux-side tests. Cross-target build and actual SCM behavior must both pass before changing compilation boundaries. |
| #1994 | Not integrated | Pipe tests mostly assert constants/error formatting; the security descriptor test silently skips when SID resolution fails. It does not prove authenticated peer refusal. |
| #1992, #1981 | Deferred | Large, overlapping `windows_driver.rs` unit-test sets include local parameter guards but not IOCTL/driver roundtrips. Deduplicate and run on Windows before selection. |
| #1957 | Not integrated | Checks an arbitrary 15 GiB disk threshold and connectivity to microsoft.com, not the actual media source or final artifact size. |
| #1950 | Rejected | Changes a timeout test to expect a null-Path binding error and has a taskkill mock that can report success without terminating the worker. |
| #1946 | Rejected | Ctrl+C handler invokes `Environment.Exit(0)` from a callback, bypassing normal guardian cleanup/evidence closure. |
| #1945 | Deferred | Throws when `wslservice` is absent rather than recording the host/guest failure in the existing bounded guardian probe. |
| #1944 | Not integrated | Adds an unconnected `SysInfoProvider` and generic threshold functions; no product caller or live telemetry freshness gate is wired. |
| #1943 | Deferred | Admin and Hyper-V module checks may be valid for live VM operations but are inserted before script mode selection, potentially blocking read-only/static use. |
| #1942 | Deferred | WSL binary/distro preflights are useful candidates; the disk-space formula (`3 × max tier`) is not derived from an evidence budget and does not prove guest space. |
| #1941 | Not integrated | Replaces a configurable poll interval with exponential backoff without showing the resulting readiness/timeout distribution. |
| #1940 | Rejected for benchmark parity | Replaces the fixed one-thread workload with host `ProcessorCount`, changing the benchmark workload across machines and invalidating comparisons. |
| #1939 | Test-only candidate | Exercises pure service state transitions with mocks; does not close Windows pagefile, queue, or GPU teardown gates. |
| #1938 | Not integrated | Restricts VM names to two hardcoded lab names despite supporting a caller-supplied VM; no safety proof for the restriction. |
| #1935 | Not integrated | Injects a test-only “no CUDA device” branch into the production probe and adds an error variant not produced by the real CUDA driver path. |
| #1931 | Not integrated | Adds a generic threshold helper not wired to host safety admission; its tests only restate the helper's comparison. |
| #1930 | Test-only candidate | Adds header/payload boundary cases, but most test raw `Read::read_exact` rather than the product IPC message reader. |
| #1929 | Rejected as false coverage | Builds a parser inside `#[cfg(test)]` and fuzzes that mock, not the production ring parser; it even accepts zero queue entries in the mock. |
| #1928 | Test-only candidate | Adds mock service failure paths but labels insufficient VRAM as a missing tenant dependency and device-create failure as occupied ports. Test names/evidence must match the injected fault. |
| #1927 | Not integrated | Mostly checks error formatting and manual payload reads already represented by existing IPC tests; no new production parser behavior. |
| #1926 | Test-only candidate | Adds numeric config boundaries, but the cases need deduplication against existing validator tests and Windows execution. |
| #1916 | Deferred | Converts an I/O error through `raw_os_error().unwrap_or(0)`, losing original `ErrorKind`; its test has no active lease and does not prove disconnect quarantine or peer teardown. |
| #1913 | Rejected | Requires a 4096-byte-aligned borrowed IOCTL input slice, which ordinary `Vec<u8>` inputs do not guarantee; would reject valid requests. |
| #1908 | No functional delta | Guard-clause rewrite of Windows mount path validation, without new traversal or canonicalization tests. |
| #1902 | Rejected as unsafe generalization | Exact Unix mode/owner policy is imposed on all config files, includes an environment-variable bypass and metadata/read TOCTOU, and weakens `forbid(unsafe_code)` to allow unsafe lookup. Windows ACL authority is not qualified. |
| #1901 | Rejected | Checks output directory capacity using `Test-Path` before the fresh output directory is created, so a legitimate first run fails. The 1 GiB threshold is arbitrary. |
| #1898 | No useful security delta | “Sanitizes” a literal WQL service name by embedding repetitive inline assignments/escaping in every query; there is no user-controlled query parameter at this site. |
| #1896 | Rejected for error semantics | Broadly changes `IoError` across block, daemon, and Windows surfaces, mapping a retryable network/write glitch to NBD `ENOSPC`. Disk-full is not the observed condition; client handling could change incorrectly. Includes an ephemeral rewrite script. |

The test-only candidates are not merged locally because Windows/WDK execution
and deduplication are still required. This batch makes no Windows production
claim and leaves the local worktree free of new Windows-driver mutations.

## Next review gates

1. Reconcile empty-diff PRs against their commit histories before any remote
   closure; do not manufacture source changes to keep them open.
2. Review overlapping block/origin and daemon/transport PRs together. A
   timeout may report an outstanding operation but must not erase ownership or
   authorize conflicting I/O without a completion/cancellation proof.
3. Treat kernel, DMA, auth, and Windows driver PRs as safety-sensitive:
   require the owning SPEC, refusal plus legitimate tests, and platform-correct
   qualification before local integration or merge recommendation.
4. Exclude temporary files, generated coverage snapshots, and unrelated
   lockfile churn from all later batches.
