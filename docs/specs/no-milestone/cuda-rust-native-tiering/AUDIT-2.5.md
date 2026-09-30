# AUDIT-2.5 — cuda-rust-native-tiering

> SSDV3 Step 2.5 · PRD: [PRD.md](PRD.md) · SPEC: [SPEC.md](SPEC.md)
> **Pass 3 re-audit — 2026-09-30.** Reviews the package as revised by passes 1–2 (same day).
> No code was implemented and no hardware was qualified by any pass.

## Direction audit — is this the best thing to do

Substantively unchanged, and still the governing judgment:

- **Scope correction is right.** Compression confined to the disposable VRAM cache; a
  compressed swap/origin format stays rejected. Keep it.
- **GPU compute remains unproven**, which is why the CPU-codec control arm exists. Pass 3
  finds the arm still has no defined measurement surface (Finding 2), so the decision it is
  meant to inform still cannot be made.
- **Priority unchanged.** WSL2 freeze work, kernel promotion refusals, and the reserve-floor
  contract drift all outrank a default-off capacity optimization.

**Recommendation unchanged.** Build ITEM-1/ITEM-2, record the CPU arm through a harness that
can actually run, and only then decide whether a GPU codec belongs in this cache.

## Findings — pass 3

| Sev | SPEC § | Issue | Required fix |
| :--- | :--- | :--- | :--- |
| **Hard no-go** | DT-4; PRD NFR-2; Assumed-ready dependencies | **Admission formula does not match the parent reserve contract.** DT-4 and NFR-2 compute `fresh_admissible_headroom = available_bytes.saturating_sub(reserve_floor_bytes)`, where `reserve_floor_bytes` is only the *configured* floor (env default 512 MiB, clamp 128–4096). The parent worker enforces `max(configured, ceil(capacity/5)) + runtime_headroom` (the 640 MiB runtime buffer) via `GpuBudgetSnapshot::required_free_bytes`, and this SPEC's assumed-ready dependencies claim to reuse exactly that. On a 6 GiB adapter with the configured 512 MiB floor the parent refuses below 1,869 MiB free, while DT-4 admits codec scratch down to 512 MiB — roughly 1.3 GiB of shared-host overcommit. Greedy overcommit on shared hardware without the real floor calculation is a Step 2.5 hard no-go. | Compute `fresh_admissible_headroom` from the parent's full free floor (`required_free_bytes(configured_reserve_bytes, runtime_headroom_bytes)`), not from the configured floor alone. Carry the same formula into NFR-2. Add a named test that codec admission refuses when only the configured floor is satisfied but the 20%/runtime floor is not. *(Fixed this turn.)* |
| High | NFR-6b; DT-10; Provider codec contract | **The CPU-codec control arm has no measurement surface.** `GpuCacheCodec` compresses into provider-owned `VramMemory` slabs and workspace (`slab: &mut M`, `workspace: &mut M`). A host-side codec cannot implement that trait, yet NFR-6b/DT-10/ITEM-2 exit/ITEM-5 all require the arm. As specified it cannot be produced. | State that the control arm is measured by a **host-side harness outside `GpuCacheCodec`**, over identical extents and the same metric envelope and integrity checks; it is never a trait implementation and never a production fallback. *(Fixed this turn.)* |
| High | Required tests matrix vs Files | Matrix names `isolated_origin.rs :: compressed_cache_fault_falls_back_to_origin`, but `isolated_origin.rs` appears only under assumed-ready dependencies — it is in neither CREATE nor MODIFY. A matrix row pointing at a file no slice will touch is not executable evidence. | Add `crates/ramshared-block/src/isolated_origin.rs` to MODIFY with that required test. *(Fixed this turn.)* |
| Medium | PRD §14 Validation plan — Performance | The performance protocol still describes only three raw/compressed paired runs. It does not require the CPU-codec control arm, the raw/compressed/origin hit-p95 triple, or identical logical read ranges, so NFR-5b, NFR-6b, and DT-10 are not exercised by the plan that is supposed to close them. | Extend the Performance bullet with the control arm, the hit-p95 triple, and the identical-logical-range requirement. *(Fixed this turn.)* |
| Medium | DT-10; NFR-5b | The self-latency comparison basis is not pinned. The raw path serves 2 MiB chunks and the compressed path serves 64 KiB extents; comparing p95 across differently shaped reads is not apples-to-apples. | Require the compressed-hit, raw-hit, and origin-read p95 figures to be measured over **identical logical read ranges**. *(Fixed this turn.)* |

