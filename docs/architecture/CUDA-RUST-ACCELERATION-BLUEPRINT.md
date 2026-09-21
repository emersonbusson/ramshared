# CUDA-Rust Acceleration Blueprint: Current State and Qualification Gates

## Status

RamShared currently stores uncompressed pages in GPU memory through its own runtime-loaded CUDA Driver API wrapper in `crates/ramshared-cuda`. This is a working path, not a future prototype. `cuda-core` and `cuda-async` are optional entries in that crate's manifest, but no production source uses them. `cutile-rs` and `cuda-oxide` are not RamShared dependencies or installed RamShared backends. This blueprint describes possible work and must not be cited as a shipped feature or performance result.

## Hardware and upstream boundary

| Surface | Local RTX 2060 (`sm_75`) | Separate `sm_80+` GPU |
| :--- | :--- | :--- |
| Existing RamShared CUDA Driver API | Working baseline; qualify each host binary | Working design; verify on target host |
| `cuda-core` / `cuda-async` | Optional manifest dependencies only; runtime integration unimplemented | Same |
| `cuda-oxide` SIMT kernels | Candidate requiring toolchain, artifact, and live tests | Candidate, not integrated |
| `cutile-rs` Tile kernels | Unsupported by upstream Tile IR | Candidate requiring supported CUDA toolkit and GPU tests |

The local host has an RTX 2060 (`sm_75`) and no `nvcc`. It cannot run `cutile` Tile kernels. Upstream Tile microbenchmarks do not establish swap throughput, compression ratio, or latency for RamShared. `cutile-rs` PRs [#278](https://github.com/NVlabs/cutile-rs/pull/278), [#279](https://github.com/NVlabs/cutile-rs/pull/279), and [#280](https://github.com/NVlabs/cutile-rs/pull/280) remain under review and are not dependencies or evidence of adopted functionality.

### Upstream PR audit snapshot (2026-09-21)

| PR | Current disposition for RamShared | Required upstream evidence before reconsideration |
| :--- | :--- | :--- |
| [#278](https://github.com/NVlabs/cutile-rs/pull/278) | Conflicts with current `main`; its stream synchronization is already present after merged [#275](https://github.com/NVlabs/cutile-rs/pull/275). Do not integrate a duplicate fix. | Rebase and retain only a regression case if it adds coverage; reproduce the original #252 failure on supported GPU/Compute Sanitizer. |
| [#279](https://github.com/NVlabs/cutile-rs/pull/279) | Unsafe to depend on as-is: an unowned raw host pointer is exposed through safe mutable slices and unconditional `Send`/`Sync`, while GPU access may still be in flight; drop can unregister without a completion proof. | Own or borrow the backing allocation, enforce exclusive CPU/GPU access and completion ordering, handle context-binding failure during teardown, and replace the zeroed-context test with valid mocks plus live GPU tests. |
| [#280](https://github.com/NVlabs/cutile-rs/pull/280) | Compiler-only tests assert the generic word `reduce`; they do not prove the distinct bitwise operations or runtime results. The proposed reduction output shape also needs verification against the API's dimension semantics. | Assert op-specific IR and integer-type constraints; test identities, axes, shapes, and numerical XOR/AND/OR results on supported GPU. |

These are source-review findings, not upstream maintainer verdicts. Pure `cutile-ir` tests can run locally, but they do not validate Tile compilation or execution on this `sm_75` host.

## Safety model

The current wrapper already ties device allocations to a CUDA context using Rust lifetimes and RAII. Replacing it with another wrapper requires a demonstrated improvement and must preserve context affinity, error handling, and allocation lifetime. A Rust Future can stop waiting or prevent queued work from starting; dropping it does not guarantee that an in-flight CUDA operation, `/dev/dxg` ioctl, or GPU kernel has stopped. Host and device buffers must remain owned until completion or a qualified teardown path is observed.

GPU compression is only a hypothesis. Four-kilobyte pages incur transfer, launch, metadata, decompression, and recovery costs. Compression ratio varies with workload; already-compressed or random pages may expand. A crash-consistent raw/compressed block map, bounded allocation, checksum, and uncompressed fallback are prerequisites. No universal `2 GiB` reserve applies: the broker/NBD reserve is `max(1536 MiB, 20%)` plus a separate 768 MiB runtime-free buffer; origin cache uses `max(2 GiB, 20%)`; StorPort uses `max(configuration, 512 MiB, 10%)`.

## Staged qualification

1. **Baseline**: Record hardware, transport, driver, kernel, active swap, binary identity, throughput, p50/p95/p99 latency, pressure/stalls, and integrity for the existing uncompressed CUDA path.
2. **Optional runtime prototype**: Exercise `cuda-core` and `cuda-async` behind a reversible feature gate. Test refusal without CUDA, context affinity, failed allocations, queued cancellation, timeout while DMA is in flight, delayed completion, and buffer lifetime. Compare with the baseline before replacing any production path.
3. **Kernel prototype**: Build reproducible `cuda-oxide` artifacts for `sm_75` or Tile kernels for `sm_80+` only. Prove round-trip and raw fallback on named compressible, incompressible, and adversarial workloads. Measure end-to-end benefit, not GPU memory bandwidth alone.
4. **Integration and recovery**: Gate broker selection by device/toolkit capability. Test crash/restart, GPU reset, memory pressure, and swapoff-first recovery. Retain the old backend and exact installed artifact for rollback. Change host installation only after tests and a safe swap transition.

The detailed proposed requirements and incomplete tests are tracked in [PRD.md](../specs/no-milestone/cuda-rust-native-tiering/PRD.md), [SPEC.md](../specs/no-milestone/cuda-rust-native-tiering/SPEC.md), and [IMPL.md](../specs/no-milestone/cuda-rust-native-tiering/IMPL.md).
