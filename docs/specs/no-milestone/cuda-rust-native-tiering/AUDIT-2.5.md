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
