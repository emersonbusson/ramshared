# Finding: EvtDeviceAdd callback and WDF
The task asks to "Return STATUS_INSUFFICIENT_RESOURCES on context allocation failure and STATUS_DEVICE_CONFIGURATION_ERROR" in "semantic NTSTATUS return mapping in EvtDeviceAdd callback" in `drivers/windows/ramshared/driver.c`.

However, the driver uses StorPort (`HW_INITIALIZATION_DATA`), not WDF/KMDF (`WdfDriverCreate`, `EvtDeviceAdd`).
There is NO `EvtDeviceAdd` callback in `drivers/windows/ramshared/driver.c` or anywhere in the codebase.
The initialization is done in `DriverEntry` mapping to `HwStorFindAdapter` and `HwStorInitialize`.

Since there is no `EvtDeviceAdd` or WDF framework used, the safe code modification is not possible.
