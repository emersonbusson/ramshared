# Hang / freeze audit — 2026-07-13

**Discipline:** Kahneman #13 (exists ≠ works), #16 (fail-safe), #18 (right layer).  
**Superprompt:** [`superprompt.md`](../../superprompt.md).  
**SSDV3 Step 3:** live E2E + cover ≥80% required — [`docs/SSDV3-PROMPTS.md`](../SSDV3-PROMPTS.md).

## Scope

Perceived freezes on daily WSL2 (Docker builds, guest “freeze”) vs RamShared hang-class bugs (ghost swap, teardown, deleted daemon inode).

## Measured facts (session)

| Item | Value |
| --- | --- |
| Guest mem cap | 16 GiB (`.wslconfig`) |
| Cascade after redeploy | zram 2G prio 200, nbd0 4G prio 100, sdc 8G prio -2; used=0 |
| Daemon | new PID, `BINARY_MATCH=OK` → `target/release/ramsharedd` |
| cascade-health | `ok:true`, `ghost:false`, `order_ok:true` |
| Ollama | residual 203/EXEC **removed** (ghost unit) |
| Docker images/cache | cleaned; Advoq stack down until rebuild |
| Go/Rust caches | cleaned; toolchains go1.26.5 / rustc 1.97.0 |

## Hang classes — status

| Class | Severity | State | Evidence / mitigation |
| --- | --- | --- | --- |
| Ghost ublk/nbd after daemon kill | CRITICAL | **Mitigated in code** (`cascade` refuse/recover) | Postmortem 2026-07-09; parse_proc_swaps tests |
| Free sparse with used_kb≠0 | HIGH | **Mitigated** (WDDM Phase 1 MEMORY 2026-07-11) | Teardown retries until used_kb==0 |
| WDDM commit refuse without write fallback | HIGH | **Mitigated** (EIO → bounded swapoff) | MEMORY 2026-07-11 |
| Daemon deleted inode vs disk binary | HIGH | **Closed this session** | Rebuild + restart; BINARY_MATCH=OK |
| Postmortem “CRASH” from Call Trace / memcg OOM / unit spam | MEDIUM | **Mitigated** (postmortem.sh classifies kernel vs OOM; no bare Call Trace) | #13 |
| Docker postgres memcg OOM | MEDIUM | **Environmental** (not cascade) | Container OOM; not ghost swap |
| BuildKit hang (web image build) | MEDIUM | **Environmental / Docker path** | Not clean guest OOM; full rebuild without cache |
| Pressure probe on daily WSL | HIGH if repeated | **Policy** — forbidden on live host | MEMORY 2026-07-11 audit |

## Open gaps (do not fake green)

1. **`cascade_io` unit cover** remains low by design — closed via live E2E (health + BINARY_MATCH), not thrash mocks on the daily host.
2. **ITEM-8 / StorPort INF** — LUN in Get-Disk still env-bound lab.
3. **Destructive demote/pressure drill** — isolated VM only; do not re-run on daily Ubuntu.
4. **Named Docker volumes** (~6G) and Advoq data — not deleted when images were pruned.
5. **I:\\** still high utilization — watch swap VHDX; not a cascade logic bug.

## SSDV3 gate to claim hang-class safe

Before declaring “cascade safe” on any PR:

- [ ] `cargo test` on touched crates
- [ ] cover ≥80% on slice business-logic files/crates (not monorepo average)
- [ ] `BINARY_MATCH=OK` + `cascade-health ok:true`
- [ ] ≥1 refusal (#13): ghost or used_kb>0 if the path touches teardown/up
- [ ] `validation.md` entry with numbers

## Rollback trigger (this audit)

If `cascade-health` is `ok:false` or `ghost:true` or BINARY_MATCH fails after boot → stop heavy workloads, `systemctl stop ramshared-cascade` (swapoff-first), reassess before `up`.
