import re

with open("crates/ramshared-block/src/handshake.rs", "r") as f:
    content = f.read()

# Replace fn definition
server_handshake = """pub fn server_handshake<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    exports: &[Export],
    tx_flags: u16,
) -> Result<usize, HandshakeError> {"""

new_server_handshake = """pub fn server_handshake<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    exports: &[Export],
    tx_flags: u16,
    auth_state: Option<&Authenticator>,
) -> Result<usize, HandshakeError> {"""

content = content.replace(server_handshake, new_server_handshake)


with open("crates/ramshared-block/src/handshake.rs", "w") as f:
    f.write(content)
