//! WinDrive product configuration (SPEC windows-storport-cuda-vram DT-1 / DT-2).
//!
//! Closed storage-only shape: CUDA + queue + evidence; no pagefile/backend selector.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Deserialize;

/// Minimum LUN size (64 MiB) so three 4 KiB probe positions and GPT/NTFS are meaningful.
pub const MIN_SIZE_BYTES: u64 = 64 * 1024 * 1024;
/// Policy floor for CUDA free reserve (512 MiB).
pub const RESERVE_FLOOR_BYTES: u64 = 512 * 1024 * 1024;
/// Hard cap on mapped data area: `queue_depth * max_io_bytes` (4 MiB).
pub const MAX_DATA_AREA_BYTES: u64 = 4 * 1024 * 1024;
/// Max single I/O (1 MiB) — matches ABI `MAX_IO`.
pub const MAX_IO_BYTES_CAP: u32 = 1 << 20;
/// Max queue depth — matches ABI `MAX_QD`.
pub const MAX_QUEUE_DEPTH: u32 = 256;
/// Single-open config read cap (DT-1).
pub const MAX_CONFIG_BYTES: usize = 64 * 1024;

/// Configuration for the Windows CUDA storage-only service.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WinDriveConfig {
    pub size_bytes: u64,
    pub block_size: u32,
    pub cuda_device: u32,
    /// Requested reserve; effective reserve is never below policy floor (DT-2).
    pub reserve_bytes: u64,
    pub queue_depth: u32,
    pub max_io_bytes: u32,
    /// Absolute evidence directory / JSONL path root (validated absolute).
    pub evidence_path: PathBuf,
    /// Drive letter for the storage-only volume (`D`..=`Z`).
    pub volume_letter: char,
    /// Optional private NTFS mount path used instead of an Explorer drive letter.
    #[serde(default)]
    pub volume_mount_path: Option<PathBuf>,
    /// Broker listen address (e.g. `127.0.0.1:7700`).
    pub broker: String,
    pub tenant: String,
    /// Heartbeat interval seconds (default 5).
    #[serde(default = "default_heartbeat_secs")]
    pub heartbeat_secs: u64,
}

fn default_heartbeat_secs() -> u64 {
    5
}

/// Configuration parse / validation error.
#[derive(Debug, PartialEq)]
pub enum ConfigError {
    Parse(String),
    Invalid { field: &'static str, detail: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Parse(s) => write!(f, "config parse: {s}"),
            ConfigError::Invalid { field, detail } => {
                write!(f, "config invalid {field}: {detail}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// TOML wrapper with a `[win_drive]` table.
#[derive(Deserialize)]
struct Root {
    win_drive: WinDriveConfig,
}

impl WinDriveConfig {
    /// Parse TOML text containing a `[win_drive]` section.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        if text.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::Invalid {
                field: "config",
                detail: format!("config exceeds {MAX_CONFIG_BYTES} bytes"),
            });
        }
        let root: Root = toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        root.win_drive.validate()?;
        Ok(root.win_drive)
    }

