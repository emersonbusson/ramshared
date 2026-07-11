# IMPL — cascade-transport-policy

> Passo 3 SSDV3. Implements [`SPEC.md`](SPEC.md). AUDIT-2.5: **GO** (NBD Day-1).  
> **Date:** 2026-07-10  
> **Status:** **GREEN** on product WSL2 (custom kernel 6.18.35.2 + cascade boot unit).

## Status gates

| Gate | Result | Evidence |
| --- | --- | --- |
| V1 priority log | **GREEN** | `[up] prioridade: zram(200) > VRAM/nbd(100) > VHDX` |
| V2 zram 200 + nbd 100 | **GREEN** | `swapon --show` live |
| V3 down clean | **GREEN** | prior cascade smoke; unit keeps managed path |
| V4 auto → nbd on WSL2 | **GREEN** | log + no ublk product path |
| V5 unit enabled | **GREEN** | `systemctl is-enabled ramshared-cascade` = enabled |
| ublk fail-closed | **GREEN** | explicit `--transport ublk` errors before mutate |
| cargo test -p ramshared-cli | **GREEN** | see validation entry |

## RF / ITEM → files

| ID | Files |
| --- | --- |
| ITEM-1 priorities | `crates/ramshared-cli/src/cascade.rs` (`up` log); `ramshared-tier` defaults |
| ITEM-2 boot | `scripts/safety/install-cascade-boot.sh --enable`; unit already in wsl2-cascade-boot |
| ITEM-3 transport auto | `cascade.rs`: `Transport::Auto`, `is_wsl2`, `resolve_transport` |
| ITEM-4 idempotent | existing `cascade_already_healthy` (unchanged contract) |

## Small decisions

1. **Default transport = Auto**, not Nbd, so off-WSL2 can prefer ublk later without flag flip.  
2. **ublk check runs before idempotent `up`** so explicit ublk on WSL2 never returns “already healthy”.  
3. **Did not** implement full ublk wire in `up` (SPEC out of scope; AUDIT NO-GO).  
4. Kernel `ublk_drv` live is **capability**, not product path.

## Validation numbers (this host, 2026-07-10)

| Metric | Value |
| --- | --- |
| uname | 6.18.35.2-microsoft-standard-WSL2+ |
| swap order | zram0 prio **200**, nbd0 prio **100**, sdc prio **−2** |
| sizes | zram 1024M, nbd 1024M, sdc 8G |
| unit | enabled + active (exited), daemon `ramsharedd --nbd /dev/nbd0` |
| ublk kernel | `ublk_drv` loaded; `/dev/ublk-control` present; **not** used by cascade |

## Gaps

| Gap | Class |
| --- | --- |
| Soak reboot 2× after enable | **Hygiene** — human/lab; no new SPEC (covers existing boot SPEC) |
| Full ublk `up` wire | Future SPEC + AUDIT-2.5 |
| Pressure thrash | Host-unsafe on live WSL2 — only qemu/civm |

## Rollback trigger

- Ghost nbd/ublk in `/proc/swaps` after boot → `ramshared down` then investigate; if unit loops, `uninstall-cascade-boot.sh`.  
- Host freeze after any ublk experiment → never re-enable product ublk; keep NBD.

## Traceability

PRD RF-T1..T5 → SPEC ITEM-1..4 → this IMPL.  
Related: `wsl2-cascade-boot` (unit), `wsl2-custom-kernel-p1` (ublk module available).
