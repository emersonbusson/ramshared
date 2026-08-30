#!/bin/bash
# Check the execution of `cargo clippy` and `cargo test` on this crate
cd crates/ramshared-wsl2d
cargo clippy -- -D warnings
cargo test
