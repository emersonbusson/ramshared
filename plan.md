1. **Understand the Vulnerability**:
   The `redacted_error` function in `crates/ramshared-winsvc/src/evidence.rs` uses a heuristic approach to redact error details (`looks_like_pointer` and length caps). This is vulnerable to false negatives, which could lead to exposing sensitive information (e.g. API keys, secrets) in the `detail` string, which is then written to the JSONL evidence file.
2. **Strict Allowlist Solution**:
   Instead of trying to heuristically redact the `detail` string, we should strictly enforce that no sensitive information is leaked. Since `error_class` and `error_code` are stable and safe enum-like values or predefined strings, we can just return an empty string for `detail`, thereby guaranteeing no secrets are leaked.
3. **Change to `redacted_error`**:
   Modify `redacted_error` to ignore the `detail` argument completely:
   ```rust
   pub fn redacted_error(class: &str, code: &str, _detail: &str) -> (String, String, String) {
       (class.to_string(), code.to_string(), String::new())
   }
   ```
4. **Update the test `stable_error_redacts_payload`**:
   The test expects `detail.contains("<redacted>")` and `!detail.contains(...)`. We will update it to verify that `detail` is exactly empty.
5. **Run Tests**: Use `cargo test --package ramshared-winsvc` to verify the fix works.
6. **Pre-commit Checks**: Complete pre-commit steps to ensure proper testing, verification, review, and reflection are done.
