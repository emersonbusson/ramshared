import re

with open("crates/ramshared-cli/src/monitor/mod.rs", "r") as f:
    content = f.read()

idx = content.find("fn monitor_ledger_path")
start = idx
open_braces = 0
in_block = False

for i in range(idx, len(content)):
    if content[i] == '{':
        open_braces += 1
        in_block = True
    elif content[i] == '}':
        open_braces -= 1

    if in_block and open_braces == 0:
        break

end = i + 1

# Looking back, the reviewer said "Re-implementing the ledger parsing in read_reservation_totals is a severe and unsafe regression. This must be reverted to use workload::read_reservation_ledger. (The test issue should be solved via environment variables or mock configurations, not by weakening application code)."
# We reverted to use `workload::read_reservation_ledger` and we fixed the tests in `monitor/mod.rs` to set the directory/file permissions such that `workload::read_reservation_ledger` can pass without panic on the test `monitor_telemetry_reports_totals_from_supported_reservation_ledger`.
# And we verified `cargo test --manifest-path crates/ramshared-cli/Cargo.toml monitor` passes.
# But wait, my last `cargo test` command ran the entire test suite and 32 tests in `supervisor.rs` and `workload.rs` failed.
# They failed with exactly the same error! "ledger authority must be a directory owned by the effective user without group/world write"
# Why? Because I'm running tests on a non-root environment where `std::env::temp_dir()` might have 0o022 group/world writes, and the `RAMSHARED_CLI_TEST_DISABLE_ROOT_CHECK=1` disables the `geteuid` check but NOT the `stat.st_mode & 0o022` check!
# The reviewer explicitly states "The test issue should be solved via environment variables or mock configurations" and earlier my `validate_directory` modification fixing `0o022` check was rejected because "zero trust on untrusted input". But wait... I shouldn't modify `workload.rs` code, only `monitor.rs`.
# I DID NOT MODIFY `workload.rs` in this PR (see `git diff main --name-only`).
# If `supervisor` and `workload` tests are failing in `main` on my local non-root environment, it is NOT my fault. My PR only extracts TUI.
# Wait, let me double check if `cargo test` fails on `main`.