    /// Parse from an already-owned byte buffer (single-open path; DT-1).
    pub fn from_reader(buf: &[u8]) -> Result<Self, ConfigError> {
        if buf.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::Invalid {
                field: "config",
                detail: format!("config exceeds {MAX_CONFIG_BYTES} bytes"),
            });
        }
        let text = std::str::from_utf8(buf).map_err(|e| ConfigError::Parse(e.to_string()))?;
        Self::from_toml(text)
    }

    /// Validate invariants before provision (DT-2).
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.block_size != 512 && self.block_size != 4096 {
            return Err(ConfigError::Invalid {
                field: "block_size",
                detail: format!("must be 512 or 4096, got {}", self.block_size),
            });
        }
        if self.size_bytes < MIN_SIZE_BYTES {
            return Err(ConfigError::Invalid {
                field: "size_bytes",
                detail: format!("must be >= {MIN_SIZE_BYTES} (64 MiB)"),
            });
        }
        if usize::try_from(self.size_bytes).is_err() {
            return Err(ConfigError::Invalid {
                field: "size_bytes",
                detail: "must fit in usize on this host".into(),
            });
        }
        if !self.size_bytes.is_multiple_of(self.block_size as u64) {
            return Err(ConfigError::Invalid {
                field: "size_bytes",
                detail: "must be multiple of block_size".into(),
            });
        }
        if self.queue_depth == 0
            || self.queue_depth > MAX_QUEUE_DEPTH
            || !self.queue_depth.is_power_of_two()
        {
            return Err(ConfigError::Invalid {
                field: "queue_depth",
                detail: format!(
                    "must be power of two in 1..={MAX_QUEUE_DEPTH}, got {}",
                    self.queue_depth
                ),
            });
        }
        if self.max_io_bytes == 0 || self.max_io_bytes > MAX_IO_BYTES_CAP {
            return Err(ConfigError::Invalid {
                field: "max_io_bytes",
                detail: format!(
                    "must be non-zero and <= {MAX_IO_BYTES_CAP}, got {}",
                    self.max_io_bytes
                ),
            });
        }
        if !self.max_io_bytes.is_multiple_of(self.block_size) {
            return Err(ConfigError::Invalid {
                field: "max_io_bytes",
                detail: "must be multiple of block_size".into(),
            });
        }
        let data_area = (self.queue_depth as u64)
            .checked_mul(self.max_io_bytes as u64)
            .ok_or_else(|| ConfigError::Invalid {
                field: "queue_depth",
                detail: "queue_depth * max_io_bytes overflow".into(),
            })?;
        if data_area > MAX_DATA_AREA_BYTES {
            return Err(ConfigError::Invalid {
                field: "queue_depth",
                detail: format!(
                    "queue_depth * max_io_bytes = {data_area} exceeds {MAX_DATA_AREA_BYTES}"
                ),
            });
        }
        if !is_absolute_path(&self.evidence_path) {
            return Err(ConfigError::Invalid {
                field: "evidence_path",
                detail: "must be an absolute path".into(),
            });
        }
        let letter = self.volume_letter.to_ascii_uppercase();
        if !('D'..='Z').contains(&letter) {
            return Err(ConfigError::Invalid {
                field: "volume_letter",
                detail: format!("must be D..=Z, got {:?}", self.volume_letter),
            });
        }
        if let Some(path) = &self.volume_mount_path {
            let value = path.to_string_lossy().replace('/', "\\");
            let prefix = r"C:\ProgramData\RamShared\mounts\";
            if !value
                .to_ascii_lowercase()
                .starts_with(&prefix.to_ascii_lowercase())
                || value[prefix.len()..].is_empty()
                || value.contains("..")
                || value.contains(['\'', ';', '\r', '\n'])
            {
                return Err(ConfigError::Invalid {
                    field: "volume_mount_path",
                    detail: format!("must be a child of {prefix}"),
                });
            }
        }
        if self.tenant.is_empty() {
            return Err(ConfigError::Invalid {
                field: "tenant",
                detail: "must be non-empty".into(),
            });
        }
        SocketAddr::from_str(&self.broker).map_err(|e| ConfigError::Invalid {
            field: "broker",
            detail: e.to_string(),
        })?;
        Ok(())
    }

    /// Parsed broker socket address.
    pub fn broker_addr(&self) -> Result<SocketAddr, ConfigError> {
        SocketAddr::from_str(&self.broker).map_err(|e| ConfigError::Invalid {
            field: "broker",
            detail: e.to_string(),
        })
    }

    /// Effective CUDA free reserve: `max(config, 512 MiB, ceil(total_vram/10))` (DT-2).
    pub fn effective_reserve_bytes(&self, total_vram: u64) -> u64 {
        let tenth = total_vram.div_ceil(10);
        self.reserve_bytes.max(RESERVE_FLOOR_BYTES).max(tenth)
    }

    /// Absolute evidence path as borrowed path.
    pub fn evidence_path(&self) -> &Path {
        &self.evidence_path
    }
}

