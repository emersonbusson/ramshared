#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use ramshared_config::resource_profile::{
    DISK_RESERVE_FLOOR_BYTES, RESOURCE_PROFILE_SCHEMA_VERSION, ResourcePlatform, ResourceProfile,
    ResourceProfileError, ResourceTarget, TierCaps, checked_required_free_bytes,
};

#[test]
fn resource_profile_accepts_variable_caps_and_rejects_overflow() {
    let mut adapter_caps = BTreeMap::new();
    adapter_caps.insert("gpu-uuid:adapter-a".to_string(), 1536 * 1024 * 1024);
    let profile = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps {
            zram_bytes: Some(256 * 1024 * 1024),
            vram_bytes: adapter_caps,
            origin_bytes: Some(3 * 1024 * 1024 * 1024),
        },
        target: Some(ResourceTarget::LinuxSwapfile {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            mount_id: 27,
            managed_relative_path: "swap/ramshared-a.swap".into(),
            bytes: 128 * 1024 * 1024,
            priority: -1,
        }),
    };

    profile
        .validate_for(ResourcePlatform::NativeLinux)
        .expect("variable user ceilings and a valid native target are accepted");

    let required = checked_required_free_bytes(&[128 * 1024 * 1024, 3 * 1024 * 1024 * 1024])
        .expect("managed sizes fit in checked arithmetic");
    assert_eq!(
        required,
        DISK_RESERVE_FLOOR_BYTES + 128 * 1024 * 1024 + 3 * 1024 * 1024 * 1024
    );
    assert!(checked_required_free_bytes(&[u64::MAX, 1]).is_err());
}

#[test]
fn resource_profile_roundtrips_stable_volume_and_adapter_ids() {
    let text = r#"
schema_version = 1

[caps]
zram_bytes = 0
origin_bytes = 2147483648

[caps.vram_bytes]
"luid:aabbccdd:00001122" = 1610612736

[target]
kind = "wsl_fallback"
windows_volume_id = "\\\\?\\Volume{01234567-89ab-cdef-0123-456789abcdef}\\"
path = "I:\\wsl\\swap.vhdx"
bytes = 4294967296
"#;
    let profile = ResourceProfile::parse(text).expect("profile parses");
    profile
        .validate_for(ResourcePlatform::Wsl2)
        .expect("stable volume and adapter identities are valid");

    let encoded = profile.to_toml().expect("profile serializes");
    let decoded = ResourceProfile::parse(&encoded).expect("serialized profile parses");
    decoded
        .validate_for(ResourcePlatform::Wsl2)
        .expect("round-tripped profile remains valid");

    assert_eq!(decoded, profile);
    assert!(encoded.contains("luid:aabbccdd:00001122"));
    assert!(encoded.contains("Volume{01234567-89ab-cdef-0123-456789abcdef}"));

    let file_origin = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        target: Some(ResourceTarget::LinuxFileOrigin {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            mount_id: 27,
            managed_relative_path: "origin/ramshared-a.img".into(),
            inode: 42,
            allocated_bytes: 3 * 1024 * 1024 * 1024,
            identity_field_hash: "a".repeat(64),
        }),
    };
    file_origin
        .validate_for(ResourcePlatform::NativeLinux)
        .expect("sealed native file-origin identity is valid");

    let volume_guid_path = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        target: Some(ResourceTarget::WslFallback {
            windows_volume_id: "volume-guid-b".into(),
            path: r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\wsl\swap.vhdx".into(),
            bytes: 4 * 1024 * 1024 * 1024,
        }),
    };
    volume_guid_path
        .validate_for(ResourcePlatform::Wsl2)
        .expect("volume-GUID path is absolute");
}

#[test]
fn resource_profile_rejects_platform_mismatch_unknown_fields_and_unsafe_paths() {
    let profile = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        target: Some(ResourceTarget::WslFallback {
            windows_volume_id: "volume-guid".into(),
            path: "I:\\wsl\\swap.vhdx".into(),
            bytes: 4 * 1024 * 1024 * 1024,
        }),
    };
    assert!(profile.validate_for(ResourcePlatform::NativeLinux).is_err());

    assert!(ResourceProfile::parse("schema_version = 1\nunknown = true\n").is_err());
    let unsupported = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION + 1,
        caps: TierCaps::default(),
        target: None,
    };
    assert!(
        unsupported
            .validate_for(ResourcePlatform::NativeLinux)
            .is_err()
    );

    let caps_only = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        target: None,
    };
    assert!(
        caps_only
            .validate_for(ResourcePlatform::NativeLinux)
            .is_ok()
    );
    assert!(caps_only.validate_for(ResourcePlatform::Wsl2).is_ok());

    let unsafe_target = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        target: Some(ResourceTarget::LinuxFileOrigin {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            mount_id: 27,
            managed_relative_path: "../outside/origin.img".into(),
            inode: 42,
            allocated_bytes: 1024,
            identity_field_hash: "a".repeat(64),
        }),
    };
    assert!(
        unsafe_target
            .validate_for(ResourcePlatform::NativeLinux)
            .is_err()
    );
}

