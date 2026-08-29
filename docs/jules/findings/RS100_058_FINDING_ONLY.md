# FINDING_ONLY: reuse packet buffer in block write dispatch path

**TASK_TITLE:** reuse packet buffer in block write dispatch path

## Evidence

The instructions state:
- Scope is strictly confined to: `crates/ramshared-block/src/vram_backend.rs` and its related test module.
- Goal: Eliminate intermediate buffer reallocation during high-throughput block write dispatching.

However, after examining the target file (`crates/ramshared-block/src/vram_backend.rs` to its final line, line 199), there is no intermediate buffer reallocation in the write dispatch path within `VramBackend`. The `write_at` method receives the payload data as a slice reference and forwards it directly to the underlying `VramMemory` backend.

```rust
    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
        self.mem
            .write_at(off, data)
            .map_err(|e| IoError(e.to_string()))
    }
```

The payload is passed by reference (`data: &[u8]`) into `write_at`. The method allocates no memory.

Further investigation of the codebase reveals that the buffer allocation for WRITE request payloads actually occurs in the network reader thread located in `crates/ramshared-wsl2d/src/conn.rs` inside the `spawn_reader` function (around line 160):

```rust
            let payload = if req.cmd == Command::Write {
                let mut p = vec![0u8; req.len as usize];
                if reader.read_exact(&mut p).is_err() {
                    break;
                }
                p
            } else {
                Vec::new()
            };
```

This allocates a new `Vec<u8>` for every write request. Modifying this logic would require changes to `ramshared-wsl2d/src/conn.rs` (and potentially redefining the `Job` struct to reuse buffers via an arena/pool).

Because this logic resides in `ramshared-wsl2d` and we are strictly confined to `crates/ramshared-block/src/vram_backend.rs`, it is architecturally impossible to implement buffer reuse logic within the assigned scope.

Thus, we are generating this `FINDING_ONLY` report as mandated by IMMUTABLE CONTRACT rule #4 ("If safe code is not possible, produce FINDING_ONLY with evidence in docs/jules/findings/").

## Contractual Blocks

1. RULES: Strictly follow repo standards, confine scope to `crates/ramshared-block/src/vram_backend.rs`.
2. MAIN_DIFF: No diff in `vram_backend.rs`. Creating a FINDING_ONLY report.
3. FILES: `docs/jules/findings/RS100_058_FINDING_ONLY.md`
4. INVARIANTS: `VramBackend` must remain a zero-allocation forwarding wrapper.
5. COUNTERFACTUAL: If we attempted to reuse a buffer, it would be architecturally incorrect as the payload is passed by reference and the actual allocation happens in `ramshared-wsl2d/src/conn.rs`.
6. RED_TEST: N/A for FINDING_ONLY.
7. COVERAGE: N/A for FINDING_ONLY.
8. REAL_PROOF: Target file read to line 199. No reallocation logic exists in `write_at`. The actual allocation was found in `crates/ramshared-wsl2d/src/conn.rs`.
9. ROLLBACK: N/A
10. PR_BOUNDARY: do not merge
