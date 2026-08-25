---
slug: wsl2-revocable-vram-origin
title: Revocable VRAM cache with authoritative SSD origin
milestone: —
issues: []
---

# PRD — Revocable VRAM cache with authoritative SSD origin

## 1. Summary

Replace product-wide VRAM preallocation with a 1–24 GiB logical block device
whose authoritative copy is an independent SSD origin and whose VRAM is a
clean, revocable 128 MiB cache. GPU pressure or measurement failure removes
cache capacity without returning block I/O errors or requiring whole-tier
swapoff.

## Current boundary — disabled staging only

This PRD specifies source and static evidence only. It authorizes no origin
provisioning, VHDX/device/GPU action, live NBD/swap transition, WSL operation,
VM lifecycle, or pressure run. Any interface text below is a candidate contract,
not a command to execute. Historical evidence stays `PARTIAL` until a separately
approved attended qualification supplies before→action→after proof.

## 2. Technical context

- **Confirmed in codebase:** `SparseVramBackend` allocates VRAM before a first
  write and returns EIO when budget/allocation fails.
- **Confirmed in codebase:** `VramBackend` has no origin; its flush is a no-op.
- **Confirmed in codebase:** product NBD requires an authoritative origin; the
  former full 4/2/1 GiB allocation composition and its selector are absent.
- **Confirmed in host plan:** the existing WSL swap VHDX remains a separate
  4 GiB last-resort tier and `SANITIZED_EXISTING_WSL_SWAP_DEVICE` must not be
  reused.
- **Inference:** write-through removes the late-allocation correctness cliff,
  provided origin identity and durability are verified before NBD activation.

## 3. Recommended option

Add one generic block backend in `ramshared-block`: origin writes complete
first; a page-valid VRAM cache is updated or invalidated afterward. Reads use
only valid cache pages, otherwise read origin and opportunistically promote.
All VRAM pages are clean and may be discarded immediately after three
restricted GPU samples. The old full-VRAM NBD composition is absent and is not
a product or rollback path.

### Day-0 legacy-preallocation source closure

The full-VRAM NBD composition and `RAMSHARED_VRAM_PREALLOC_LEGACY` selector
were removed from executable source. Closure evidence includes a clean scan
showing no selector aliases, profile chooser, or NBD `VramBackend` composition;
the named sunset test; thresholded checker coverage; and documentation
governance recorded in `validation.md`. This closes the source-governance
prerequisite. Focused Rust tests, rustfmt, and Clippy for this exact worktree
remain pending on the external Guard repair; host qualification, release
promotion, and activation approval remain blocked separately.

If a later change fails the named test or introduces a product-path regression,
keep the candidate disabled, reject promotion, and restore only the exact
reviewed origin-capable source snapshot needed to recover the build. Do not
restore legacy preallocation; correct the origin-cache path and rerun the
evidence before any requalification.

## 4. Functional requirements

- **RF-1:** Logical capacity defaults to 4 GiB and accepts 1–24 GiB.
- **RF-2:** Chunk size is sealed at 128 MiB; cache validity is tracked at the
  4 KiB block level so partial writes never expose zero/uninitialized bytes.
- **RF-3:** Complete every positional write on the origin before acknowledging
  it. Ordinary writes may join a dirty batch; NBD `FLUSH` and write `FUA` are
  the durability frontiers and must complete `sync_data` before success. A
  zero-progress, partial-write, FLUSH, or FUA failure returns origin I/O failure
  and invalidates cache state that could outlive the failed durability epoch.
- **RF-4:** A cache allocation/write/read failure invalidates affected cache
  state and continues from origin without EIO when origin succeeds.
- **RF-5:** Read from cache only when validity and generation match; otherwise
  read origin and promote only within the current physical target.
- **RF-6:** Advertise and enforce NBD FLUSH/FUA semantics. Unknown command flags
  fail before backend mutation; FLUSH and FUA complete on origin before success.
- **RF-7:** Physical target is the minimum of logical capacity, the sealed
  physical cache cap, and
  `WDDM budget - external usage - max(2GiB,20% total VRAM)`. The rollout cap
  starts at 1 GiB and may be configured only up to logical capacity. Missing
  GPU/WDDM measurement produces target zero.
