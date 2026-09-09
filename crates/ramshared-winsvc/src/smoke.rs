//! Post-boot smoke checks (SPEC ITEM-7 / RF-6 flow 6).
//!
//! Detects ImDisk-style regression (volume/pagefile missing after update) and
//! signals graceful feature disable.

/// Outcome of post-boot smoke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmokeResult {
    /// Disk enumerated and pagefile present on VRAM volume.
    Ok,
    /// Feature should degrade (log + disable pagefile path).
    Degrade { check: &'static str, detail: String },
}

/// Inputs observed on the host (injected for tests; WMI on Windows).
#[derive(Clone, Debug, Default)]
pub struct SmokeInputs {
    pub disk_enumerated: bool,
    pub pagefile_active_on_vram: bool,
    pub vram_volume_present: bool,
}

/// Run smoke checks. Pure function — no I/O.
pub fn post_boot_smoke(inputs: &SmokeInputs) -> SmokeResult {
    if !inputs.vram_volume_present {
        return SmokeResult::Degrade {
            check: "volume",
            detail: "VRAM volume not present after boot".into(),
        };
    }
    if !inputs.disk_enumerated {
        return SmokeResult::Degrade {
            check: "disk",
            detail: "virtual disk not enumerated".into(),
        };
    }
    if !inputs.pagefile_active_on_vram {
        return SmokeResult::Degrade {
            check: "pagefile",
            detail: "pagefile.sys not active on VRAM volume".into(),
        };
    }
    SmokeResult::Ok
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn test_smoke_all_present_ok() {
        assert_eq!(
            post_boot_smoke(&SmokeInputs {
                disk_enumerated: true,
                pagefile_active_on_vram: true,
                vram_volume_present: true,
            }),
            SmokeResult::Ok
        );
    }

    #[test]
    fn test_smoke_missing_vram_volume_degrade() {
        let r = post_boot_smoke(&SmokeInputs {
            disk_enumerated: true,
            pagefile_active_on_vram: true,
            vram_volume_present: false,
        });
        assert_eq!(
            r,
            SmokeResult::Degrade {
                check: "volume",
                detail: "VRAM volume not present after boot".into(),
            }
        );
    }

    #[test]
    fn test_smoke_missing_disk_degrade() {
        let r = post_boot_smoke(&SmokeInputs {
            disk_enumerated: false,
            pagefile_active_on_vram: true,
            vram_volume_present: true,
        });
        assert_eq!(
            r,
            SmokeResult::Degrade {
                check: "disk",
                detail: "virtual disk not enumerated".into(),
            }
        );
    }

    #[test]
    fn test_smoke_missing_pagefile_degrade() {
        let r = post_boot_smoke(&SmokeInputs {
            disk_enumerated: true,
            pagefile_active_on_vram: false,
            vram_volume_present: true,
        });
        assert_eq!(
            r,
            SmokeResult::Degrade {
                check: "pagefile",
                detail: "pagefile.sys not active on VRAM volume".into(),
            }
        );
    }

    #[test]
    fn test_smoke_all_missing_degrade_volume_first() {
        let r = post_boot_smoke(&SmokeInputs {
            disk_enumerated: false,
            pagefile_active_on_vram: false,
            vram_volume_present: false,
        });
        assert_eq!(
            r,
            SmokeResult::Degrade {
                check: "volume",
                detail: "VRAM volume not present after boot".into(),
            }
        );
    }
}
