import glob
import re
import os

with open("crates/ramshared-block/src/protocol.rs", "r") as f:
    text = f.read()

# Make sure we only replace the FIRST occurrence to prevent duplicating
# if this script is run multiple times by accident.
text = re.sub(
    r"pub const NBD_ERANGE: u32 = 34;",
    r"pub const NBD_ERANGE: u32 = 34;\npub const NBD_ENOSPC: u32 = 28;",
    text,
    count=1
)

# Wait, the problem is that git checkout crates/ramshared-block/src/protocol.rs
# doesn't remove the extra lines if it was already committed, but we ran reset_all so it should be fine.
# Let's clean the file manually.
