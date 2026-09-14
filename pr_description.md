## Summary
Adds protocol version negotiation header and implements backward-compatible field addition strategy.
- Decomposed `crates/ramshared-broker/src/protocol.rs` into `protocol/mod.rs`, `protocol/msg.rs`, and `protocol/codec.rs`.
- Added `VersionHeader` struct to `Msg::Register`.
- Added `features: Vec<String>` to `Msg::Registered`.
- Added `#[serde(other)] Unknown` variants to `Msg` and `NbdEndpoint`.
- Added `#[non_exhaustive]` to structs and enums in `Msg`.

## Commits
- feat(broker): add protocol version negotiation header and backward compatibility

## Validation
- Ran `cargo test` and `cargo clippy --all-targets`.
- Verified protocol unit tests for new backwards-compatible fields and unknown type parsing.
- E2E broker tests passing.

## Rollback trigger
Revert commit if protocol parsing regressions are found or `cargo test` fails.
