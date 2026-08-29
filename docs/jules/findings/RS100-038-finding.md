# FINDING_ONLY: Non-existent dummy_req logic in workload.rs

**Target**: `crates/ramshared-cli/src/workload.rs`

## Observation
The adversarial audit prompt requested to hoist a repeated `dummy_req` allocation outside a loop in a benchmark workload within `crates/ramshared-cli/src/workload.rs`.

However, `dummy_req` does not exist in `crates/ramshared-cli/src/workload.rs`. It exists in `crates/ramshared-wsl2d/src/conn.rs`.

Furthermore, there is no "benchmark workload" loop with a `dummy_req` allocation in `crates/ramshared-cli/src/workload.rs`.

## Conclusion
Due to the strict confinement rule targeting `crates/ramshared-cli/src/workload.rs`, safe code modification to implement the requested change is architecturally impossible within the allowed scope. No code was hallucinated, and this `FINDING_ONLY` report was produced with the required evidence.

## Evidence

Running a `grep` for `dummy_req` in the specified file yields no results:
```bash
grep -inR "dummy_req" crates/ramshared-cli/src/workload.rs
# (No output)
```

Running a `grep` for `dummy_req` in the workspace shows it is located in a different crate:
```bash
grep -inR "dummy_req" .
./crates/ramshared-wsl2d/src/conn.rs:427:    fn dummy_req() -> Request {
./crates/ramshared-wsl2d/src/conn.rs:442:            req: dummy_req(),
./crates/ramshared-wsl2d/src/conn.rs:762:                    req: dummy_req(),
```
