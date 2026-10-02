# AUDIT-2.5 — wsl2-cascade-boot

> Re-issued 2026-09-30 against `SPEC.md` revision 2 (the PRD-revision-2
> rewrite). The previous audit targeted the retired `scripts/safety/*.sh`
> orchestrator and is superseded. Review for ambiguity, atomicity, rollback
> layering, evidence executability, security flow subversion, Day-0 shims,
> vague verbs, and generic test matrices (`docs/SSDV3-PROMPTS.md` STEP 2.5).

## Findings

| Sev | Finding | Class | Resolution in SPEC |
| --- | --- | --- | --- |
| HIGH | DT-2 drops `nbd-product-preflight.sh` from the boot path on an **assumption** of Rust gate parity. No executable proof was named that the absorbed gates cover what the shell preflight covers (manifest digest, lower-sink identity/capacity, release layout). | static risk | Accepted-boundaries block: parity is maintained by reusing `nbd_readiness::RefusalCode` / `host_gate::*`, and is proved by keeping `scripts/safety/test-nbd-product-preflight.sh` green after the unit change. Not by a second boot-time gate implementation. |
| MED | `boot`'s host-prerequisite gate needed the host-resume lease, but `mint_lease` is the host-gate service's job. The `boot.rs` type list named no lease **verifier**, so the unit's removed `ConditionPathExists=/run/ramshared/host-resume-lease.json` left a silent hole (NFR-6 violation: a missing lease would look like a hang). | reproduced defect in SPEC draft | Added `verify_host_lease(path, now)` to `boot.rs` — reads the lease and rejects on `host_gate::lease_expired`. `boot` verifies, never mints. |
| MED | A token at `/var/lib/ramshared/approvals/` was accepted on content alone. A world-writable token is a privilege bypass: any user could mint `activate:<running>`. | reproduced defect in SPEC draft | New security-checklist item + `approval_token_is_root_owned` + matrix row `boot_refuses_untrusted_approval_token` (#13 pair with `scoped_approval_accepts_only_the_running_release_version`). `ApprovalUntrusted` added to `BootError`. |
| MED | RF-10 says "no **user-visible** surface" may print green, but the matrix named only TUI tests. `status` plain text and `status --json` are both user-visible. | reproduced defect in SPEC draft | `lifecycle.rs` added as MODIFY with two named tests: `status_text_reports_off_and_blocked_without_live_daemon`, `status_json_never_publishes_active_without_live_daemon`. |
| LOW | DT-5 keeps `TimeoutStopSec=infinity`, stronger than NFR-2's "for example, 600 s". A stop that never returns is not a "refusal", so NFR-6's visibility contract did not cover it. | static risk | `ExecStop` journals `NBD_STOP_WINDOW=unbounded` on entry so a long swapoff is expected rather than mysterious. |
| LOW | Attended `sudo ramshared up` intentionally bypasses the boot gates. Without an explicit statement, a later change might "fix" this by retrofitting the token onto `up` and break the attended path. | incorrect-conclusion risk if left implicit | Named under **Accepted boundaries**: do not retrofit. |

## Open questions

- **WSL with `systemd=false`.** The unit never runs. Documented in `PRD.md` §9;
  RF-6 documentation must state that boot enable requires `systemd=true` in
  `/etc/wsl.conf`. Not blocking — it is a stated prerequisite, not a defect.
- **Stop-window infinity on a wedged swapoff.** If `swapoff` itself hangs on a
  foreign driver stall, the unit stays in `deactivating` and the operator has
  no automatic recovery. This is the honest trade against `kill -9` creating
  ghost swap (2026-09-23). Recovery stays attended (`wsl --shutdown`), per
  `PRD.md` §2.1. Not blocking.

## Verdict

**go** — implement SPEC as written. The four medium findings are already folded
into `SPEC.md` (types, security checklist, and test matrix) before this verdict;
nothing in this audit requires a third revision. Residual risk is the two open
questions above, both of which are documented behaviour rather than unknown
behaviour.

Classification note (repository rule): findings 2–4 are **reproduced defects in
the SPEC draft**, found by reading the draft against the PRD and the security
checklist, and fixed before implementation — not bugs in shipping code.
Findings 1, 5 and 6 are **static risks / implicit assumptions**, resolved by
making the contract explicit.
