# FINDING_ONLY: Architectural Trap in NBD Checksum Header

**Identity:** ResilienceChaos100/2026-09-06/data-integrity/067
**Target:** `crates/ramshared-block/src/request.rs`

## Analysis
The objective requested adding and verifying a "checksum header on incoming block write requests before committing to storage backend". However, `crates/ramshared-block/src/request.rs` implements the standard Network Block Device (NBD) fixed-newstyle transmission phase.

According to the NBD specification (SPEC §10.1), a write request strictly consists of a 28-byte request header immediately followed by exactly `len` bytes of unformatted payload (the block data).

Injecting an inline checksum header into the payload (e.g., prepending 8 bytes of hash) violates the protocol in multiple fatal ways:
1. **Misalignment & Rejection:** The `validate` function enforces that `req.len` is an exact multiple of the block size (e.g., 4096). If a checksum is subtracted from the length, the data will no longer align.
2. **Standard Client Breakage:** Upstream Linux kernel NBD clients and standard tools do not attach an inline checksum header to the raw block data. Requiring one would unconditionally break all standard NBD client attachments.
3. **Data Corruption:** Treating part of the payload as a checksum header would result in writing shifted or truncated block data to the storage backend, permanently corrupting the guest filesystem.

Data integrity in RamShared (such as the detection of torn reads and bit-flips) is designed to be maintained safely out-of-band via pre-allocated structures like `ChecksumTable` (SPEC §8.1), tracking FNV-1a 64 hashes over the exact, unmodified payload buffers, rather than polluting the NBD wire protocol.

## Verdict
This is an architectural scope trap. The codebase cannot be safely modified to embed a checksum header within the NBD write request payload without destroying protocol compliance and corrupting data. No code modifications were made.
