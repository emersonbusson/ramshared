#!/bin/bash
cargo test --package ramshared-winsvc --lib -- evidence::tests::stable_error_redacts_payload
