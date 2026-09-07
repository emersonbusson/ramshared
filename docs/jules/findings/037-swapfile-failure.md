# Finding: Swapfile Allocation Failure Handling (037)

The prompt requests graceful handling of `fallocate` / `mkswap` `ENOSPC` failures during swap expansion, falling back to a minimal pool without system panic in `crates/ramshared-agent/src/swap.rs`.

However, as per the code in `swap.rs`, the file specifically manages Swap execution over **NBD**. It attaches an NBD block device (`nbd-client`), runs `mkswap` on the block device itself (`/dev/nbdX`), and turns it on with `swapon`. It does *not* allocate swapfiles using `fallocate`.

The system relies on exported block devices provided by a broker over NBD (or VRAM directly). The swap sizing and ENOSPC errors are mapped via standard system error codes when running these commands (e.g., `SwapError::DiskFull` from `28` ENOSPC). Since there is no `fallocate` logic for files in the agent (as it operates on NBD block devices instead of regular files), adding file-based swapfile expansion logic with `fallocate` is an architectural trap that contradicts the established VRAM-over-NBD architecture.

As a result, no code changes are necessary, and this report fulfills the `FINDING_ONLY` requirement for safe orthogonal slice handling.
