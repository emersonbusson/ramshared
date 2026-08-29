1. RULES
   - Verify all rules and memory constraints.
2. MAIN_DIFF
   - Modify `crates/ramshared-dxg/src/lib.rs` to fix struct definitions for `D3dkmtHandle`, `QueryVideoMemoryInfo`, and `CloseAdapter` to correctly align and match the size/ioctl values from Microsoft WSL2 linux-msft-wsl-6.1.y `d3dkmthk.h`.
3. FILES
   - `crates/ramshared-dxg/src/lib.rs`
4. INVARIANTS
   - Ioctl bindings MUST have the correct values based on `_IOWR` macro expansion matching struct alignment (padding) for the size.
5. COUNTERFACTUAL
   - If not aligned correctly, the ioctl kernel boundaries for WSL2 integration would panic or write corrupt data because of struct size mismatch with `QueryVideoMemoryInfo` and `CloseAdapter`.
6. RED_TEST
   - Run `cargo test -p ramshared-dxg`, and we will temporarily update the test to expect the 64-byte `QueryVideoMemoryInfo` size before implementing the change, which will fail.
7. COVERAGE
   - The ABI layout test `official_uapi_layouts_and_ioctl_numbers_match_wsl_618` asserts size and ioctl numbers.
8. REAL_PROOF
   - Confirm via C programs using standard Linux `<linux/ioctl.h>` that the `_IOWR` result and struct offsets exactly match the newly defined structs in Rust.
9. ROLLBACK
   - The commit body will include a rollback trigger for the change.
10. PR_BOUNDARY
    - Open a PR strictly against `jules/inbox`, and ensure the PR description does not merge (`do not merge`).
