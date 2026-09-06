# ramshared-integrity

Block-level checksum verification, torn-read detection, and data corruption prevention.

## Scope & Responsibility

`ramshared-integrity` provides fast, continuous data validation across memory tiers:
- **Block Hashing:** Fast non-cryptographic checksum algorithms indexed by block number to detect torn reads or VRAM bit-flips.
- **Pre-Allocated Checksum Tables:** Memory-efficient checksum indexes capable of tracking multi-gigabyte block devices with minimal overhead.
- **Reproducible Test Patterns:** Deterministic block generation and verification routines for stress and resilience qualification drills.

## Workspace Dependencies

- Pure algorithm library; zero internal workspace dependencies.

## Safety Invariants

- **Safe Code Only:** `#![forbid(unsafe_code)]` enforced.
- **Zero Panic:** Returns typed [`IntegrityError`](src/pattern.rs) and [`ChecksumMismatchError`](src/hash.rs) upon data corruption.

## Testing

```bash
cargo test -p ramshared-integrity
```
