import re

with open("crates/ramshared-uring/src/lib.rs", "r") as f:
    content = f.read()

# Ah, `is_multiple_of` actually IS stable in the compiler we're using (1.98.0).
# The reviewer's feedback was wrong on that. Let's revert back to `!buf_size.is_multiple_of(4096)`.
content = content.replace("buf_size % 4096 != 0", "!buf_size.is_multiple_of(4096)")

# Revert unnecessary cast. On our compiler, `rlim_t` is `u64`.
content = content.replace("(rlim.rlim_cur as u64)", "rlim.rlim_cur")

# Fix the test compilation error. massive_buf_size is u64 but validate... takes usize.
# massive_buf_size should just be `(rlim.rlim_cur as usize / 4096 + 2) * 4096`
# Wait, `rlim.rlim_cur as usize` could panic or overflow if rlim_cur is larger than usize.
# On 64-bit platforms they are the same. Let's cast using `try_into`.
content = content.replace("((rlim.rlim_cur as u64 / 4096) + 2) * 4096", "(((rlim.rlim_cur / 4096) + 2) * 4096) as usize")

with open("crates/ramshared-uring/src/lib.rs", "w") as f:
    f.write(content)
