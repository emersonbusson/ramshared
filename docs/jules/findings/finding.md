FINDING_ONLY

The requested specific semantic error `IntegrityError::CorruptedMemory` with exact byte offset and detected bit-flip mask is already perfectly implemented in `crates/ramshared-integrity/src/pattern.rs`.

Evidence from `crates/ramshared-integrity/src/pattern.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityError {
    CorruptedMemory { offset: usize, bit_flip_mask: u8 },
    InvalidStride { stride: usize, page_size: usize },
}
```

```rust
pub fn verify_block(buf: &[u8], idx: u64, kind: Pattern) -> Result<(), IntegrityError> {
    let mut expected = vec![0u8; buf.len()];
    fill_block(&mut expected, idx, kind);
    for (offset, (&actual, &exp)) in buf.iter().zip(expected.iter()).enumerate() {
        if actual != exp {
            return Err(IntegrityError::CorruptedMemory {
                offset,
                bit_flip_mask: actual ^ exp,
            });
        }
    }
    Ok(())
}
```
