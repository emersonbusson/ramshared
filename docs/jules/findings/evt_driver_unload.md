# Finding: Architectural Mismatch for EvtDriverUnload in StorPort Driver

## Task Context
Target: `drivers/windows/ramshared/driver.c`
Objective: Verify driver context existence before releasing global synchronization objects on unload.
Keywords: `EvtDriverUnload context cleanup`, `EvtDriverUnload`.

## Investigation
I searched the `drivers/windows/ramshared/` directory and the `driver.c` file for `EvtDriverUnload` or `DriverUnload` routines.
Neither `EvtDriverUnload` nor `DriverUnload` exist in this driver.

The RamShared Windows driver is a **StorPort virtual miniport**.
As specified in `drivers/windows/ramshared/driver.c`, it initializes by calling `StorPortInitialize` and sets up `HW_INITIALIZATION_DATA` hooks (e.g. `HwInitialize`, `HwStartIo`, `HwAdapterControl`, `HwFreeAdapterResources`).
It does not expose a standard WDM `DriverUnload` (i.e. `DriverObject->DriverUnload`) nor a WDF `EvtDriverUnload` because StorPort manages the lifetime of the miniport driver and its resources.
Teardown in a StorPort virtual miniport is handled primarily through `HwFreeAdapterResources` and other adapter control callbacks.

## Conclusion
Since the driver architecture relies on the StorPort model rather than raw WDM or WDF, `EvtDriverUnload` is structurally non-existent. A safe orthogonal slice modifying this nonexistent function is impossible.
Therefore, in accordance with the Immutable Contract item 4: "If safe code modification is not possible, produce FINDING_ONLY with evidence in docs/jules/findings/."
