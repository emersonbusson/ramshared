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

    fn inputs(disk: bool, pagefile: bool, volume: bool) -> SmokeInputs {
        SmokeInputs {
            disk_enumerated: disk,
            pagefile_active_on_vram: pagefile,
            vram_volume_present: volume,
        }
    }

    fn degrade_check(r: &SmokeResult) -> &'static str {
        match r {
            SmokeResult::Ok => "Ok",
            SmokeResult::Degrade { check, .. } => check,
        }
    }

    #[test]
    fn all_good() {
        assert_eq!(post_boot_smoke(&inputs(true, true, true)), SmokeResult::Ok);
    }

    /// The three degrade reasons are checked in order, and each one names the
    /// specific check that failed so the operator knows what to repair.
    #[test]
    fn every_missing_input_degrades_with_its_own_check_name() {
        assert_eq!(
            degrade_check(&post_boot_smoke(&inputs(false, false, false))),
            "volume"
        );
        assert_eq!(
            degrade_check(&post_boot_smoke(&inputs(false, false, true))),
            "disk"
        );
        assert_eq!(
            degrade_check(&post_boot_smoke(&inputs(true, false, true))),
            "pagefile"
        );
    }

    #[test]
    fn degrade_detail_describes_the_missing_artifact() {
        let r = post_boot_smoke(&inputs(false, false, false));
        let SmokeResult::Degrade { detail, .. } = r else {
            panic!("expected a degrade outcome");
        };
        assert!(detail.contains("VRAM volume"), "got: {detail}");
    }
}
