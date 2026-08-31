# FINDING_ONLY: validate PCIe BAR aperture size before mapping dxg memory

## Architectural Constraint
The WDDM/DXG UAPI integration in `crates/ramshared-dxg/src/lib.rs` strictly mirrors the WSL 6.18 `d3dkmthk.h` layouts. It does not contain PCIe BAR aperture size fields (`bar0_size`, etc.) nor does it perform raw memory mapping.

## Conclusion
Implementing PCIe BAR size validation here would require inventing non-existent UAPI fields, violating the architectural constraint. Therefore, no code changes can be safely made in this crate.
# Findings
This directory contains architectural and validation findings from autonomous analysis loops, specifically those detailing non-actionable requirements or systemic constraints (`FINDING_ONLY`).
