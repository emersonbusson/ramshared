# FINDING_ONLY: Guard Clause Refactoring Trap

### 1. RULES
Adhere to the CleanArchitecture-100 guard clauses principle and immutable contract. Crucially, as per the memory constraint: "If an instructed task requests modifications to non-existent logic or a fix where safe code modification is impossible, you must not hallucinate code. Instead, generate a FINDING_ONLY report with evidence of the logic's absence in the docs/jules/findings/ directory." The file `crates/ramshared-tier/src/priority.rs` already implements early-return guard clauses and contains no nested if/else priority sorting logic. Thus, this is an adversarial trap.

### 2. MAIN_DIFF
No changes will be made to the `ramshared-tier` crate. A `FINDING_ONLY` report is generated instead.
The `docs/governance/document-lifecycle-policy.json` file is updated to register `docs/jules/findings/**/*.md`.

### 3. FILES
- `docs/jules/findings/guard_clause_trap.md` (New)
- `docs/governance/document-lifecycle-policy.json` (Modified)

### 4. INVARIANTS
Do not hallucinate code. Maintain the exact required PR description format. Ensure all `docs/` files are properly registered and pass `docs-check.sh`.

### 5. COUNTERFACTUAL
If I were to attempt to refactor `validate_order` in `priority.rs`, I would be modifying code that already complies with the guard clause principle, either hallucinating a problem or performing a no-op change, which violates the exact objective and the instruction against hallucinating code.

### 6. RED_TEST
Since this is a `FINDING_ONLY` trap for non-existent logic, no RED_TEST can or should be implemented. We will execute `cargo test` to ensure the baseline remains GREEN.

### 7. COVERAGE
Code coverage is unaffected. Document coverage is maintained by running `node tools/ci/generate-documentation-inventory.mjs --write` and `./scripts/docs-check.sh`.

### 8. REAL_PROOF
The file `crates/ramshared-tier/src/priority.rs` contains the following `validate_order` implementation, which already utilizes guard clauses with early returns:

```rust
pub fn validate_order(p: TierPriorities) -> Result<(), OrderError> {
    if p.zram <= p.vram {
        return Err(OrderError::ZramNotAboveVram);
    }
    if p.vram <= p.vhdx {
        return Err(OrderError::VramNotAboveVhdx);
    }
    Ok(())
}
```

There is no deeply nested if/else logic or sorting algorithm within this file.

### 9. ROLLBACK
Rollback trigger: If the documentation check (`./scripts/docs-check.sh`) fails or `cargo test` reports any new failures related to the documentation inventory changes.

### 10. PR_BOUNDARY
The PR will strictly target `jules/inbox` and remain unmerged ("do not merge"). The PR body will contain `## Resumo`, `## Commits`, `## Labels`, `## Validacao`, and `## Rollback trigger` sections.
