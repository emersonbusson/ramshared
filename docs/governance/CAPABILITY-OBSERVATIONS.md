# Capability observations

`capability-observations.generated.json` is a deterministic, passive view of
the RamShared SSDV3 folders and of real repository paths explicitly referenced
by their PRD, SPEC, or IMPL documents. It exists to make coverage and missing
links reviewable without turning documentation discovery into a release claim.

## Authority boundary

Every catalog row has `observation_state: "OBSERVED"`, including a row that
reconciles to a `DONE` entry in `claims.json`. An observation is not a claim,
does not validate implementation, does not validate live hardware behavior,
and cannot promote any capability.

Only [`claims.json`](claims.json), through the existing documentation-governance
checks and required evidence, may publish a qualified state. A missing claim is
reported as an observed surface, never inferred as `DONE`, `PARTIAL`, or
`BLOCKED`.

## Scope and safety

The policy allows only direct folders in `docs/specs/no-milestone/`, bounded
document reads, safe non-symlink files, and explicit repository prefixes. A
path merely mentioned in prose is catalogued only when it is a real regular
file under an allowed prefix. Parent traversal, absolute paths, symlinks, and
unbounded discovery are refused.

This is a repository metadata generator. It does not execute a benchmark,
start a daemon, change a driver, access a VM, or alter host storage.

## Commands

```bash
node tools/ci/generate-capability-observations.mjs --write
node tools/ci/generate-capability-observations.mjs --check
```

Use `--write` only when deliberately refreshing the tracked generated catalog.
`--check` is read-only and fails when the catalog differs from its deterministic
input. The policy is at
[`capability-observation-policy.json`](capability-observation-policy.json).
