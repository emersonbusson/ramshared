# SSDV3 Passo 2.5 Audit — WSL2-native VRAM autotier

## Verdict

**GO for ITEM-1 through ITEM-3 and pure ITEM-2 policy. CONDITIONAL GO for live ITEM-4.** Live automatic swapoff/re-enable remains environment-bound until absorption and timeout behavior are proven without pressure on the daily WSL host.

## Findings

- **Critical, fixed in SPEC:** freeing CUDA chunks before swapoff completion would corrupt live swap. SPEC now requires `used_kb == 0` after swapoff.
- **High, fixed in SPEC:** adapter handles are per `/dev/dxg` open. Provider owns the file and closes all handles in reverse order.
- **High, fixed in SPEC:** multi-adapter CUDA↔LUID mapping is unproven. Automatic selection rejects ambiguity.
- **High, fixed in SPEC:** fallback after a live dxg failure could hide host pressure. Fallback is startup-only; later failure fails closed.
- **Critical, fixed in code:** returning EIO after Linux swap selected an NBD slot could lose a page. The accepted write now completes and schedules demote; later commits stop while swapoff is pending.
- **Critical, fixed in code:** failed/timed-out swapoff previously reached `free_all_live()`. Teardown now retries and refuses release until swapoff is confirmed and `used_kb == 0`.
- **High, fixed in code:** demote completion previously depended on another NBD request. The 5-second controller tick now polls completion and WDDM independently.
- **High, fixed in code:** recovery previously existed only as a pure state. The empty tier is re-enabled after 3 healthy WDDM samples, without eager promotion.
- **Medium:** ioctl polling has no kernel timeout owned by RamShared. Controller must treat stale age as invalid; hard cancellation remains an external uAPI limitation.
- **Medium:** WDDM `current_usage` includes the process. Policy subtracts only measured RamShared CUDA commit and saturates; hardware comparison remains required.

## Security checklist

Bounded count/pointer lifetime, no TOCTOU re-read, no unknown flags, no addresses in logs, hot removal produces error, and close/drop is balanced. No new privileged kernel surface is introduced.

## Open gates

1. Prove LUID association for CUDA on multi-adapter WSL before enabling explicit CUDA allocation there.
2. Run 3 isolated live-page demote/recovery trials with median+p99.
3. Verify Windows WDDM view against dxg results on available vendors.

## Abort triggers

Any silent fallback after provider activation, `used_kb > 0` chunk release, unbounded adapter allocation, or claim of HMM/NUMA ownership changes verdict to NO-GO.
