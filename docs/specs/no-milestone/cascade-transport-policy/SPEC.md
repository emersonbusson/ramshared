# SPEC — cascade-transport-policy

> Implements [`PRD.md`](PRD.md). Zero creativity out of scope.

## Traceability

| PRD | ITEM |
| --- | --- |
| RF-T1 | ITEM-1 priorities already in cascade.rs — assert logs |
| RF-T2 | ITEM-2 install-cascade-boot --enable |
| RF-T3, RF-T4 | ITEM-3 transport auto / ublk refuse on WSL2 |
| RF-T5 | ITEM-4 existing idempotent up |

## ITEM-1 — Priority order (no code change required)

Keep `TierPriorities::default()`: zram=200, vram=100, disk≈−2.  
`up` must log the order once. Disk tier is pre-existing WSL swap (e.g. `/dev/sdc`).

## ITEM-2 — Boot enable

```bash
sudo bash scripts/safety/install-cascade-boot.sh --enable
```

Requires: systemd, `target/release/ramshared`+`ramsharedd`, `ramshared check` ready, preflight OK.  
Unit: `ramshared-cascade.service` oneshot RemainAfterExit.

## ITEM-3 — transport auto

Default `--transport` / omit = **auto**:

| Environment | Resolved |
| --- | --- |
| WSL2 | **nbd** + stderr reason (ublk freeze policy) |
| non-WSL2 + `/dev/ublk-control` | ublk (future full wire; may still error until implemented) |
| else | nbd |

Explicit `--transport ublk` on WSL2: fail closed in `up` (do not start half-daemon).

## ITEM-4 — Idempotent up

Unchanged: if cascade healthy, return status without re-setup.

## Validation

| V | Check |
| --- | --- |
| V1 | up logs priority zram > VRAM > VHDX |
| V2 | up creates zram prio 200 and nbd prio 100 |
| V3 | down leaves only non-managed disk swap |
| V4 | transport auto on WSL2 does not attempt ublk product path |
| V5 | systemd unit enabled after install --enable |

## Kahneman

| # | Rule |
| --- | --- |
| #16 | ublk not Day-1 on WSL2 |
| #18 | Fix freeze at daemon teardown layer before enabling product ublk |
| #17 | up/down idempotent |

## Out of SPEC

Implementing full ublk `up` wire on WSL2; soak thrash tests.