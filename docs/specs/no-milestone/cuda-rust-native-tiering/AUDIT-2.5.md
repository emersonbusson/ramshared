# AUDIT-2.5 — cuda-rust-native-tiering

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
| PR #278 | Open and conflicting after merged PR #275 changed async tensor lifetime handling. | Compare the exact surviving failure case with current `main`; do not replay its unconditional stream synchronization as a new fix without a reproducer. |
| PR #279 | Open. Its proposed `PinnedHostMapping` exposes safe host slices and `DerefMut` while the device pointer can be used asynchronously; its zero-length test constructs a zeroed `CudaContext`, which is not a valid Rust value. | Remove the invalid test fixture, specify host/GPU aliasing and in-flight unregister ownership, then test a real context and fault/teardown paths on supported hardware. |
| PR #280 | Open. The added tests cover lowering into Tile IR, not GPU result equivalence or operation-specific identity/axis boundaries. | Add op-specific negative and result tests, then qualify execution on `sm_80+` with the supported CUDA toolkit. |

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

## Forensic findings

| Severity | Boundary | Finding | Required closure |
| :--- | :--- | :--- | :--- |
| Blocker | GPU DMA lifetime | Future cancellation cannot be equated with driver completion. A timed-out or dropped operation may still own DMA buffers and a context. | Specify an ownership state machine, then test cancellation before submission, timeout during flight, delayed completion, and teardown failure. |
| Blocker | Swap data integrity | Variable-sized compressed pages change allocation, mapping, acknowledgement, and crash recovery. CRC32 alone does not establish atomicity. | Specify the raw/compressed metadata format, commit point, rollback, restart, and swapoff-first recovery; test interrupted writes and byte-exact reads. |
| Blocker | Hardware qualification | `cutile` Tile code cannot run on the local `sm_75` GPU; no `sm_80+` qualification evidence is present. | Run named kernel and fallback tests on a supported GPU and toolkit, with artifact provenance and workload-specific measurements. |
| High | Backend migration | Optional `cuda-core`/`cuda-async` entries have not been compared with the existing Driver API implementation. | Prove equivalent allocation, transfer, context affinity, error, and lifetime behavior before switching the production backend. |
| High | Host safety | A proposed 50 ms cancellation deadline cannot guarantee that `/dev/dxg` or another foreign driver call has stopped. | Bound admission and report timeouts honestly; retain resources until completion; run controlled pressure and recovery tests. |
| High | Evidence matrix | The proposed SPEC test names do not yet correspond to executable tests, and its coverage and live-E2E gates have not run for a new backend. | Add tests first, achieve the per-file 80% line-coverage gate, then execute live before/action/after and `BINARY_MATCH` on the installed surface. |

## Hard-gate disposition

- [x] PRD and SPEC distinguish current facts from proposed behavior.
- [x] `sm_75` and `sm_80+` are separate hardware gates; the existing uncompressed
  path remains the fallback.
- [x] Reserve policies are separated by product surface rather than presented
  as a universal 2 GiB rule.
- [ ] Critical cancellation, data-integrity, and recovery decisions are fully
  specified and exercised by executable named tests.
- [ ] Each new business-logic file passes the SSDV3 per-file coverage gate.
- [ ] GPU execution, pressure/recovery, and installed-binary identity are
  qualified on every claimed target surface.

## Verdict: `no-go` for production migration or host replacement

Step 3 may continue only as isolated, opt-in implementation slices with the
existing CUDA path preserved. The current source and tests do not justify
enabling compression, claiming `cutile` integration, or replacing the running
host binaries. Re-audit this file after the blockers have executable evidence.