- **RF-8:** Grow at most one chunk per two seconds after three healthy samples;
  after three restricted samples, release clean chunks immediately to target.
- **RF-9:** Emit logical capacity, cached VRAM, GPU headroom, origin written
  bytes, cache fallbacks/invalidations, and WSL fallback swap use separately.
  A broker IO reply is observable only after its byte and IO counters are
  published, so receipt is a completion barrier for the associated telemetry.
- **RF-10:** Bind origin to a sealed host manifest and the opened file
  descriptor: configuration SHA-256, PARTUUID, parent PTUUID, partition and
  parent `dev_t`, expected size, and swap UUID must all match. Dynamically
  discover and reject the current root, active swaps, their parent devices,
  and the WSL distro/swap VHDX paths; never trust a drive letter, device path,
  size, or caller override alone.
- **RF-11:** Keep VHDX creation/partitioning and initial `mkswap` in an explicit
  attended provisioning action. Normal startup only revalidates the sealed
  identity and existing swap signature; it never formats an origin.
- **RF-12:** Keep authoritative SSD/NBD I/O independent from synchronous GPU
  calls. Cache requests use bounded waits or nonblocking delivery; timeout,
  worker disconnect, injected stall, or cache failure immediately falls back
  to origin without blocking its request loop. Cache disable/release uses a
  dedicated bounded control lane that remains deliverable after local
  `UNAVAILABLE` and while the data queue is full; zero/success is returned only
  after the worker acknowledges release (or the cache was already disabled).
- **RF-13:** Lock only mappings that already exist after GPU initialization.
  `MCL_FUTURE` is forbidden before any later CUDA/DXG/cache mapping.
- **RF-14:** Trusted, non-daemonizing daemon helpers use an invocation-private
  process group, finite output storage, non-reaping Linux exit observation, and
  bounded final direct-child reap. The zombie leader pins PID/PGID until any
  residual exact group and descendant-held pipe are contained. Stable nonzero
  fatal containment follows an unprovable reap, and RAII custody survives a
  spawn-callback panic. A helper that deliberately changes session/group or
  daemonizes is outside this trusted-helper contract.
- **RF-15:** Accept unauthenticated TCP listeners only on loopback, RFC1918
  IPv4, IPv6 ULA, or the exact Tailscale CGNAT `100.64.0.0/10`; reject every
  global, documentation, multicast, link-local, and unspecified address.
- **RF-16:** Never unlink an existing Unix socket at startup. Bind through a
  no-follow validated parent, capture and pin the exact new socket identity,
  and remove it only if the same socket remains. Terminal state is checked at
  the top of every worker iteration, independent of queue fullness or wake
  delivery, and broker panic/error propagates only after bounded worker cleanup.
- **RF-17:** Read each origin manifest from one no-follow regular FD with an
  exact `max+1` ceiling; refuse symlink, hardlink/nonregular input, concurrent
  append, path replacement, or opened/named identity/type/size drift.

## 5. Non-functional requirements

- **NFR-1:** No acknowledged write may exist only in VRAM.
- **NFR-2:** Cache revoke is valid while Linux swap uses the block device
  because origin remains authoritative; origin detach still requires the full
  swapoff-first lifecycle.
- **NFR-3:** Origin I/O errors remain NBD EIO and force `AT_RISK/BLOCKED`; GPU
  cache errors do not.
- **NFR-4:** No raw device is created, formatted, partitioned, attached, or
  mounted by source verification.
- **NFR-5:** The legacy full-VRAM NBD selector/composition is absent from
  executable source and guarded by
  `legacy_preallocation_removed_before_day0_deadline`. Activation remains
  blocked on separate live gates, and only disabled staging plans may be
  produced.
- **NFR-6:** Tests may signal only exact fixture process groups they create;
  no process-name lookup or broad PID search is a cleanup mechanism.
- **NFR-7:** Shutdown and cache release never depend on a spare data-queue slot,
  a polling timeout, or draining work admitted before terminal state.

