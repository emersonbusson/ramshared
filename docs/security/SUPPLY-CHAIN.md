# Supply Chain Security Policy

This document outlines the supply chain security policy for Cargo dependencies in the RamShared project.

## Cargo-Deny Configuration Rationale

The `cargo-deny` tool is used to enforce our supply chain security policy. The configuration is defined in `deny.toml`.
- **Advisories**: We fail on any RustSec vulnerability.
- **Licenses**: We enforce a strict allowlist of licenses.
- **Bans**: We forbid duplicate dependency versions and wildcards.
- **Sources**: We only allow dependencies from crates.io.

## Approved License List

The following licenses are approved for use in RamShared dependencies:
- MIT
- Apache-2.0
- BSD-3-Clause
- ISC
- Unicode-3.0
- Zlib

## Advisory Database Update Schedule

The vulnerability advisory database is synchronized automatically before every CI build.
