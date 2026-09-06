# Finding Report: NBD Peer Block Size Architectural Mismatch

## Context
The task requested to "validate peer block size is a power of two between 512 and 65536" within `crates/ramshared-block/src/handshake.rs`. However, after investigating the implementation of the NBD protocol server handshake (export negotiation), this was identified as an architectural mismatch trap.

## Evidence
In the NBD protocol, the client **never** sends its own block size value to the server during the handshake phase (`NBD_OPT_GO` or `NBD_OPT_INFO`). Instead, the client merely sends an information request (e.g., `NBD_INFO_BLOCK_SIZE`, identifier `3`) instructing the server to advertise its *server-side* size constraints (minimum, preferred, and maximum sizes).

The structure of the `NBD_OPT_GO` client payload processed in `crates/ramshared-block/src/handshake.rs` is extracted here:
```rust
/// Extracts the export name from the `NBD_OPT_GO`/`NBD_OPT_INFO` payload:
/// `[u32 name_len][name][u16 n_info][...]`. Malformed/truncated ⇒ error (closes).
fn go_export_name(data: &[u8]) -> Result<&[u8], HandshakeError> {
    if data.len() < 4 {
        return Err(HandshakeError::InvalidFormat);
    }
    let name_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let name_end = 4usize
        .checked_add(name_len)
        .ok_or(HandshakeError::InvalidFormat)?;
    // needs the name + n_info (u16) after it.
    if data.len() < name_end + 2 {
        return Err(HandshakeError::InvalidFormat);
    }
    Ok(&data[4..name_end])
}
```
The payload strictly includes the length of the export name, the export name string itself, and `n_info` (a `u16` count of subsequent info request IDs). It does not contain a 32-bit block size value. Since the peer (client) does not send any block size configuration, the server is architecturally incapable of validating it against bounds such as 512 and 65536.

The server's role in this interaction is restricted to advertising its own configured constraints via the `NBD_REP_INFO` response (implemented in `write_export_info`), meaning the objective to validate the peer block size cannot be fulfilled.

RULES followed all guidelines MAIN_DIFF produced finding report for architectural mismatch FILES modified docs/jules/findings/sanity_check_block_065.md INVARIANTS client payload lacks block size field COUNTERFACTUAL if implemented would fail because client does not send block size RED_TEST none COVERAGE none REAL_PROOF none ROLLBACK trigger revert if findings are invalid PR_BOUNDARY do not merge
