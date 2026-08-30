1. **RULES**:
   - Guard Clauses over nested if/else.
   - Specific & semantic errors.
   - Run tests, check coverage, zero unsafe, commit using conventional style.

2. **MAIN_DIFF**:
   - Re-implement `StateTransitionError` using the rich domain type `StateTag` instead of `&'static str`.
     ```rust
     #[derive(Clone, Copy, Debug, Eq, PartialEq)]
     pub enum StateTransitionError {
         IllegalTransition {
             expected: Option<StateTag>,
             actual: StateTag,
         },
         StaleGeneration {
             provided: u64,
             expected: u64,
         },
     }
     ```
   - Update `crates/ramshared-tier/src/n3_state.rs` locations that return `FailureReason::StateTransition` to use `StateTag` variants properly. Preflight actions that don't have a `StateTag` equivalent should probably just pass `expected: None` and `actual: StateTag::Absent`, or we can add `PreflightState` to the error variant if necessary. But wait, `StateTag` has `Absent`, `Negotiating`, `Granted`, `Quiescing`, `Drained`, `Revoked`, `Failed`. Preflight states don't have `StateTag`. For `request_demotion`, the machine state is actually `LeaseState::Absent`. `actual` should use `self.lease_state.tag()`. Wait, `request_demotion` is on `PreflightModel` not `LeaseMachine`! In `PreflightModel`, it doesn't have `self.lease_state.tag()`.
     If `PreflightModel::request_demotion` returns `InvalidTransition`, we can pass a dummy or we need `PreflightState` context. But the code review said "use rich domain types (like StateTag or LeaseState)". So I will add `PreflightState` instead of strings for preflight. But `PreflightModel` has `PreflightState`.
     Let's define:
     ```rust
     #[derive(Clone, Copy, Debug, Eq, PartialEq)]
     pub enum StateTransitionError {
         IllegalTransition {
             expected: StateTag,
             actual: StateTag,
         },
         IllegalPreflight {
             expected: PreflightState,
             actual: PreflightState,
         },
         StaleGeneration {
             provided: u64,
             expected: u64,
         },
     }
     ```
     This perfectly satisfies exact context using domain types.
     Wait, I can just do:
     ```rust
     #[derive(Clone, Copy, Debug, Eq, PartialEq)]
     pub enum StateTransitionError {
         IllegalTransition {
             expected: Option<StateTag>,
             actual: StateTag,
         },
         IllegalPreflight {
             expected: Option<PreflightState>,
             actual: PreflightState,
         },
         StaleGeneration {
             provided: u64,
             expected: u64,
         },
     }
     ```

3. **FILES**:
   - `crates/ramshared-tier/src/n3_state.rs`
   - `crates/ramshared-tier/tests/n3_state.rs`

4. **INVARIANTS**:
   - Use actual rich domain types.

5. **COUNTERFACTUAL**:
   - `&'static str` violates the requirement for semantic error returns.

6. **RED_TEST**:
   - Modify test file to use the rich domain types.

7. **COVERAGE**:
   - Same as before.

8. **REAL_PROOF**:
   - `cargo test` & `cargo clippy`.

9. **ROLLBACK**:
   - Compilation failure.

10. **PR_BOUNDARY**:
   - Target PR to `jules/inbox`.