## Open questions — pass 3

- Is **1.25×** the right prototype bound once p95 is measured over identical logical ranges?
  A raw hit is a VRAM copy and a compressed hit adds at least a checksum and a decode step;
  if raw-hit p95 is in the sub-millisecond range the bound may be unreachable — which would
  itself be the answer (the GPU codec does not belong here). Tighten or reject with numbers;
  never loosen to pass.
- Which exact nvCOMP release exposes a documented pre-decode checksum usable through a
  dynamically loaded C API on the WSL CUDA driver with sm75? Open since pass 1; hard entry
  gate for the GPU codec.
- Does the CPU-codec arm alone reach the ≥10% net capacity gain with no GPU contention?
- At what extent size does batched GPU encode/decode stop paying for its own overhead under
  the 50 ms read deadline?
- Can the isolated-worker supervisor confirm old-process exit and prevent a replacement
  worker from racing unresolved GPU work?
- Will the parent reserve-floor contract be reconciled to one authoritative default before
  any compression admission is qualified? Note that even after this pass's formula fix, the
  *value* of the configured floor remains unreconciled (512 MiB env vs `max(1536 MiB, 20%)`
  in the parent PRD mitigation). This SPEC now uses the correct *shape* of the floor.

## Verdict — pass 3

**`no-go`** on the package as reviewed in this pass: one hard-gate failure — the codec
admission formula overcommits shared host memory relative to the parent reserve contract.

That fix and the four alignment fixes above were applied in the same turn. After those edits
the verdict returns to a **conditional `go`** for isolated, opt-in **ITEM-1 and ITEM-2
only**, with entry gates still closed:

1. **ITEM-3 (GPU codec) is gated** on the CPU-codec control arm recorded at ITEM-2 exit
   through the host-side harness, and on resolution of the nvCOMP pre-decode checksum
   question. If the CPU arm alone meets the usefulness gate without GPU contention, sunset
   the feature (DT-9) instead of building the GPU codec.
2. **Promotion beyond experimental** requires the completed matrix, the 1.25× self-latency
   bound over identical logical ranges, the co-load gate, three paired runs including the
   CPU arm, and reconciliation of the parent reserve-floor *value*.
3. **Production enablement, host activation, a 2:1 claim, or universal GPU support** remain
   no-go.

---

## Pass 2 record (2026-09-30)

| Sev | Issue (pass 2) | Disposition |
| :--- | :--- | :--- |
| Hard no-go | Self-latency gate said "within the declared bound" but never declared a number. | Fixed in pass 2: compressed-hit p95 ≤ 1.25× raw-hit p95, raw/compressed/origin triple published. Range-comparison basis tightened in pass 3. |
| Hard no-go | Codec sub-deadline (DT-3) had no named test. | Fixed in pass 2: `codec_subdeadline_falls_through_to_raw_or_miss` added (matrix 36 rows). |
| High | DT-11 contradicted "Out now", PRD flow step 5, and the security checklist on who revokes the cache. | Fixed in pass 2: DT-11 restated as worker-side codec-fault isolation; transport/protocol/process revocation unchanged. |
| High | Implementation order circular: ITEM-3 gated on an arm only ITEM-5 produced. | Fixed in pass 2: arm moved to ITEM-2 exit. Measurement surface defect found in pass 3. |
| High | DT-3 overclaimed preemption of a blocked driver call. | Fixed in pass 2: claim limited to admission and inter-step continuation. |
| Medium | Traceability mapped codec fault isolation to ITEM-3; "Out now" read as forbidding the CPU arm. | Fixed in pass 2. |

Pass 2 verdict: `no-go` → conditional `go` for ITEM-1/ITEM-2 after same-turn fixes.

