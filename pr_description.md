## Summary
Added rate limiting using a TokenBucket algorithm to `crates/ramshared-block/src/request.rs` to mitigate resource exhaustion and request flooding attacks.

## Commits
- feat: implement token bucket rate limiter in ramshared-block
- test: add coverage for request rate limiting

## Validation
- `cargo clippy --all-targets` passes with 0 warnings.
- `cargo test` passes.
- Confirmed `ORTHOGONAL SCOPE` by strictly modifying `ramshared-block` and exposing `serve_with_rate_limit` alongside `serve`, avoiding cross-crate breaking changes or test-relocation CI locks.

## Rollback trigger
If the new `TokenBucket` causes unexpected timeouts or false-positive rate limiting during valid bursts, or if downstream crates fail to compile when opting into `serve_with_rate_limit`.
