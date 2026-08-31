1. **RULES**:
   - Guard Clauses Over Nested IF/ELSE.
   - Physical Sanity Checks.
   - Semantic Error Returns (-EINVAL, -ENODEV, -EBUSY).
   - English only.

2. **MAIN_DIFF**:
   - Update `ramshared_bdev_open` in `queue.c` to use `test_and_set_bit(RAMSHARED_STATE_OPEN, &rs_dev->state_flags)` and return `-EBUSY` if it was already set.
   - Update `ramshared_pci_remove` in `main.c` to use `test_and_set_bit(RAMSHARED_STATE_REMOVING, &rs_dev->state_flags)` and return early if already set, preventing double removals.

3. **FILES**:
   - `drivers/block/ramshared/queue.c`
   - `drivers/block/ramshared/main.c`

4. **INVARIANTS**:
   - No wild pointers, no double-frees, no race conditions on state flags.

5. **COUNTERFACTUAL**:
   - If not implemented with `test_and_set_bit`, concurrent opens could result in the bit being cleared prematurely on release, breaking exclusive access semantics.

6. **RED_TEST**:
   - Fails if multiple concurrent opens don't result in -EBUSY for subsequent callers.

7. **COVERAGE**:
   - Ensures correct exclusive open and safe removal operations.

8. **REAL_PROOF**:
   - Clean compilation and static checks pass.

9. **ROLLBACK**:
   - Revert if device hangs or fails to open exclusively.

10. **PR_BOUNDARY**:
    - "do not merge"
