# FINDING_ONLY: Vector Allocation in Test Fixture Loop (workload.rs:2151)

## Intent
The task requested hoisting a `Vec::new()` initialization and calling `.clear()` on it within a loop in `crates/ramshared-cli/src/workload.rs` to prevent repeated vector allocations.

## Analysis
1. `Vec::new()` in Rust is a `const fn` (`RawVec::NEW`) that creates an empty vector with 0 capacity. It performs **0 heap allocations**.
2. The `ReservationLedger` struct owns its `reservations: Vec<Reservation>` field. In Rust's ownership model, hoisting a vector outside the loop and calling `.clear()` on it in each iteration is impossible because `ReservationLedger` takes ownership of the vector (moving it).
3. The instantiation of `ReservationLedger` (and its `Vec::new()`) is located inside the `Ok(())` arm of `fs::create_dir(&path)`. When `fs::create_dir` succeeds, `serde_json::to_vec` runs once and `return path;` immediately exits the function and loop.
4. On loop iterations where `fs::create_dir(&path)` returns `Err(ErrorKind::AlreadyExists)`, execution hits `continue`, skipping line 2151 entirely. No vector is constructed on retry iterations.
5. The function `fixture()` is a test helper function inside `#[cfg(test)] mod tests` in `crates/ramshared-cli/src/workload.rs`, not a hot production runtime path.

## Conclusion
The claim that line 2151 performs repeated vector allocations in a loop is an architectural and language misconception. Hoisting the initialization and using `.clear()` is both impossible due to Rust move semantics and unnecessary because `Vec::new()` is non-allocating and executes at most once before function return.
