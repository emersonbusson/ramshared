# AUDIT-2.5 — Revocable VRAM cache with authoritative SSD origin

## Current boundary — disabled staging only

This audit evaluates source/static design only. It authorizes no VHDX/device,
GPU, NBD/swap, WSL, VM, formatting, or pressure action; live evidence remains
`PARTIAL` until a separately approved attended qualification.

## Findings

| Sev | SPEC § | Issue | Required fix |
| --- | --- | --- | --- |
| High | DT-3 | A write-through label is insufficient unless acknowledgement ordering includes the origin durability operation. | DT-3 now orders origin write and flush before cache work. |
| Critical | DT-3 | The initial audit did not define full positional writes, zero-progress, sync failure, or cache validity after durability failure. | `OriginStorage` now has exact full-write/sync semantics and three named failure/legitimate tests. |
| High | DT-5 | Chunk-valid alone would expose uninitialized bytes after partial writes. | DT-2/DT-5 require block-granular validity and generation. |
| High | DT-8 | VHDX path, disk number, or size could select the existing WSL swap. | DT-8 requires canonical PARTUUID, rejects `SANITIZED_EXISTING_WSL_SWAP_DEVICE`, and pairs refusal with legitimate proof. |
| High | rollback | Old swapoff-before-VRAM-free rule conflicts with clean cache revocation. | Atomicity distinguishes cache release from origin detach; only detach remains swapoff-first. |
| Medium | DT-6 | “Adaptive” growth could burst the whole logical capacity. | Three-sample and one-chunk/two-second bounds are explicit and tested. |
| Medium | telemetry | Physical GPU use could be mislabeled product data. | DT-10 and observability separate logical, cache, headroom, origin, and fallback swap. |
| High | DT-11 | Initial schema text did not make origin failure or late cache release red and gave no recovery rule. | Exact origin/cache enums, sticky recovery, target deadline, status precedence, and named false-green test are required. |
| Medium | traceability | NFRs and exact coverage commands were absent. | NFR rows and the canonical per-file coverage command are now explicit. |
| Critical | DT-4 | The NBD request loop shared synchronous CUDA/DXG calls with authoritative origin serving, so a driver hang could stop swap progress. | Origin serving is isolated from cache work; bounded timeout/disconnect/fault tests must prove immediate origin fallback. |
| Critical | DT-8/DT-14 | Path+PARTUUID+size did not bind the opened FD or exclude dynamically discovered root/swap parents, and normal `up` could run `mkswap`. | Seal manifest+FD+PARTUUID+PTUUID+`dev_t`+swap UUID; remove all formatting from normal startup. |
| Critical | lifecycle | A default systemd stop could kill the backend while its NBD device was still active swap. | Split controller/backend ownership and forbid backend termination until exact swapoff proof; keep host recovery fail-closed. |
| High | DT-3 | `sync_data` on every write was undocumented write amplification and did not implement NBD FUA as a distinct contract. | Implement dirty batching plus explicit FLUSH/FUA durability and failure tests. |
| High | DT-13 | `MCL_FUTURE` was armed before later cache mappings. | Initialize GPU first and lock only current mappings; test call ordering. |
| Critical | DT-19 | An NBD attach may take effect before `nbd-client` reports timeout; terminating the backend from the command error can strand a live kernel device. | Reconcile two stable global snapshots plus exact target PID/size/holder/swap/dev_t status. Preserve backend and seal evidence on effect or ambiguity; terminate only after repeated absence proof. |
| High | DT-18 | Path validation followed by a later external-tool open leaves a retarget window. | Hold exact partition/parent/managed-device FDs, use process-FD paths where supported, revalidate before/after, and document the canonical-name ABI residual honestly. |
| High | DT-20 | `zramctl --find` may return success with malformed output after allocating a device. | Reconcile the exact one-device delta, prove inactivity, reset while FD-bound, and require final full-set equality; ambiguity preserves evidence. |

## Open questions

No source-blocking questions. Real PARTUUID, fixed VHDX creation receipt, WDDM
behavior, and 1–24 GiB live matrix remain environment-bound rollout evidence.

## Verdict

**go** only for the smallest source/static correction. Activation is **no-go**
until the new unit/static gates pass and a separately approved host validation
proves swapoff-first teardown, strong opened-FD identity, cache-hang isolation,
and terminal `PRODUCT_OFF`. Source implementation is also **no-go** if normal
startup can format, the backend can be killed before proved swapoff, unknown NBD
flags mutate state, or synchronous GPU work remains on the origin request path.
