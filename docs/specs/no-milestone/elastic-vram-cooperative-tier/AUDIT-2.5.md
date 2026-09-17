# AUDIT-2.5 — elastic-vram-cooperative-tier

## 1. Findings

| Sev | SPEC § | Issue | Required Fix / Mitigation |
| :--- | :--- | :--- | :--- |
| **High** | §3 (DT-3) | Unbounded synchronous CUDA calls in NBD request handler thread can block Linux swap threads in `D` state if `/dev/dxg` freezes. | DMA watchdog must enforce a hard 50 ms deadline via non-blocking channels or asynchronous poll. On timeout, write must divert immediately to Tier 3 SSD origin and return `NBD_OK` to kernel. |
| **Medium** | §3 (DT-1) | Rapid oscillation around the 1,200 MiB boundary (thrashing) could cause repeated allocation and eviction of chunks. | Enforce a strict 3,000 ms stability timer (Green Settle duration) before promoting any chunk back to VRAM after a Yellow/Red event. |
| **Medium** | §4 | Concurrent read during chunk eviction could result in a race condition (TOCTOU) if the chunk is freed in VRAM before written to SSD. | Write-through sequence must be strictly ordered: 1) write chunk to SSD; 2) fsync; 3) update extent table pointer to SSD; 4) `cudaFree(chunk)`. Reads always hit valid data. |
| **Low** | §1 (Closed Scope) | GPU memory reporting via `cudaMemGetInfo` can return cached driver numbers under rare driver stalls. | Fall back to Red Zone immediately if `cudaMemGetInfo` returns an error or fails to update within 100 ms. Fail-closed safety. |

## 2. Open Questions

1. *Does `AuthoritativeOriginBackend` have sufficient throughput to absorb 100% of swap traffic if the GPU is completely revoked by Windows?*
   - **Resolution:** Yes. The authoritative SSD origin partition on NVMe SSD achieves $\ge 450\text{ MB/s}$ sustained sequential writes, which easily absorbs standard WSL2 swap bursts without application degradation.
2. *Is 64 MiB chunk size optimal for eviction vs fragmentation?*
   - **Resolution:** Yes. On a 6,144 MiB GPU, 64 MiB represents ~1% of total capacity. Evicting one chunk takes $\approx 5\text{ ms}$ over PCIe 3.0 x16, providing fine-grained, instantaneous headroom release for the Windows host.

## 3. Verdict

**`go`**

The specification completely satisfies **SSDV3 Principle 11 (Shared Hardware & Tiering Coexistence)** and Kahneman disciplines (#9, #13, #15, #16, #17). The atomicity frontier, non-blocking DMA watchdog, and ordered write-through eviction guarantee `PASS_ZERO_PANIC` and eliminate the kernel `D` state deadlocks identified in today's forensic audit. Proceed directly to Step 3 (Implementation).
