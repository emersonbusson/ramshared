use crate::config::WinDriveConfig;
use ramshared_winbroker::BrokerConfigV1;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const REQUIRED_ARTIFACTS: [ArtifactRole; 7] = [
    ArtifactRole::BrokerExe,
    ArtifactRole::BrokerConfig,
    ArtifactRole::WinsvcExe,
    ArtifactRole::WinsvcConfig,
    ArtifactRole::DriverInf,
    ArtifactRole::DriverCat,
    ArtifactRole::DriverSys,
];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactRole {
    BrokerExe,
    BrokerConfig,
    WinsvcExe,
    WinsvcConfig,
    DriverInf,
    DriverCat,
    DriverSys,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManifestArtifact {
    pub role: ArtifactRole,
    pub relative_path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StartPolicy {
    Demand,
    Automatic,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ServiceIdentities {
    pub broker_name: String,
    pub broker_account: String,
    pub consumer_name: String,
    pub consumer_account: String,
}

impl ServiceIdentities {
    fn validate(&self) -> Result<(), String> {
        if self.broker_name != "RamSharedBroker"
            || self.broker_account != r"NT SERVICE\RamSharedBroker"
            || self.consumer_name != "RamSharedWinSvc"
            || self.consumer_account != "LocalSystem"
        {
            return Err("manifest service identities do not match the product contract".into());
        }
        Ok(())
    }
}

impl ProductManifestV1 {
    pub fn artifact(&self, role: ArtifactRole) -> Result<&ManifestArtifact, String> {
        self.artifacts
            .iter()
            .find(|artifact| artifact.role == role)
            .ok_or_else(|| format!("validated manifest is missing role {role:?}"))
    }

    pub fn version_directory(&self) -> String {
        format!("{}-{}", self.version, &self.commit[..12])
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProductManifestV1 {
    pub schema: u32,
    pub version: String,
    pub commit: String,
    pub architecture: String,
    pub start_policy: StartPolicy,
    pub services: ServiceIdentities,
    pub artifacts: Vec<ManifestArtifact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScmDefinition {
    pub service_name: String,
    pub image_path: PathBuf,
    pub config_path: PathBuf,
    pub dependencies: Vec<String>,
    pub start_policy: StartPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RollbackRecord {
    pub old_manifest: ProductManifestV1,
    pub old_definitions: Vec<ScmDefinition>,
    pub candidate_started: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPlan {
    pub version_directory: String,
    pub idempotent: bool,
    pub rollback_old: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductStatus {
    pub storage_owned: bool,
    pub ambiguous_residue: bool,
    pub services_stopped: bool,
    pub active_version: Option<String>,
}

pub fn parse_manifest(bytes: &[u8]) -> Result<ProductManifestV1, String> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err("manifest exceeds 64 KiB".into());
    }
    let manifest: ProductManifestV1 = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if manifest.schema != 1 {
        return Err("schema must be 1".into());
    }
    if manifest.commit.len() < 12
        || !manifest.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        || manifest.version.is_empty()
        || manifest.architecture != "x86_64-pc-windows-msvc"
    {
        return Err("version, hexadecimal commit >=12, and supported architecture required".into());
    }
    manifest.services.validate()?;
    let mut roles = BTreeSet::new();
    for artifact in &manifest.artifacts {
        validate_artifact_path(&artifact.relative_path)?;
        validate_hash(&artifact.sha256)?;
        if !roles.insert(artifact.role) {
            return Err("duplicate artifact role".into());
        }
    }
    if roles != REQUIRED_ARTIFACTS.into_iter().collect() {
        return Err("manifest must contain exactly all seven product artifact roles".into());
    }
    Ok(manifest)
}

fn validate_hash(hash: &str) -> Result<(), String> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_lowercase())
    {
        return Err("sha256 must be uppercase 64-hex".into());
    }
    Ok(())
}

pub fn validate_artifact_path(value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with(['/', '\\']) || value.contains(':') {
        return Err("artifact path must be a normalized relative child".into());
    }

    if value
        .split(['/', '\\'])
        .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err("artifact path must be a normalized relative child".into());
    }

    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("artifact path must be a normalized relative child".into());
    }

    Ok(())
}

pub fn verify_staged_files(
    staging_root: &Path,
    manifest: &ProductManifestV1,
) -> Result<(), String> {
    for artifact in &manifest.artifacts {
        validate_artifact_path(&artifact.relative_path)?;
        let path = staging_root.join(&artifact.relative_path);
        let mut file = File::open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let metadata = file
            .metadata()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("{} is not a regular file", path.display()));
        }
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        let hash_hex: String = digest
            .finalize()
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect();
        if hash_hex != artifact.sha256 {
            return Err(format!("{} hash mismatch", path.display()));
        }
    }
    Ok(())
}

pub fn validate_cross_config(
    broker: &BrokerConfigV1,
    winsvc: &WinDriveConfig,
) -> Result<(), String> {
    if broker.capacity_bytes != winsvc.size_bytes {
        return Err("broker capacity must equal LUN size".into());
    }
    if broker.allowed_tenant != winsvc.tenant {
        return Err("broker tenant must equal winsvc tenant".into());
    }
    if broker.capacity_bytes == 0
        || !broker
            .capacity_bytes
            .is_multiple_of(u64::from(winsvc.block_size))
    {
        return Err("broker capacity must be nonzero and block aligned".into());
    }
    Ok(())
}

pub fn plan_install(
    current: Option<&ProductManifestV1>,
    candidate: &ProductManifestV1,
) -> InstallPlan {
    InstallPlan {
        version_directory: candidate.version_directory(),
        idempotent: current == Some(candidate),
        rollback_old: current.is_some(),
    }
}

pub fn plan_rollback(record: &RollbackRecord) -> Result<Vec<ScmDefinition>, String> {
    if record.candidate_started {
        return Err("started candidate requires forward-only safe stop".into());
    }
    if record.old_definitions.len() != 2 {
        return Err("rollback requires both old SCM definitions".into());
    }
    Ok(record.old_definitions.clone())
}

pub fn plan_uninstall(status: &ProductStatus) -> Result<(), String> {
    if status.storage_owned || status.ambiguous_residue || !status.services_stopped {
        Err("uninstall refuses owned or ambiguous storage and running services".into())
    } else {
        Ok(())
    }
}

pub fn build_residue_script(letter: char) -> Result<String, Box<dyn std::error::Error>> {
    let uppercase_letter = letter.to_ascii_uppercase();
    if !uppercase_letter.is_ascii_uppercase() {
        return Err("invalid volume letter for host residue query".into());
    }
    let escaped_letter = uppercase_letter.to_string().replace('\'', "''");
    Ok(format!(
        concat!(
            "$ErrorActionPreference='Stop';",
            "$d=@(Get-CimInstance Win32_DiskDrive|?{{",
            "$_.Model -match 'RAMSHARE|VRAMDISK' -or $_.Caption -match 'RAMSHARE|VRAMDISK'}});",
            "$p=@(Get-CimInstance Win32_PageFileUsage|?{{",
            "$_.Name -match '^{letter}:\\\\'}});",
            "Write-Output ($d.Count.ToString()+'|'+$p.Count.ToString())"
        ),
        letter = escaped_letter
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use std::path::PathBuf;

    fn identities() -> ServiceIdentities {
        ServiceIdentities {
            broker_name: "RamSharedBroker".into(),
            broker_account: r"NT SERVICE\RamSharedBroker".into(),
            consumer_name: "RamSharedWinSvc".into(),
            consumer_account: "LocalSystem".into(),
        }
    }

    fn manifest() -> ProductManifestV1 {
        ProductManifestV1 {
            schema: 1,
            version: "1.2.3".into(),
            commit: "abcdef1234567890".into(),
            architecture: "x86_64-pc-windows-msvc".into(),
            start_policy: StartPolicy::Demand,
            services: identities(),
            artifacts: REQUIRED_ARTIFACTS
                .iter()
                .enumerate()
                .map(|(index, role)| ManifestArtifact {
                    role: *role,
                    relative_path: format!("artifact-{index}"),
                    sha256: "A".repeat(64),
                })
                .collect(),
        }
    }

    #[test]
    fn manifest_rejects_unknown_and_over_64k() {
        let mut value = serde_json::to_value(manifest()).unwrap();
        value["unknown"] = serde_json::json!(1);
        assert!(parse_manifest(&serde_json::to_vec(&value).unwrap()).is_err());
        assert!(parse_manifest(&vec![b'x'; MAX_MANIFEST_BYTES + 1]).is_err());
    }

    #[test]
    fn artifact_path_cannot_escape() {
        for path in ["../x", r"C:\x", "/x", "a/../x", "./x", ""] {
            assert!(validate_artifact_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn hash_must_be_sha256_hex() {
        let mut candidate = manifest();
        candidate.artifacts[0].sha256 = "a".repeat(64);
        assert!(parse_manifest(&serde_json::to_vec(&candidate).unwrap()).is_err());
    }

    #[test]
    fn mixed_commit_is_refused() {
        let mut candidate = manifest();
        candidate.commit = "short".into();
        assert!(parse_manifest(&serde_json::to_vec(&candidate).unwrap()).is_err());
    }

    #[test]
    fn broker_capacity_must_equal_lun_size() {
        assert!(validate_cross_config(&broker_config(1, "t"), &fake_config(2, "t")).is_err());
    }

    #[test]
    fn broker_tenant_must_equal_winsvc_tenant() {
        assert!(validate_cross_config(&broker_config(4096, "x"), &fake_config(4096, "t")).is_err());
    }

    #[test]
    fn same_version_repair_is_idempotent() {
        let candidate = manifest();
        assert!(plan_install(Some(&candidate), &candidate).idempotent);
    }

    #[test]
    fn half_registered_candidate_rolls_back_old_definitions() {
        let record = RollbackRecord {
            old_manifest: manifest(),
            old_definitions: vec![definition("RamSharedBroker"), definition("RamSharedWinSvc")],
            candidate_started: false,
        };
        assert_eq!(plan_rollback(&record).unwrap(), record.old_definitions);
    }

    #[test]
    fn uninstall_refuses_owned_storage() {
        assert!(
            plan_uninstall(&ProductStatus {
                storage_owned: true,
                ambiguous_residue: false,
                services_stopped: true,
                active_version: Some("1".into()),
            })
            .is_err()
        );
    }

    #[test]
    fn staged_files_are_single_open_hashed_and_mismatch_is_refused() {
        let root =
            std::env::temp_dir().join(format!("ramshared-package-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut candidate = manifest();
        for artifact in &mut candidate.artifacts {
            let bytes = artifact.relative_path.as_bytes();
            std::fs::write(root.join(&artifact.relative_path), bytes).unwrap();
            artifact.sha256 = Sha256::digest(bytes)
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect();
        }
        assert!(verify_staged_files(&root, &candidate).is_ok());
        std::fs::write(
            root.join(&candidate.artifacts[0].relative_path),
            b"tampered",
        )
        .unwrap();
        assert!(verify_staged_files(&root, &candidate).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn started_candidate_is_forward_only_and_clean_uninstall_is_allowed() {
        let record = RollbackRecord {
            old_manifest: manifest(),
            old_definitions: vec![definition("RamSharedBroker"), definition("RamSharedWinSvc")],
            candidate_started: true,
        };
        assert!(plan_rollback(&record).is_err());
        assert!(
            plan_uninstall(&ProductStatus {
                storage_owned: false,
                ambiguous_residue: false,
                services_stopped: true,
                active_version: Some("1".into()),
            })
            .is_ok()
        );
    }

    fn definition(name: &str) -> ScmDefinition {
        ScmDefinition {
            service_name: name.into(),
            image_path: PathBuf::from(format!(r"C:\versions\{name}.exe")),
            config_path: PathBuf::from(format!(r"C:\versions\{name}.toml")),
            dependencies: vec![],
            start_policy: StartPolicy::Demand,
        }
    }

    fn broker_config(size: u64, tenant: &str) -> BrokerConfigV1 {
        BrokerConfigV1 {
            schema: 1,
            capacity_bytes: size,
            allowed_tenant: tenant.into(),
            evidence_path: PathBuf::from(r"C:\e"),
        }
    }

    fn fake_config(size: u64, tenant: &str) -> WinDriveConfig {
        WinDriveConfig {
            size_bytes: size,
            block_size: 4096,
            cuda_device: 0,
            reserve_bytes: 512 * 1024 * 1024,
            queue_depth: 4,
            max_io_bytes: 4096,
            evidence_path: PathBuf::from(r"C:\e"),
            volume_letter: 'D',
            volume_mount_path: None,
            broker_pipe: crate::config::BrokerPipeV1::NamedPipeV1,
            broker_ready_timeout_secs: 30,
            tenant: tenant.into(),
            heartbeat_secs: 5,
        }
    }

    #[test]
    fn observe_host_residue_script_escaping() {
        let valid_script = build_residue_script('D').unwrap();
        assert!(valid_script.contains("$_.Name -match '^D:\\\\'"));

        let lowercase_script = build_residue_script('z').unwrap();
        assert!(lowercase_script.contains("$_.Name -match '^Z:\\\\'"));

        let invalid_char = build_residue_script('1');
        assert!(invalid_char.is_err());

        let injected_char = build_residue_script('\'');
        assert!(injected_char.is_err());
    }
}
