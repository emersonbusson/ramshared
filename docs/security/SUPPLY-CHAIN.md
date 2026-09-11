# Supply Chain Security Policy

This document defines the supply chain security policy for the RamShared workspace. It outlines the rationale behind our `cargo-deny` configuration, the list of approved licenses, and the schedule for updating our advisory database.

## 1. `cargo-deny` Configuration Rationale

Our `cargo-deny` configuration is defined in `deny.toml` at the repository root. It serves as our primary defense-in-depth mechanism to ensure that untrusted external dependencies are rigorously vetted before entering the build pipeline. The configuration adheres to a fail-closed, zero-trust philosophy.

*   **Graph Check**: We enable `all-features = true` to guarantee that all dependencies, including those feature-gated (such as GPU compute backends and specific I/O drivers), are analyzed.
*   **Advisories**: We use version 2 of the advisories check, which automatically rejects any yanked crates from crates.io. We maintain an empty `ignore` list (`ignore = []`); any advisory flags will halt the build and require immediate review and mitigation, rather than being bypassed.
*   **Licenses**: The policy employs a strict allowlist. Dependencies with unapproved licenses will cause the build to fail closed.
*   **Bans**: We set `wildcards = "deny"` to prevent unspecified dependency versions from introducing unexpected changes. `multiple-versions = "warn"` is used to highlight duplicate versions in the dependency graph, allowing for legitimate divergence in transitive dependencies while keeping the surface minimal.
*   **Sources**: Only the official `crates.io` registry is allowed (`allow-registry = ["https://github.com/rust-lang/crates.io-index"]`). Any unknown Git repositories or custom registries are explicitly denied (`unknown-registry = "deny"`, `unknown-git = "deny"`), enforcing strict source provenance.

## 2. Approved License List

Our dependency graph is restricted to the following rigorously vetted, OSI-approved open-source licenses. Any dependency introducing a license outside this list will fail the `cargo-deny` check and must undergo a compliance review.

*   `MIT`
*   `Apache-2.0`
*   `ISC`
*   `Unicode-3.0`
*   `Zlib`

The confidence threshold for license matching is set to `0.8` to ensure accurate automated detection.

## 3. Advisory Database Update Schedule

To maintain a strong defense posture against emerging vulnerabilities, our RustSec advisory database must be kept strictly up-to-date.

*   **Continuous CI Validation**: The `cargo-audit` step in our CI pipeline validates the advisory database. It requires the snapshot to be within the allowed age limit defined by `RUSTSEC_DB_MAX_AGE_DAYS` (configured in `.github/workflows/security-scans.yml` and `docs/governance/ci-contract.json`).
*   **Routine Updates**: The database snapshot configuration (`RUSTSEC_DB_COMMIT` and `RUSTSEC_DB_COMMIT_UTC`) should be updated proactively to track the upstream `main` branch of the RustSec advisory-db.
*   **Incident Response**: In the event of a `cargo-audit` CI failure due to an outdated database, engineers must update the commit hashes and timestamps in the CI contract files to the latest available upstream snapshot.
