# ADR-0008 — Evidence and document lifecycle is explicit and fail-closed

## Status

Accepted

## Date

2026-08-11

## Scope

Repository governance records, read-only checkers, and laboratory evidence
custody; no host, guest, driver, GPU, or kernel operation.

## Context

RamShared already keeps architecture decisions, SSDV3 artifacts, benchmark
records, validation entries, and laboratory output. Those records have
different ages and producers. Without an explicit lifecycle, a reader can
mistake a historical artifact for a current capability, or a local file for
evidence tied to an exact campaign.

The repository also contains a historical filename collision for ADR-0007.
Rewriting old records would damage provenance, but accepting an unbounded
duplicate numeric sequence would make the ADR registry ambiguous.

## Decision

1. New task, document, claim, campaign, and cleanup records use their
   repository-native schemas with accountable ownership, time semantics,
   source identity, lifecycle, and retention where applicable.
2. `observed_at` and `verified_at` are distinct facts. A verification time
   cannot replace the time an experiment or record was observed.
3. New campaign evidence is manifest-led, bounded, sanitized, and checked
   read-only before it can support a completion claim. Incomplete, failed, or
   blocked runs remain non-promoting evidence.
4. Historical records remain immutable and are cataloged as historical or
   unqualified when their original custody fields cannot be proven. No checker
   upgrades them by inference.
5. The ADR canonical index has one current numeric record per number. The
   existing ADR-0007 filename collision is retained only as an explicit
   historical, noncanonical mapping; it is not precedent for another duplicate.
6. Governance checkers refuse malformed, orphaned, unsafe, or ambiguous
   records. They neither repair records nor execute host or laboratory actions.

## Consequences

### Positive

- A reader can distinguish current, observed, historical, blocked, and
  completed facts without relying on filenames or informal recency.
- Evidence promotion has a minimum custody boundary and does not silently
  traverse symlinks, paths outside the repository, or unbounded artifacts.
- The decision registry becomes machine-checkable while retaining historical
  file bytes and their provenance.

### Costs and limitations

- New records have more required metadata and their producing work must name
  an owner role and retention decision.
- Historical records may remain unqualified indefinitely; preservation is not
  equivalent to proof.
- Repository checks cannot prove that a physical WSL2, Windows, GPU, or kernel
  experiment occurred. Environment-bound evidence remains `PARTIAL` as needed.

## Alternatives considered

| Alternative | Why rejected |
| --- | --- |
| Rewrite historical ADRs and evidence into the new shape | It would blur original provenance and invent missing facts. |
| Permit duplicate ADR numbers when their titles differ | Numeric references would remain ambiguous to humans and tools. |
| Treat file modification time as observation time | Filesystem time is mutable and does not identify the experiment. |
| Automatically clean stale or malformed evidence | A read-only governance tool must not delete records or affect host storage. |
| Require live hardware proof for every documentation change | Documentation checks must not pressure or mutate shared hosts. |

## Kahneman

- **#16 — fail-safe default:** an incomplete or malformed record is non-
  promoting and fails closed.
- **#17 — idempotent effects:** checkers are read-only and repeated execution
  does not change evidence, task state, or host state.
- **#18 — fix in the owning layer:** record integrity belongs to the producer
  and its schema, not to an inferred workaround in an index or CI summary.

## Rollback trigger

Roll back this governance profile through a follow-up ADR if either condition
is observed in a reviewed run: (1) a checker accepts an undeclared duplicate
numeric ADR or a new unmanifested evidence directory, or (2) the required
metadata causes a legitimate existing producer to be blocked for more than 14
days without a bounded schema amendment. Rollback means disabling only the
affected new rule after preserving its refusal evidence; it never means
rewriting historical records or bypassing host-safety gates.
