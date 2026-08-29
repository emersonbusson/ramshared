FINDING_ONLY

The task instructs the auditor to use `checked_add` and `checked_mul` on 64-bit block offset calculations within `crates/ramshared-block/src/isolated_origin.rs`.

However, after a thorough review of `crates/ramshared-block/src/isolated_origin.rs`, there are no block index multiplications or offset calculations involving multiplications (`mul`) within this file. The struct `AuthoritativeOriginBackend` receives byte-based offsets directly (e.g. `offset: u64`) and uses simple length additions (`checked_add`) to verify boundaries via `check_range`.

The only geometry validation check is `!size.is_multiple_of(u64::from(block))`.

As the specific code logic (block * block_size) does not exist in this confined file, it is structurally impossible to safely add `checked_mul` logic without hallucinating entirely new features or functions. Thus, this finding acts as evidence under adversarial constraints.
