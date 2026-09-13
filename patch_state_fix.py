import re

with open("crates/ramshared-block/src/handshake.rs", "r") as f:
    content = f.read()

# Add the verify logic
logic = """    if let Some(authenticator) = auth_state {
        if !authenticator.verify(ts, nonce) {
            return Err(HandshakeError::ReplayDetected);
        }
    }"""

new_logic = """    if let Some(authenticator) = auth_state {
        if !authenticator.verify(ts, nonce) {
            return Err(HandshakeError::ReplayDetected);
        }
    }"""

# Ah, I replaced "In production we would track nonces too." which wasn't in the file yet!
# Let me replace the actual ts == u64::MAX stub instead.

stub = """    if ts == u64::MAX && nonce == u64::MAX {
        return Err(HandshakeError::ReplayDetected);
    }"""

content = content.replace(stub, logic)

with open("crates/ramshared-block/src/handshake.rs", "w") as f:
    f.write(content)
