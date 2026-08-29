1. RULES
Read README.md, AGENTS.md, CLAUDE.md, etc.
Baseline git SHA is 578aba8be59861c70c788e80cb1c4349cfaf13c7 on origin/main.
No unvetted unsafe code.
...
2. MAIN_DIFF
Refactor `session` loop in `crates/ramshared-agent/src/main.rs` to use `SessionDispatcher` with methods `tick_psi`, `drain_exec`, `dispatch_msg`, `check_watchdog`.
3. FILES
- `crates/ramshared-agent/src/main.rs`
4. INVARIANTS
Must pass `cargo test` and `cargo clippy -- -D warnings`.
Event loop must remain single-threaded dispatch.
5. COUNTERFACTUAL
If we just inline everything, it violates the decomposition requirement. If we allocate unnecessary boxes, it impacts performance. We use a zero-cost struct with methods.
6. RED_TEST
We will add `test_session_dispatcher_methods` test in `mod tests` of `main.rs` that explicitly calls the new `SessionDispatcher` methods and asserts state changes, which will fail to compile until the struct is implemented.
7. COVERAGE
Validates the state machine transitions and side-effects.
8. REAL_PROOF
Run the test suite successfully after implementation.
9. ROLLBACK
If regressions occur in the agent connection loop, the PR will be reverted.
10. PR_BOUNDARY
Target branch: jules/inbox, unmerged.
