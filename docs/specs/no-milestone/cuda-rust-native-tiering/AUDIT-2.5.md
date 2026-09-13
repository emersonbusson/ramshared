# AUDIT-2.5 — cuda-rust-native-tiering

## Forensic Scope Review
- **Target Surface**: `crates/ramshared-cuda`, `crates/ramshared-vram`, userspace async CUDA acceleration.
- **Specification Under Audit**: [SPEC.md](SPEC.md)
- **PRD**: [PRD.md](PRD.md)
- **Methodology Reference**: `docs/SSDV3-PROMPTS.md` (Step 2.5) + Kahneman #13, #15, #17

---

## Findings

| Sev | SPEC § | Issue | Required Fix |
| :--- | :--- | :--- | :--- |
| **LOW** | §1 (Scope) | Toolchain requirements for `cuda-oxide` vs `cuda-core`: `cuda-core` compiles on stable Rust 1.89+ while `cuda-oxide` SIMT codegen requires pinned nightly (`nightly-2026-08-28`). | Fixed in SPEC DT-3: compile static PTX artifacts ahead of time for `sm_75`, allowing stable Rust 1.89+ to load the PTX without nightly toolchain requirement at runtime. |
| **LOW** | §3 (DT-4) | Variable-sized slab fragmentation under intense random swap writes. | Fixed: implement 4KB fixed-slot quantized bins (e.g. 1KB, 2KB, 4KB buckets) to eliminate memory fragmentation. |

---

## Hard NO-GO Checklist Audit

- [x] **Missing Kahneman on critical**: Present (Kahneman #13, #15, #17 mapped with executable cargo commands).
- [x] **Day-0 violation**: Zero dirty workarounds; uses official NVIDIA crates (`cuda-core` v0.3.1, `cuda-async` v0.3.1).
- [x] **Incomplete test matrix**: Full matrix with named tests (`test_cuda_core_context_lifecycle`, `test_async_dma_cancellation_token`, `test_in_gpu_page_compression_roundtrip`, `test_gpu_compute_capability_dispatch`).
- [x] **Privilege / uAPI / driver boundary**: Operates in userspace with standard device access.
- [x] **Foreign process / API shapes**: Pure RamShared conventions; zero foreign narrative leaks.
- [x] **Platform gate mismatch**: Correctly gates `sm_75` (Turing) for PTX and `sm_80+` for Tile IR.
- [x] **Shared hardware overcommit**: Preserves the 2,048 MB host floor from Principle 11.
- [x] **Unbounded foreign driver waits**: Prohibits blocking ioctls; enforces 50ms async cancellation token.

---

## Open Questions
None. All architectural decisions (DT-1 through DT-4) are closed.

---

## Verdict

### **`go`**

The specification satisfies all SSDV3 and Kahneman criteria without unresolved defects or hard no-go triggers. Proceed to **STEP 3 — IMPL**.