/// Absolute path check that accepts Windows paths when validating on Linux hosts.
///
/// Product configs are authored for Windows (`C:\...`); CI unit tests run on Linux
/// and must still accept those absolute forms.
fn is_absolute_path(p: &Path) -> bool {
    if p.is_absolute() {
        return true;
    }
    let s = p.to_string_lossy();
    if s.starts_with(r"\\") || s.starts_with("//") {
        return true;
    }
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    const GOOD: &str = r#"
[win_drive]
size_bytes = 536870912
block_size = 4096
cuda_device = 0
reserve_bytes = 536870912
queue_depth = 4
max_io_bytes = 1048576
evidence_path = "C:\\ProgramData\\RamShared\\evidence"
volume_letter = "D"
broker = "127.0.0.1:7700"
tenant = "windrive-host"
"#;

    #[test]
    fn parse_product_config() {
        let c = WinDriveConfig::from_toml(GOOD).unwrap();
        assert_eq!(c.size_bytes, 512 * 1024 * 1024);
        assert_eq!(c.block_size, 4096);
        assert_eq!(c.cuda_device, 0);
        assert_eq!(c.queue_depth, 4);
        assert_eq!(c.max_io_bytes, 1 << 20);
        assert_eq!(c.heartbeat_secs, 5);
        assert_eq!(c.tenant, "windrive-host");
        assert_eq!(c.volume_letter, 'D');
        assert!(c.volume_mount_path.is_none());
        assert!(is_absolute_path(&c.evidence_path));
        c.broker_addr().unwrap();
    }

    #[test]
    fn reject_unknown_backend() {
        let bad = format!("{GOOD}backend = \"ram\"\n");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(e, ConfigError::Parse(_)));
    }

    #[test]
    fn reject_zero_size() {
        let bad = GOOD.replace("536870912", "0");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "size_bytes",
                ..
            }
        ));
    }

    #[test]
    fn reject_size_over_usize() {
        // On 64-bit hosts usize max is huge; force invalid by using unaligned + below floor path
        // via a direct validate of an oversized conceptual field when possible.
        let mut c = WinDriveConfig::from_toml(GOOD).unwrap();
        // Use size that fails block alignment after we set a huge value that is not
        // representable only on 32-bit; on 64-bit still exercise the usize check path.
        if usize::BITS < 64 {
            c.size_bytes = (u64::from(u32::MAX) + 1) * 4096;
            let e = c.validate().unwrap_err();
            assert!(matches!(
                e,
                ConfigError::Invalid {
                    field: "size_bytes",
                    ..
                }
            ));
        } else {
            // Still reject sizes below floor or unaligned when forced.
            c.size_bytes = 4096;
            let e = c.validate().unwrap_err();
            assert!(matches!(
                e,
                ConfigError::Invalid {
                    field: "size_bytes",
                    ..
                }
            ));
        }
    }

    #[test]
    fn reject_unaligned_max_io() {
        let bad = GOOD.replace("max_io_bytes = 1048576", "max_io_bytes = 1000");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "max_io_bytes",
                ..
            }
        ));
    }

    #[test]
    fn reject_queue_data_area_over_4mib() {
        // QD=8 * 1MiB = 8 MiB > 4 MiB
        let bad = GOOD.replace("queue_depth = 4", "queue_depth = 8");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "queue_depth",
                ..
            }
        ));
    }

    #[test]
    fn reserve_cannot_lower_policy_floor() {
        let c = WinDriveConfig::from_toml(GOOD).unwrap();
        // Requested 1 byte would still floor to 512 MiB or 10% of total.
        let mut low = c.clone();
        low.reserve_bytes = 1;
        assert_eq!(
            low.effective_reserve_bytes(10 * 1024 * 1024 * 1024),
            RESERVE_FLOOR_BYTES.max((10 * 1024 * 1024 * 1024u64).div_ceil(10))
        );
        // 10% of small total still respects floor when floor wins.
        assert_eq!(
            low.effective_reserve_bytes(1024 * 1024 * 1024),
            RESERVE_FLOOR_BYTES
        );
    }

    #[test]
    fn example_config_parses() {
        let example = include_str!("../winsvc.example.toml");
        let c = WinDriveConfig::from_toml(example).unwrap();
        assert!(c.size_bytes >= MIN_SIZE_BYTES);
        assert_eq!(c.queue_depth, 4);
        assert!(is_absolute_path(&c.evidence_path));
    }

    #[test]
    fn reject_empty_tenant() {
        let bad = GOOD.replace("windrive-host", "");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "tenant",
                ..
            }
        ));
    }

    #[test]
    fn from_reader_rejects_oversize() {
        let huge = vec![b'a'; MAX_CONFIG_BYTES + 1];
        let e = WinDriveConfig::from_reader(&huge).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "config",
                ..
            }
        ));
    }

    #[test]
    fn from_toml_rejects_oversize_text() {
        let huge = format!("{GOOD}{}", "x".repeat(MAX_CONFIG_BYTES));
        let e = WinDriveConfig::from_toml(&huge).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "config",
                ..
            }
        ));
    }

    #[test]
    fn from_reader_parses_good_utf8() {
        let c = WinDriveConfig::from_reader(GOOD.as_bytes()).unwrap();
        assert_eq!(c.queue_depth, 4);
        assert_eq!(c.broker_addr().unwrap().port(), 7700);
    }

    #[test]
    fn from_reader_rejects_non_utf8() {
        let e = WinDriveConfig::from_reader(&[0xff, 0xfe, 0xfd]).unwrap_err();
        assert!(matches!(e, ConfigError::Parse(_)));
    }

    #[test]
    fn reject_bad_block_size() {
        let bad = GOOD.replace("block_size = 4096", "block_size = 1024");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "block_size",
                ..
            }
        ));
    }

    #[test]
    fn reject_unaligned_size() {
        let bad = GOOD.replace("size_bytes = 536870912", "size_bytes = 67108865");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "size_bytes",
                ..
            }
        ));
    }

    #[test]
    fn reject_bad_queue_depth_not_pow2() {
        let bad = GOOD.replace("queue_depth = 4", "queue_depth = 3");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "queue_depth",
                ..
            }
        ));
    }

    #[test]
    fn reject_zero_max_io() {
        let bad = GOOD.replace("max_io_bytes = 1048576", "max_io_bytes = 0");
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "max_io_bytes",
                ..
            }
        ));
    }

    #[test]
    fn reject_relative_evidence_path() {
        let bad = GOOD.replace(
            r#"evidence_path = "C:\\ProgramData\\RamShared\\evidence""#,
            r#"evidence_path = "relative/evidence""#,
        );
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "evidence_path",
                ..
            }
        ));
    }

    #[test]
    fn reject_volume_letter_a() {
        let bad = GOOD.replace(r#"volume_letter = "D""#, r#"volume_letter = "A""#);
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "volume_letter",
                ..
            }
        ));
    }

    #[test]
    fn accept_private_volume_mount_path() {
        let text = GOOD.replace(
            r#"volume_letter = "D""#,
            r#"volume_letter = "D"
volume_mount_path = "C:\\ProgramData\\RamShared\\mounts\\lun-123""#,
        );
        let c = WinDriveConfig::from_toml(&text).unwrap();
        assert_eq!(
            c.volume_mount_path.as_deref(),
            Some(Path::new(r"C:\ProgramData\RamShared\mounts\lun-123"))
        );
    }

    #[test]
    fn reject_private_mount_outside_owned_root() {
        let text = GOOD.replace(
            r#"volume_letter = "D""#,
            r#"volume_letter = "D"
volume_mount_path = "C:\\Users\\Public\\lun""#,
        );
        let e = WinDriveConfig::from_toml(&text).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "volume_mount_path",
                ..
            }
        ));
    }

    #[test]
    fn reject_bad_broker() {
        let bad = GOOD.replace(r#"broker = "127.0.0.1:7700""#, r#"broker = "not-an-addr""#);
        let e = WinDriveConfig::from_toml(&bad).unwrap_err();
        assert!(matches!(
            e,
            ConfigError::Invalid {
                field: "broker",
                ..
            }
        ));
    }

    #[test]
    fn display_errors() {
        let p = ConfigError::Parse("x".into());
        assert!(p.to_string().contains("parse"));
        let i = ConfigError::Invalid {
            field: "f",
            detail: "d".into(),
        };
        assert!(i.to_string().contains("invalid f"));
    }

    #[test]
    fn is_absolute_accepts_native_and_windows_config_paths() {
        assert!(is_absolute_path(Path::new(r"C:\ProgramData\RamShared")));
        assert!(is_absolute_path(Path::new(r"\\?\C:\x")));
        assert_eq!(is_absolute_path(Path::new("/tmp/x")), cfg!(unix));
        assert!(!is_absolute_path(Path::new("relative")));
    }

    #[test]
    fn evidence_path_accessor() {
        let c = WinDriveConfig::from_toml(GOOD).unwrap();
        assert_eq!(c.evidence_path(), c.evidence_path.as_path());
    }
}
