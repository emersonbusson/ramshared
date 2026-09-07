# Architectural Scope Trap: NBD Protocol Handshake Re-authentication

**Target File:** `crates/ramshared-block/src/handshake.rs`
**Objective:** Re-validate session security tokens upon reconnection following system wake.

## Evidence

The target file implements the `server_handshake` function for the standard NBD (Network Block Device) fixed-newstyle protocol. The file comment explicitly states:
`//! NBD fixed-newstyle: **server**-side handshake (export negotiation).`
`//! Generic over `Read + Write` -> testable without socket/root. SPEC §10.1.`
`//! Supports NBD_OPT_EXPORT_NAME (simple) and NBD_OPT_GO/NBD_OPT_INFO (modern, used by recent versions of nbd-client).`

The standard NBD protocol does not natively support custom session token validation or re-authentication during the export negotiation phase. Injecting custom options or modifying the expected payload structure of `NBD_OPT_GO` or `NBD_OPT_EXPORT_NAME` to carry security tokens would violate the fixed-newstyle protocol specification (SPEC §10.1) and break compatibility with standard tools like `nbd-client`. Additionally, system sleep and wake power events are handled outside of the pure stream-oriented NBD handshake parser.

## Conclusion

Implementing session token validation in this file would introduce hidden statefulness and violate standard NBD protocol boundaries. Thus, this is an architectural scope trap, and no code modifications have been made.
