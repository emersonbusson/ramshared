# Finding Report: Block Offset Calculations

**Architectural Principle Assessed**: Physical Limits & Sanity Checks
**Target Scope**: `crates/ramshared-block/src/request.rs`

## Finding

The user requested that we enforce checked 64-bit arithmetic on block offset calculations using `checked_add`/`checked_mul` in `crates/ramshared-block/src/request.rs` to eliminate integer wrapping and buffer overflows.

However, an exhaustive audit of the `request.rs` production codebase reveals that the requested implementation **already exists and correctly adheres to the physical limits principle**.

The primary production bounds-checking logic is encapsulated in the `validate` function, which explicitly uses `checked_add` and verifies boundary constraints before any backend dispatch occurs.

Evidence from `crates/ramshared-block/src/request.rs`:
```rust
fn validate<B: BlockBackend + ?Sized>(req: &Request, backend: &B) -> Result<(), u32> {
    let bs = backend.block_size() as u64;
    if bs == 0 {
        return Err(NBD_EINVAL);
    }
    if !req.offset.is_multiple_of(bs) {
        return Err(NBD_EINVAL);
    }
    if !(req.len as u64).is_multiple_of(bs) {
        return Err(NBD_EINVAL);
    }

    let Some(end) = req.offset.checked_add(req.len as u64) else {
        return Err(NBD_ERANGE);
    };
    if end > backend.size_bytes() {
        return Err(NBD_ERANGE);
    }

    if req.cmd == Command::Write && backend.is_read_only() {
        return Err(NBD_EACCES);
    }

    Ok(())
}
```

This logic already:
1. Validates physical hardware block alignment (`is_multiple_of(bs)`).
2. Explicitly uses `checked_add` for calculating the physical end boundary.
3. Aborts and returns the semantic precise domain error (`NBD_ERANGE`) immediately on overflow, preventing buffer corruption.

The only instances of unchecked arithmetic (`o..o + buf.len()`) reside exclusively within the test-only mock implementations (`mod tests`) such as `MemBackend` and `ReadOnlyBackend`. Modifying these test mocks violates the user's intent to enforce production hardware limits, as verified by previous Code Review attempts which correctly rejected modifying only test mocks.

Therefore, because the production codebase in `request.rs` already perfectly complies with the defensive programming contract, no safe production code change can be orthogonal. This is a known configuration state, and this report serves as explicit documentation of existing compliance.
