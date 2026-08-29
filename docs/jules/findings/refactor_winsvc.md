# FINDING_ONLY: WinDriveConfig validation logic not in scope

## Issue

The instruction mandates the following goal: "Decompose complex validation logic for Windows service config settings into early-return validators."

The instruction strictly confines the scope to: "crates/ramshared-winsvc/src/main.rs and its related test module."

## Finding

An architectural check of the codebase reveals that the validation logic for `WinDriveConfig` does not reside in `crates/ramshared-winsvc/src/main.rs`. Instead, it resides entirely within `crates/ramshared-winsvc/src/config.rs` under the `pub fn validate(&self) -> Result<(), ConfigError>` method.

Because the instructions explicitly state:
- "Scope is strictly confined to: crates/ramshared-winsvc/src/main.rs and its related test module."
- "If safe code is not possible, produce FINDING_ONLY with evidence in docs/jules/findings/."

We cannot safely perform this refactoring. Modifying `config.rs` directly violates the strict scope confinement. Adding hallucinated validation code to `main.rs` that duplicates or bypasses `config.rs` is architecturally unsound and unsafe.

## Evidence

Running `grep -n "pub fn validate" -A 10 crates/ramshared-winsvc/src/config.rs` shows the validation logic:

```rust
112:    pub fn validate(&self) -> Result<(), ConfigError> {
113-        if self.block_size != 512 && self.block_size != 4096 {
114-            return Err(ConfigError::Invalid {
115-                field: "block_size",
116-                detail: format!("must be 512 or 4096, got {}", self.block_size),
117-            });
118-        }
119-        if self.size_bytes < MIN_SIZE_BYTES {
120-            return Err(ConfigError::Invalid {
121-                field: "size_bytes",
122-                detail: format!("must be >= {MIN_SIZE_BYTES} (64 MiB)"),
...
```

No implementation of `WinDriveConfig` or its validation exists in `crates/ramshared-winsvc/src/main.rs`.

## Conclusion

Safe code modification within the strictly confined scope is impossible.
