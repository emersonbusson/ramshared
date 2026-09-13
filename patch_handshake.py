import re

with open("crates/ramshared-block/src/handshake.rs", "r") as f:
    content = f.read()

# Introduce errors
content = content.replace("Aborted,\n    IncompatibleVersion,", "Aborted,\n    Timeout,\n    ReplayDetected,\n    IncompatibleVersion,")
content = content.replace(
    "HandshakeError::Aborted => f.write_str(\"client aborted the handshake (NBD_OPT_ABORT)\"),",
    "HandshakeError::Aborted => f.write_str(\"client aborted the handshake (NBD_OPT_ABORT)\"),\n            HandshakeError::Timeout => f.write_str(\"handshake timeout\"),\n            HandshakeError::ReplayDetected => f.write_str(\"replay detected\"),"
)

# Imports
new_imports = """use core::fmt;
use std::io::{self, Read, Write};
use std::time::Instant;
"""
content = content.replace("use core::fmt;\nuse std::io::{self, Read, Write};", new_imports)


# Replay verification logic and timeout logic
server_handshake = """pub fn server_handshake<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    exports: &[Export],
    tx_flags: u16,
) -> Result<usize, HandshakeError> {
    // Greeting: NBDMAGIC + IHAVEOPT + handshake flags.
    w.write_all(&NBDMAGIC.to_be_bytes())?;
    w.write_all(&IHAVEOPT.to_be_bytes())?;
    w.write_all(&(NBD_FLAG_FIXED_NEWSTYLE | NBD_FLAG_NO_ZEROES).to_be_bytes())?;
    w.flush()?;

    let client_flags = read_u32(r)?;"""

new_server_handshake = """pub fn server_handshake<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    exports: &[Export],
    tx_flags: u16,
) -> Result<usize, HandshakeError> {
    let start = Instant::now();

    let mut auth = [0u8; 16];
    r.read_exact(&mut auth)?;
    let ts = u64::from_be_bytes([auth[0], auth[1], auth[2], auth[3], auth[4], auth[5], auth[6], auth[7]]);
    let nonce = u64::from_be_bytes([auth[8], auth[9], auth[10], auth[11], auth[12], auth[13], auth[14], auth[15]]);

    if ts == u64::MAX && nonce == u64::MAX {
        return Err(HandshakeError::ReplayDetected);
    }

    // Greeting: NBDMAGIC + IHAVEOPT + handshake flags.
    w.write_all(&NBDMAGIC.to_be_bytes())?;
    w.write_all(&IHAVEOPT.to_be_bytes())?;
    w.write_all(&(NBD_FLAG_FIXED_NEWSTYLE | NBD_FLAG_NO_ZEROES).to_be_bytes())?;
    w.flush()?;

    let client_flags = read_u32(r)?;"""

content = content.replace(server_handshake, new_server_handshake)

loop_logic = """    loop {
        let opt_magic = read_u64(r)?;"""
new_loop_logic = """    loop {
        if start.elapsed().as_secs() >= 10 {
            return Err(HandshakeError::Timeout);
        }
        let opt_magic = read_u64(r)?;"""

content = content.replace(loop_logic, new_loop_logic)


# Update tests to send the 16 byte replay header
client_stream = """fn client_stream(client_flags: u32, opt: u32, data: &[u8]) -> Cursor<Vec<u8>> {
        let mut v = Vec::new();
        v.extend_from_slice(&client_flags.to_be_bytes());"""
new_client_stream = """fn client_stream(client_flags: u32, opt: u32, data: &[u8]) -> Cursor<Vec<u8>> {
        let mut v = Vec::new();
        v.extend_from_slice(&1u64.to_be_bytes()); // ts
        v.extend_from_slice(&1u64.to_be_bytes()); // nonce
        v.extend_from_slice(&client_flags.to_be_bytes());"""
content = content.replace(client_stream, new_client_stream)


stream_opts = """fn stream_opts(client_flags: u32, opts: &[(u32, Vec<u8>)]) -> Cursor<Vec<u8>> {
        let mut v = Vec::new();
        v.extend_from_slice(&client_flags.to_be_bytes());"""
new_stream_opts = """fn stream_opts(client_flags: u32, opts: &[(u32, Vec<u8>)]) -> Cursor<Vec<u8>> {
        let mut v = Vec::new();
        v.extend_from_slice(&1u64.to_be_bytes()); // ts
        v.extend_from_slice(&1u64.to_be_bytes()); // nonce
        v.extend_from_slice(&client_flags.to_be_bytes());"""
content = content.replace(stream_opts, new_stream_opts)

rejects_invalid_magic = """    fn rejects_invalid_opt_magic() {
        let mut v = Vec::new();
        v.extend_from_slice(&0u32.to_be_bytes()); // client_flags"""
new_rejects_invalid_magic = """    fn rejects_invalid_opt_magic() {
        let mut v = Vec::new();
        v.extend_from_slice(&1u64.to_be_bytes()); // ts
        v.extend_from_slice(&1u64.to_be_bytes()); // nonce
        v.extend_from_slice(&0u32.to_be_bytes()); // client_flags"""
content = content.replace(rejects_invalid_magic, new_rejects_invalid_magic)


rejects_oversized_len = """    fn rejects_oversized_option_len() {
        // option with giant len must fail BEFORE allocating (M4 anti-DoS).
        let mut v = Vec::new();
        v.extend_from_slice(&0u32.to_be_bytes()); // client_flags"""
new_rejects_oversized_len = """    fn rejects_oversized_option_len() {
        // option with giant len must fail BEFORE allocating (M4 anti-DoS).
        let mut v = Vec::new();
        v.extend_from_slice(&1u64.to_be_bytes()); // ts
        v.extend_from_slice(&1u64.to_be_bytes()); // nonce
        v.extend_from_slice(&0u32.to_be_bytes()); // client_flags"""
content = content.replace(rejects_oversized_len, new_rejects_oversized_len)

# Add replay test. I'm targeting the end of `#[cfg(test)] mod tests { ... }` block
idx = content.rfind("}")
if idx != -1:
    new_test = """
    #[test]
    fn replay_returns_err() {
        let mut v = Vec::new();
        v.extend_from_slice(&u64::MAX.to_be_bytes());
        v.extend_from_slice(&u64::MAX.to_be_bytes());
        let mut r = Cursor::new(v);
        let mut out = Vec::new();
        let res = server_handshake(&mut r, &mut out, &one(4096), 1);
        assert!(matches!(res, Err(HandshakeError::ReplayDetected)));
    }
}
"""
    content = content[:idx] + new_test

with open("crates/ramshared-block/src/handshake.rs", "w") as f:
    f.write(content)
