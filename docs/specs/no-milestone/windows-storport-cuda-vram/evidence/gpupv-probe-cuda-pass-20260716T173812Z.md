# GPU-PV real CUDA probe — PASS — 2026-07-16

## Context

After offline UMD copy and repeated protocol negotiation events (`0x10006` request vs
channel `0x10005`), a bounded lab campaign re-checked whether the guest could run **real**
CUDA — not file presence alone.

## Environment

| Field | Value |
| --- | --- |
| Host build | `26200.8655` (25H2) |
| Guest build | `26200.8037` (25H2) |
| Guest `nvidia-smi -L` | `GPU 0: NVIDIA GeForce RTX 2060 (UUID: GPU-1d3109d8-…)` |
| Guest driver | `610.74`, `6144 MiB` |
| Display PnP | Hyper-V Video OK; NVIDIA VEN_1414 OK `prob=0` (+ one Unknown ghost) |
| Host GPU after | RTX 2060 OK; `nvidia-smi` functional |

Protocol events 33101/33100 still appear on boot (request `0x10006`, negotiate `0x10005`) but
do not block nvidia-smi or `nvcuda` load on this host/guest pair.

## probe-cuda

Binary: `C:\ramshared\bin\ramshared-winsvc.exe` (SHA `F129B25F…` already on host bin).  
Lab-only VC++ side-by-side: `VCRUNTIME140.dll`, `VCRUNTIME140_1.dll`, `MSVCP140.dll` next to the
exe (guest initially failed with `STATUS_DLL_NOT_FOUND` / `0xC0000135` without them).

Config: `C:\ProgramData\RamShared\probe-cuda.toml` — 64 MiB size, `reserve_bytes=134217728`,
`broker=127.0.0.1:19876`, `tenant=guest-probe` (broker not required for DeviceMem-only probe path).

```text
EXIT=0
probe-cuda: device=0 name=NVIDIA GeForce RTX 2060 size=67108864
  free_before=5360320512 free_after=5360320512
  offsets=[0, 33554432, 67104768]
probe-cuda: PASS
BEFORE_USED_MIB=1596 AFTER_USED_MIB=1596  (guest nvidia-smi memory.used restored)
```

No product Online, no NTFS format, no pagefile, no thrash. Final: `win11-drill` Off, host GPU OK.

## Residual gaps

| Gap | Status |
| --- | --- |
| Guest UBR lag (`8037` vs host `8655`) | Open — Windows Update on guest still recommended |
| Virtual PCI protocol mismatch events | Observed; non-blocking for this smoke |
| Product Online + 3-round storage SHA on guest | Open (next) |
| Physical host BINARY_MATCH / Online | **BLOCKED** by README daily-host policy + SHA mismatch |
| InfVerif DIRID 13 (ERROR 1322) | Open — Universal INF attestation isolation |