#[test]
fn resource_profile_rejects_zero_or_unbound_storage_identity() {
    let invalid = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        target: Some(ResourceTarget::LinuxSwapfile {
            filesystem_uuid: " ".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            mount_id: 0,
            managed_relative_path: "swap/ramshared-a.swap".into(),
            bytes: 0,
            priority: i32::MAX,
        }),
    };
    assert!(invalid.validate_for(ResourcePlatform::NativeLinux).is_err());

    let invalid_swap_targets = [
        ResourceTarget::LinuxSwapfile {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            mount_id: 0,
            managed_relative_path: "swap/ramshared-a.swap".into(),
            bytes: 1024,
            priority: -1,
        },
        ResourceTarget::LinuxSwapfile {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            mount_id: 27,
            managed_relative_path: "swap/ramshared-a.swap".into(),
            bytes: 0,
            priority: -1,
        },
        ResourceTarget::LinuxSwapfile {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            mount_id: 27,
            managed_relative_path: "swap/ramshared-a.swap".into(),
            bytes: 1024,
            priority: 32_768,
        },
    ];
    for target in invalid_swap_targets {
        let profile = ResourceProfile {
            schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
            caps: TierCaps::default(),
            target: Some(target),
        };
        assert!(profile.validate_for(ResourcePlatform::NativeLinux).is_err());
    }

    let invalid_origin = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        target: Some(ResourceTarget::LinuxFileOrigin {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            mount_id: 27,
            managed_relative_path: "origin/ramshared-a.img".into(),
            inode: 0,
            allocated_bytes: 1024,
            identity_field_hash: "not-a-hash".into(),
        }),
    };
    assert!(
        invalid_origin
            .validate_for(ResourcePlatform::NativeLinux)
            .is_err()
    );

    let invalid_windows_path = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        target: Some(ResourceTarget::WslFallback {
            windows_volume_id: "volume-guid-a".into(),
            path: r"relative\swap.vhdx".into(),
            bytes: 1024,
        }),
    };
    assert!(
        invalid_windows_path
            .validate_for(ResourcePlatform::Wsl2)
            .is_err()
    );
}

#[test]
fn resource_profile_rejects_oversized_or_controlled_identity_and_paths() {
    for identity in [String::new(), "gpu\nidentity".into(), "x".repeat(513)] {
        let mut vram_bytes = BTreeMap::new();
        vram_bytes.insert(identity, 1024);
        let profile = ResourceProfile {
            schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
            caps: TierCaps {
                vram_bytes,
                ..TierCaps::default()
            },
            target: None,
        };
        assert!(profile.validate_for(ResourcePlatform::NativeLinux).is_err());
    }
    assert!(ResourceProfile::parse(&" ".repeat(64 * 1024 + 1)).is_err());

    let unsafe_linux_paths = [
        String::new(),
        "/absolute/path".into(),
        "nested//empty".into(),
        "nested/./dot".into(),
        "nested/line\nbreak".into(),
        format!("nested/{}", "x".repeat(4096)),
    ];
    for managed_relative_path in unsafe_linux_paths {
        let profile = ResourceProfile {
            schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
            caps: TierCaps::default(),
            target: Some(ResourceTarget::LinuxSwapfile {
                filesystem_uuid: "fs-uuid-a".into(),
                device_identity: "wwn-0x5000-local-a".into(),
                mount_id: 27,
                managed_relative_path,
                bytes: 1024,
                priority: -1,
            }),
        };
        assert!(profile.validate_for(ResourcePlatform::NativeLinux).is_err());
    }

    for path in [
        "C:\\wsl\\line\nbreak.vhdx".into(),
        format!("C:\\{}", "x".repeat(32_768)),
    ] {
        let profile = ResourceProfile {
            schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
            caps: TierCaps::default(),
            target: Some(ResourceTarget::WslFallback {
                windows_volume_id: "volume-guid-a".into(),
                path,
                bytes: 1024,
            }),
        };
        assert!(profile.validate_for(ResourcePlatform::Wsl2).is_err());
    }
}

#[test]
fn resource_profile_errors_render_without_losing_the_failed_gate() {
    assert!(
        ResourceProfileError::Parse("bad TOML".into())
            .to_string()
            .contains("bad TOML")
    );
    assert!(
        ResourceProfileError::UnsupportedSchemaVersion(9)
            .to_string()
            .contains("version 9")
    );
    assert!(
        ResourceProfileError::PlatformMismatch {
            target: "wsl_fallback",
            platform: "native_linux",
        }
        .to_string()
        .contains("native_linux")
    );
    assert!(
        ResourceProfileError::Invalid {
            field: "target.bytes",
            reason: "must be greater than zero",
        }
        .to_string()
        .contains("target.bytes")
    );
    assert!(
        ResourceProfileError::CapacityOverflow
            .to_string()
            .contains("overflows")
    );
}
