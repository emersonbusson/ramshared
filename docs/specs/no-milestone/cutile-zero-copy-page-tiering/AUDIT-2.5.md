# AUDIT-2.5 — cutile-zero-copy-page-tiering

## Scope of Review
- **Subsystem**: Zero-copy host memory registration (`cuMemHostRegister`), `DeviceAllocation` foreign memory bridge, bitwise Tile reductions (`reduce_xor`, `reduce_and`, `reduce_or`).
- **Risk Surface**: DMA over PCIe, OS host memory locking (`RLIMIT_MEMLOCK`), GPU virtual addressing, foreign lifetime token validity, thread concurrency under asynchronous execution.

---

## Findings

| Sev | SPEC § | Issue | Required Fix / Verification |
| :--- | :--- | :--- | :--- |
| **Low** | §Security Checklist | Potential TOCTOU if host memory is deallocated via `free()` while registered by CUDA. | Enforce that `PinnedHostMapping` takes ownership or holds a lifetime guard on the underlying allocation. The caller must guarantee the host allocation remains valid until the mapping is dropped. |
| **Low** | §DT-2 | Page alignment assumption on platforms with 64KB pages (e.g. ARM64). | Ensure alignment check uses system page size or validates 4096-byte alignment as the minimum base for CUDA page locking. |
| **Info** | §Kahneman Map | Multi-threaded context sharing of registered host memory. | Validated: `CU_MEMHOSTREGISTER_PORTABLE` flag explicitly permits all CUDA contexts to access the mapping safely. |

---

## Open Questions

1. *Does `cuMemHostGetDevicePointer` return an identical pointer to `host_ptr` on 64-bit platforms with UVA enabled?*
   - **Answer**: Yes, on 64-bit systems with UVA (Unified Virtual Addressing, standard across all modern Linux/WSL2 NVIDIA drivers), `dev_ptr == host_ptr as CUdeviceptr`. However, explicitly calling `cuMemHostGetDevicePointer_v2` is required by the driver API contract for portability across virtualized hypervisor setups.

2. *Can `reduce_xor` operate on sub-byte types or only primitive integer types?*
   - **Answer**: Bitwise reductions (`reduce_xor`, `reduce_and`, `reduce_or`) are restricted to integer and byte element types (`u8`, `u16`, `u32`, `u64`, `i8`, `i16`, `i32`, `i64`). Floating point types (`f16`, `f32`, etc.) are rejected at compile time by `cutile-compiler`.

---

## Anti-Skynet & Host Safety Verification

- **Host Memory Reserve Cushion**: Host registrations are strictly non-swappable physical pages. A hard process cap of $512\text{ MB}$ prevents runaway swap exhaustion on the host OS.
- **Rollback Safety**: Any driver registration error triggers instant fallback to staged bounce-buffer DMA.
- **Zero Freeze**: All GPU memory transfers remain stream-ordered and asynchronous, preventing blocking kernel threads.

---

## Verdict

**`go`**
The specification satisfies all SSDV3 criteria, enforces Kahneman disciplines #13, #16, and #17, provides explicit executable tests for every critical path, and maintains strict host safety boundaries.
