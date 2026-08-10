# Documentation parity

This matrix routes each documentation question to one RamShared source. It is
an ownership map, not a second copy of the underlying requirements.

| Objective | Canonical source | Owner role | Evidence source | State semantics | Known limitation |
| --- | --- | --- | --- | --- | --- |
| Architecture and topology | `ARCHITECTURE.md` | architecture | `docs/decisions/` | Describes ownership; does not prove runtime behavior | Hardware-specific details remain in their owning SPEC |
| Capability state | `docs/governance/claims.json` | documentation-governance | `validation.md` | `DONE` requires qualified evidence; otherwise `UNQUALIFIED`, `PARTIAL`, or `BLOCKED` | Legacy claims require explicit migration |
| PRD and SPEC requirements | `docs/SSDV3-PROMPTS.md` | feature-owner | `docs/specs/` | PRD/SPEC stage is not implementation proof | Feature folders vary in historical format |
| Operation | `docs/runbooks/` | operator-safety | `scripts/safety/` | Runbooks describe supported procedures | Platform preconditions remain mandatory |
| Empirical validation | `validation.md` | reliability | Slice evidence under `docs/specs/` | Append-only measured state with explicit verdict | Old entries use legacy schemas |
| Benchmark comparison | `docs/BENCHMARKS.md` | performance | `docs/benchmarks/` | New PASS claims require a qualified v1 record | Historical entries are legacy-unqualified |
| Reliability gaps | `docs/reliability/GAP-REGISTER.md` | reliability | Owning SPEC and validation evidence | Open gates stay `PARTIAL`, `DEFERRED`, or `BLOCKED` | External environments can remain unavailable |
| Postmortem closure | `docs/postmortems/` | reliability | Regression drill and validation record | Closure requires measured effectiveness | Historical postmortems predate schema 1 |