## Pass 1 record (2026-09-30)

| Sev | Issue (pass 1) | Disposition |
| :--- | :--- | :--- |
| Hard no-go | Required tests matrix missing 4 rows named in Files. | Fixed in pass 1; verified complete in passes 2–3. |
| High | No codec sub-deadline; one slow codec step could revoke the entire cache. | Fixed in pass 1 (DT-3, DT-11); reopened and settled in pass 2. |
| High | No compressed-hit self-latency gate. | Added in pass 1; bound declared in pass 2; comparison basis pinned in pass 3. |
| High | Missing counterfactual: CPU codec never measured, so the GPU-compute choice was unfalsifiable. | Fixed in pass 1 (NFR-6b, DT-10); surface defined in pass 3. |
| Medium | Day-0: failed usefulness gate left the codec disabled in the tree. | Fixed in pass 1 (DT-9 sunset rule). |
| Medium | Admission inherits the unreconciled parent reserve floor. | Formula shape corrected in pass 3; floor *value* still open in the parent SPEC. |
| Medium | IMPL.md advertised superseded swap-page compression. | Rewritten in pass 1. |
| Medium | DT-7 requires an unverified nvCOMP pre-decode checksum. | Open question + ITEM-3 entry gate since pass 1. |
| Low | 64 KiB extent ceiling unreconciled with vendor batched-chunk guidance. | Recorded; measure before changing. |

Pass 1 verdict: `no-go` → conditional `go` for ITEM-1/ITEM-2 after same-turn fixes.

---

## Historical audit record (2026-09-21)

The following CUDA/cutile findings are preserved as historical research. They are not
current requirements for this cache-compression SPEC unless repeated in the findings above.

## Audit scope and evidence

This is the September 2026 re-audit of [PRD.md](PRD.md) and [SPEC.md](SPEC.md)
against the current source tree. The earlier document-only `go` verdict was
invalidated: named tests and a proposed backend were described as if they
already existed. This audit does not qualify a GPU kernel, package, or host
installation.

Facts verified in the repository:

- `crates/ramshared-cuda` has a working dynamically loaded CUDA Driver API
  path and RAII wrappers; `cuda-core` and `cuda-async` are optional manifest
  dependencies but have no production call sites.
- Neither `cutile` nor `cuda-oxide` is a RamShared dependency. No compressed
  swap representation or GPU compression kernel exists.
- The local RTX 2060 is `sm_75`; the Tile path requires `sm_80+`. The local
  host lacks `nvcc`, so it cannot qualify a Tile build or execution.
- Existing CUDA unit tests pass. The live GPU integration tests are ignored by
  the default test command; their pass status cannot be inferred from it.

## Upstream candidate audit (2026-09-21)

The current `NVlabs/cutile-rs` README explicitly sets `sm_80` as its minimum
and marks `sm_70`/`sm_75` unsupported. Native Tile support for the local
RTX 2060 is therefore not a small compatibility change to propose upstream;
an `sm_75` experiment must use a separate SIMT path and remain independent of
any `sm_80+` Tile qualification.

| Candidate | Current observation | Required before adoption |
| :--- | :--- | :--- |
| PR #278 | Open and conflicting after merged PR #275 changed async tensor lifetime handling. Current `main` synchronizes before exposing the host vector and deliberately retains its uninitialized buffer if synchronization fails; the older PR does not cover that failure path. | Compare the exact surviving failure case with current `main`; do not replay its synchronization block or overwrite the stronger error handling without a reproducer. |
| PR #279 | Open. Its proposed `PinnedHostMapping` exposes safe host slices, `DerefMut`, and `Send`/`Sync` while the device pointer can be used asynchronously; its zero-length test constructs a zeroed `CudaContext`, which is not a valid Rust value. The proposed `Drop` records bind/unregister errors but has no demonstrated in-flight completion proof. | Remove the invalid test fixture; specify host/GPU aliasing, registration ownership, and in-flight unregister behavior before exposing a safe API. Then test a real context and fault/teardown paths on supported hardware. |
| PR #280 | Open. The added tests only search IR text for `reduce`, so they do not distinguish XOR, AND, and OR or prove GPU results. Their `[8,16]` input reduced along axis 1 should have shape `[8]`, yet the fixtures declare output `[1,1]`. The pre-existing reduction lowering also removes `dim` without a bounds check, so invalid axes can panic instead of producing a JIT error. | Correct the fixture's result shape, assert op-specific identity/body/type and axis refusal, and compare device output to CPU bitwise reductions (including zero/all-ones and signed cases) on `sm_80+` with the supported toolkit. |

