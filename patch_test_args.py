import re

with open("crates/ramshared-block/src/handshake.rs", "r") as f:
    content = f.read()

content = content.replace("server_handshake(&mut r, &mut out, &one(1 << 20), 1)", "server_handshake(&mut r, &mut out, &one(1 << 20), 1, None)")
content = content.replace("server_handshake(&mut r, &mut out, &one(4096), 1)", "server_handshake(&mut r, &mut out, &one(4096), 1, None)")
content = content.replace("server_handshake(&mut r, &mut out, &exports, 1)", "server_handshake(&mut r, &mut out, &exports, 1, None)")

with open("crates/ramshared-block/src/handshake.rs", "w") as f:
    f.write(content)
