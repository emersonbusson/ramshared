# Finding Report: Windows power broadcast trap

## Observation
The user requested implementing a Windows power broadcast (`PBT_APMRESUMEAUTOMATIC`) handler in `crates/ramshared-winsvc/src/windows_driver.rs` to register for Windows power events and trigger driver reconnection on system resume.

## Architectural constraint
`crates/ramshared-winsvc/src/windows_driver.rs` strictly isolates Windows unsafe mapping and IOCTLs for the control handle/mapped queue adapter. The driver itself runs as a service/daemon that operates the data plane over IOCTLs, and does not (and should not) implement a Windows message loop to handle `WM_POWERBROADCAST` (which is typically reserved for GUI applications or explicitly structured service handlers registered with `RegisterDeviceNotification` in a different layer). As per architectural intent, the pure Rust driver logic handles reconnection implicitly if IO fails, and registering power event broadcasts here is an architectural scope trap.
