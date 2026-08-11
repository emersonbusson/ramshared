# Documentation parity

This matrix routes each documentation question to one RamShared source. It is
an ownership map, not a second copy of the underlying requirements.

| Objective | Canonical source | Owner role | Evidence source | State semantics | Known limitation |
| --- | --- | --- | --- | --- | --- |
| Architecture and topology | `ARCHITECTURE.md` | architecture | `docs/decisions/` | Describes ownership; does not prove runtime behavior | Hardware-specific details remain in their owning SPEC |
| Capability state | `docs/governance/claims.json` | documentation-governance | `validation.md` | `DONE` requires qualified evidence; otherwise `UNQUALIFIED`, `PARTIAL`, or `BLOCKED` | Legacy claims require explicit migration |
| PRD and SPEC requirements | `docs/SSDV3-PROMPTS.md` | feature-owner | `docs/specs/` | PRD/SPEC stage is not implementation proof | Feature folders vary in historical format |
| Operation | `docs/runbooks/README.md` | operator-safety | `scripts/safety/` | Runbooks describe supported procedures | Platform preconditions remain mandatory |
| Empirical validation | `validation.md` | reliability | Slice evidence under `docs/specs/` | Append-only measured state with explicit verdict | Old entries use legacy schemas |
| Task and evidence timestamps | `TASK.md` and `docs/governance/RECORD-SCHEMAS.md` | delivery-governance | `validation.md` | Task changes advance a timestamp; evidence is append-only | Legacy validation entries remain deliberately legacy |
| Document lifecycle coverage | `docs/governance/document-lifecycle-policy.json` | documentation-governance | `docs/reference/DOCUMENTATION-INVENTORY.json` | Classification is coverage only, never technical verification | Freshness belongs only to reviewable documentation |
| Passive capability coverage | `docs/governance/capability-observation-policy.json` | documentation-governance | `docs/governance/capability-observations.generated.json` | Every row is observed; only `claims.json` can publish a qualified state | Discovery cannot prove code execution or live hardware behavior |
| Campaign evidence custody | `docs/governance/campaign-evidence-lifecycle.json` | reliability-evidence | `docs/governance/campaign-evidence-catalog.generated.json` | Historical observations are unqualified; a complete manifest still needs native proof | Static checks do not run a lab or authenticate an author |
| Governance threat model | `docs/security/THREAT-MODEL.md` | security | `docs/decisions/ADR-0008-evidence-and-document-lifecycle.md` | Defines custody and authority boundaries; it is not an operating procedure | Surface-specific threat analysis remains with the owning SSDV3 slice |
| Workstation-space recovery | `docs/runbooks/WORKSTATION-SPACE-RECOVERY.md` | operator-safety | `docs/governance/space-cleanup-receipts.jsonl` | Historical receipts never authorize a new cleanup | VM, VHD, swap, volumes, source and protected evidence are excluded |
| Benchmark comparison | `docs/BENCHMARKS.md` | performance | `docs/benchmarks/` | New PASS claims require a qualified v1 record | Historical entries are legacy-unqualified |
| Reliability gaps | `docs/reliability/GAP-REGISTER.md` | reliability | Owning SPEC and validation evidence | Open gates stay `PARTIAL`, `DEFERRED`, or `BLOCKED` | External environments can remain unavailable |
| Postmortem closure | `docs/postmortems/` | reliability | Regression drill and validation record | Closure requires measured effectiveness | Historical postmortems predate schema 1 |
