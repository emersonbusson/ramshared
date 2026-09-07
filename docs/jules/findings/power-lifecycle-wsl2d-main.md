# Finding Report: WSL2 VM suspend detection and state preservation

## Objective
Detect WSL2 VM pause events and preserve volatile block device state cleanly in `crates/ramshared-wsl2d/src/main.rs`.

## Analysis
Implementing ACPI S3/S4 sleep/wake detection in `crates/ramshared-wsl2d/src/main.rs` is an architectural scope trap. This daemon operates in WSL2 user-space. Within WSL2, Hyper-V completely abstracts and hides host ACPI events (like suspend/resume) from the guest Linux kernel and user-space processes. Therefore, standard signal handling or ACPI event interception cannot be used to synchronously flush block device states before the host goes to sleep.

## Conclusion
This feature request is invalid for WSL2 user-space. The daemon cannot intercept system freeze or suspend signals, as these are inherently invisible to the guest VM. We must produce this FINDING_ONLY report to document the limitation without modifying the code.