## 6. Flows

Write: validate flags/bounds → complete origin positional write → record dirty
epoch → enqueue cache update without waiting → for FUA sync origin → success.
FLUSH: sync the current dirty epoch → success. Read: issue a bounded cache probe,
or immediately read origin on timeout/disconnect/miss → optionally enqueue a
rate-limited promotion. Restriction: record sample → on third sample compute
target → discard least-recent clean chunks. Origin error fails the request;
cache failure only reduces performance.

Shutdown: publish terminal state → wake the worker if possible → at the next
iteration boundary discard pending work and stop → join the worker → join and
propagate the broker outcome. Cache release uses the independent control lane
and waits only for its bounded acknowledgement.

## 7. Data / state model

Each cache chunk owns optional provider memory, a generation, block-validity
bitmap, and last access. Backend counters distinguish origin bytes,
cache bytes, fallback reads, invalidations, promotion refusals, and releases.
Origin state is `OFF`, `READY`, `DEGRADED`, or `FAILED`; cache state is `OFF`,
`ACTIVE`, `RESTRICTED`, `UNAVAILABLE`, or `STUCK`. An origin error is sticky
until three successful origin read+sync probes; cache is `STUCK` when it remains
more than one chunk above target for over two seconds.
The origin manifest contains a sanitized origin role, fixed size, PARTUUID,
parent PTUUID, partition and parent `dev_t`, expected swap UUID, logical
capacity, physical cache cap, chunk size, GPU reserve policy, and SHA-256 of the
sealed configuration. The daemon reads only a bounded, singly linked regular FD
and revalidates opened and named identity/type/size before accepting the bytes,
then revalidates the origin FD and current root/swap exclusions before use.
Public evidence never reproduces a host path, disk GUID, device node, or volume
identity.

## 8. Interfaces

- Daemon grammar: `--origin-manifest <sealed-guest-manifest>`; no direct origin
  path override exists in the product startup path.
- Sealed configuration binds PARTUUID, PTUUID, `dev_t`, swap UUID, logical
  capacity, physical cache cap, chunk, and headroom.
- Status/monitor schema v4 cache/origin fields.
- Windows guardian configuration references the separate VHDX and PARTUUID.

## 9. Dependencies and risks

The origin adds SSD latency and write amplification; batching changes the
performance shape but makes no performance claim until registered benchmarks
exist. A bad identity could overwrite unrelated storage, so provisioning stays
separate, attended, and exact-approval-only. A GPU driver call can hang forever;
therefore it may not share the authoritative origin request path.

Rollback trigger: any checksum mismatch after cache release, acknowledged
write missing from origin, cache error returned as EIO while origin succeeds,
origin identity accepting `SANITIZED_EXISTING_WSL_SWAP_DEVICE`, or physical cache exceeding target by
more than one 128 MiB chunk for over two seconds.

## 10. Implementation strategy

Implement fake-origin/fake-VRAM tests first; then generic backend, daemon
composition, receipt/status fields, configuration validation, and static
Windows planning. Do not provision the real VHDX in this source slice.

## 11. Documents to update

`README.md`, `ARCHITECTURE.md`, `docs/FAQ.md`, degradation/gap registers,
incident docs, `validation.md`, and the current autotier/incident SPECs.

## 12. Out of scope

Real VHDX creation/partition/format/attachment, live swap activation, 24 GiB
matrix, artificial GPU pressure, and benchmark claims.

## 13. Acceptance criteria

Named origin/cache, timeout, durability, strong-identity,
provisioning-separation, and removal tests pass; changed Rust files meet 80%
line coverage; daemon command/receipt tests bind manifest to FD identity; source
verification does not open a real block device; live E2E remains partial.

## 14. Validation plan

Unit: ordering, fallback, generation/validity, rate limiting, target formula,
origin identity/refusal. Integration: tempfile origin write→cache release→read
with identical SHA-256. Static removal: named checker plus clean candidate scan.
Static Windows: separate path, fixed size, exact PARTUUID, no existing-swap
path. Live VM matrix is env-bound.
