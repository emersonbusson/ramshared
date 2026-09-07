# FINDING_ONLY: Zero-allocation priority tier sorting

The request to implement "zero-allocation priority tier sorting under extreme memory starvation" in `crates/ramshared-tier/src/priority.rs` is an architectural scope trap.

The `ramshared-tier` crate only sets static tier priorities (e.g., `zram > VRAM > VHDX`) for `swapon`. It does not govern memory slices or dynamic sorting. The Linux kernel VM handles actual page eviction. Implementing dynamic sorting of priority tiers is invalid as the architecture enforces a strict static cascade priority.
