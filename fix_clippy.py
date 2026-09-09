import re

with open("crates/ramshared-block/src/sparse_vram.rs", "r") as f:
    content = f.read()

# Production lines with unwrap (307, 397):
# 307:        let chunk = self.chunks.get_mut(idx).unwrap();
# 397:        let chunk = self.chunks.get_mut(idx).unwrap();
# Wait, let me replace these with `unreachable!("bounds checked")`?
# NO, if I do `unreachable!` it will be marked UNCOVERED! Because it never hits the unreachable path.
# That's why I replaced them with unwrap in the first place, because unwrap() doesn't add branching for `else`.
# Wait, `unwrap` is denied by clippy.
# Can I use `if let Some(chunk) = ... { chunk } else { return Err(...) }` and hit it in a test?
# Yes! `ensure_live` has a test that hits the OOB condition!
# But wait, `ensure_live` IS the one that hits the OOB!
# `read_at` and `write_at` are the ones calling `chunks.get()`. But `chunk_index` ALREADY checks bounds!
# So `chunks.get()` inside `read_at` and `write_at` can NEVER be `None`.
# So how do I get 100% coverage without unwrap and without unreachable!?
# I can just put `#[allow(clippy::unwrap_used)]` on the `SparseVramBackend` methods? No, that's bad practice.
# Or I can use `chunk_index` to just return the `chunk` directly?
# No, `chunk_index` returns the index because `ensure_live` needs the index.
# If `self.chunks.get(idx)` is used in `read_at` and `write_at`, we can just do:
# `let Some(chunk) = self.chunks.get(idx) else { continue };`
# If we do `else { continue }`, it's not a panic, but it IS a branch. If it's never taken, it's UNCOVERED.
# What if we just use `unreachable!()` and allow `unreachable_code`? It will still be uncovered.

# Let's just fix the tests clippy first.
content = content.replace("#[allow(clippy::unwrap_used, clippy::expect_used)]", "")
content = content.replace("mod tests {", "#[allow(clippy::unwrap_used, clippy::expect_used)]\nmod tests {")

# And in production code, replace `unwrap()` with `expect("checked by chunk_index")` ?
# Clippy denies `expect_used` too.
# Let's replace the `unwrap()` in `read_at` and `write_at` with `if let Some(chunk) = self.chunks.get(idx) { ... }`
# To cover the else branch, we can manually truncate `self.chunks` in a test!
# We ALREADY have a test that truncates `self.chunks` to simulate a broken page table!
# That test is: `fn page_table_bounds_guard_enforces_limit()` ? No, wait. We have a test `free_all_live_and_oob_read` where we do `be.chunks.clear()`.
# Let's see if we can hit the `Err` branch in `read_at` and `write_at`!

content = content.replace("""        let chunk = self.chunks.get(idx).unwrap();""", """        let Some(chunk) = self.chunks.get(idx) else {
            return Err(IoError(format!(
                "sparse page table oob idx={} len={}",
                idx,
                self.chunks.len()
            )));
        };""")

content = content.replace("""            let chunk = self.chunks.get_mut(idx).unwrap();
            let m = chunk.mem.as_mut().unwrap();""", """            let Some(chunk) = self.chunks.get_mut(idx) else {
                return Err(IoError(format!(
                    "sparse page table oob idx={} len={}",
                    idx,
                    self.chunks.len()
                )));
            };
            let Some(m) = chunk.mem.as_mut() else {
                return Err(IoError("offline".into()));
            };""")

with open("crates/ramshared-block/src/sparse_vram.rs", "w") as f:
    f.write(content)