These are source-level audit findings, not claims that the PRs have been
updated, reviewed, or merged. The local `sm_75` host cannot close the Tile
execution gate.

## Cutile host-validation gate (2026-09-21)

No cutile PR may be opened or updated on the strength of source review or
host-only IR tests. Validate the exact candidate revision on the intended
host first, including compiler tests and GPU-result tests on supported
hardware, before proposing it upstream.

The local `feat/tile-bitwise-reductions` revision `9a463dd` was checked on
the WSL2 host. `nvidia-smi` reported one GeForce RTX 2060 (`sm_75`, driver
616.92). `nvcc` was absent, and the CUDA 13.x toolkit was not found in the
default locations checked by `cuda-bindings`. `cargo fmt --all -- --check`,
`cargo test --locked --package cutile-ir`, and strict `cargo clippy --locked
--package cutile-ir --all-targets -- -D warnings` passed. These IR tests do
not exercise the branch's compiler or GPU-result behavior. `cargo test
--locked --package cutile-compiler --lib` failed during the `cuda-bindings`
build script, before any compiler test ran, because it could not locate a
CUDA 13.0+ toolkit. No CUDA Tile kernel was compiled or executed.

Disposition: **not ready for a cutile PR**. Installing a toolkit alone would
not make this `sm_75` GPU satisfy upstream's `sm_80+` Tile requirement. A
supported GPU and toolkit are needed for the Tile candidate; any separately
designed `sm_75` SIMT implementation would need its own host qualification.
The open PRs remain untouched while this gate is red; source-only findings
are local review notes, not upstream acceptance or execution evidence.

## Forensic findings (historical)

| Severity | Boundary | Finding | Required closure |
| :--- | :--- | :--- | :--- |
| Blocker | GPU DMA lifetime | Future cancellation cannot be equated with driver completion. A timed-out or dropped operation may still own DMA buffers and a context. | Specify an ownership state machine, then test cancellation before submission, timeout during flight, delayed completion, and teardown failure. |
| Blocker | Swap data integrity | Variable-sized compressed pages change allocation, mapping, acknowledgement, and crash recovery. CRC32 alone does not establish atomicity. | Specify the raw/compressed metadata format, commit point, rollback, restart, and swapoff-first recovery; test interrupted writes and byte-exact reads. |
| Blocker | Hardware qualification | `cutile` Tile code cannot run on the local `sm_75` GPU; no `sm_80+` qualification evidence is present. | Run named kernel and fallback tests on a supported GPU and toolkit, with artifact provenance and workload-specific measurements. |
| High | Backend migration | Optional `cuda-core`/`cuda-async` entries have not been compared with the existing Driver API implementation. | Prove equivalent allocation, transfer, context affinity, error, and lifetime behavior before switching the production backend. |
| High | Host safety | A proposed 50 ms cancellation deadline cannot guarantee that `/dev/dxg` or another foreign driver call has stopped. | Bound admission and report timeouts honestly; retain resources until completion; run controlled pressure and recovery tests. |
| High | Evidence matrix | The proposed SPEC test names do not yet correspond to executable tests, and its coverage and live-E2E gates have not run for a new backend. | Add tests first, achieve the per-file 80% line-coverage gate, then execute live before/action/after and `BINARY_MATCH` on the installed surface. |

## Historical verdict: `no-go` for production migration or host replacement

Step 3 may continue only as isolated, opt-in implementation slices with the
existing CUDA path preserved. The current source and tests do not justify
enabling compression, claiming `cutile` integration, or replacing the running
host binaries. Re-audit this file after the blockers have executable evidence.
