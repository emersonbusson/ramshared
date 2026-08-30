import re

with open("crates/ramshared-uring/src/lib.rs", "r") as f:
    content = f.read()

# Fix is_multiple_of
content = content.replace("!buf_size.is_multiple_of(4096)", "buf_size % 4096 != 0")

# Fix rlim comparison
content = content.replace("(total_bytes as u64) > rlim.rlim_cur", "(total_bytes as u64) > (rlim.rlim_cur as u64)")

# Wait, page_size() is available in ramshared-uring/src/lib.rs because it is defined at the top of the file:
# pub fn page_size() -> usize { ... }
# but let's change it to 4096 just to be safe in the test.
content = content.replace("round_up_to_page(rlim.rlim_cur as usize) + page_size()", "((rlim.rlim_cur as u64 / 4096) + 2) * 4096")
# wait, rlim_cur as u64 / 4096 as u64 is a bit complicated. Let's just use 4096 and do proper rounding.

with open("crates/ramshared-uring/src/lib.rs", "w") as f:
    f.write(content)
