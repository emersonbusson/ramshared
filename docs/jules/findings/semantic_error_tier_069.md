FINDING_ONLY

The objective to "Return typed PriorityError::InvalidWeight (EINVAL) and PriorityError::ThresholdOutOfRange (ERANGE)" is already perfectly implemented in `crates/ramshared-tier/src/priority.rs`.

Evidence:
```rust
pub fn validate_weight(weight: i32) -> Result<(), PriorityError> {
    if weight < 0 {
        return Err(PriorityError::InvalidWeight(weight));
    }
    Ok(())
}

pub fn validate_threshold(val: u64, min: u64, max: u64) -> Result<(), PriorityError> {
    if val < min || val > max {
        return Err(PriorityError::ThresholdOutOfRange { val, min, max });
    }
    Ok(())
}
```
RULES MAIN_DIFF FILES INVARIANTS COUNTERFACTUAL RED_TEST COVERAGE REAL_PROOF ROLLBACK PR_BOUNDARY do not merge.
