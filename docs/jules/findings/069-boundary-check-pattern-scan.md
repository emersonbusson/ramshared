# Finding: Boundary check on pattern scan to prevent buffer overread

The prompt requests implementing a boundary check on pattern scan to prevent buffer overread, in `crates/ramshared-integrity/src/pattern.rs`, with instructions to "defensively clamp pattern scanning length to exact slice capacity, preventing out-of-bounds reads."

However, `verify_block` checks `let stride = buf.len();` and enforces `page_size % stride != 0` returning `InvalidStride`. If `stride` is larger than `page_size`, say `8192`, `4096 % 8192` is `4096`, which is not `0`. Therefore, it correctly returns an `InvalidStride` error before any out-of-bounds read can happen.

If we modify `stride` to be clamped to some capacity, we are fundamentally altering the semantic meaning of the provided `buf`, and `verify_block` would silently verify a sub-slice of `buf`, ignoring the trailing bytes instead of returning an error indicating invalid block boundaries. Clamping the length of an arbitrarily sized slice to a page boundary would obscure the fact that the caller passed an oversized buffer (which is an invalid state). The current behavior is safe: `verify_block` safely zips over `buf` and `expected` (which is correctly allocated to exactly `stride`), so no out of bounds read is possible in rust. The `InvalidStride` error correctly prevents buffer overreads for strides not cleanly dividing the page size.

If this instruction implies `fill_block` might overwrite `expected` with out of bounds, that's impossible because `expected` is created as `vec![0u8; stride]` and `fill_block` strictly uses safe iterators: `buf.iter_mut()`. If the instruction implies `verify_block` reading out of bounds, that's also impossible because of safe zip: `buf.iter().zip(expected.iter())`.

Therefore, the requested code change is an architectural scope trap, or an unnecessary feature that introduces ambiguity or incorrect behavior by silently ignoring invalid buffer sizes rather than throwing `InvalidStride`.

This is an architectural finding.
