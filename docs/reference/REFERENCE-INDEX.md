# RamShared reference index

Use this router to start at the owning document and then follow deeper links.

| Question | Canonical source | Next-depth references | Boundary |
| --- | --- | --- | --- |
| How do I build and test the repository? | `README.md` | `.claude/rules/coding.md` | Platform-specific tools still apply |
| How do I operate the Linux or WSL2 lifecycle? | `docs/runbooks/` | `docs/reliability/DEGRADATION-MATRIX.md` | Shared-host pressure requires the approved watchdog |
| How do I operate the Windows lifecycle? | `docs/runbooks/windows-autonomous-broker.md` | `docs/specs/no-milestone/` | Driver changes require Windows-native gates |
| How do I run a host-safety campaign? | `scripts/safety/` | `.claude/rules/benchmarks.md` | Never substitute unsupervised pressure |
| How do I register or compare a benchmark? | `docs/BENCHMARKS.md` | `docs/benchmarks/evidence.schema.json` | Legacy-unqualified data cannot promote |
| Which reliability gaps remain open? | `docs/reliability/GAP-REGISTER.md` | `docs/reliability/DEGRADATION-MATRIX.md` | Open evidence is not DONE |
| Where are architecture decisions recorded? | `ARCHITECTURE.md` | `docs/decisions/` | Durable decisions belong in ADRs |
| How do I make an SSDV3 change? | `docs/SSDV3-PROMPTS.md` | `.claude/rules/ssdv3.md` | Live evidence must match the changed surface |
| How do I investigate and close an incident? | `docs/postmortems/TEMPLATE.md` | `docs/reliability/BLACK-BOX-FORENSICS.md` | Closure requires regression and refusal proof |
