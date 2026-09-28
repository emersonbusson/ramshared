#![allow(clippy::expect_used)]

use std::collections::BTreeMap;

use ramshared_config::resource_profile::{
    DISK_RESERVE_FLOOR_BYTES, RESOURCE_PROFILE_SCHEMA_VERSION, ResourcePlatform, ResourceProfile,
    ResourceProfileError, ResourceTarget, StorageVolumeIdentity, TierCaps,
    checked_required_free_bytes,
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
        targets: vec![ResourceTarget::LinuxSwapfile {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            managed_relative_path: "swap/ramshared-a.swap".into(),
            bytes: 128 * 1024 * 1024,
            priority: -1,
        }],
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
fn resource_profile_supports_multiple_targets_on_one_and_multiple_volumes() {
    let text = r#"
schema_version = 1

[[targets]]
kind = "linux_swapfile"
filesystem_uuid = "fs-uuid-a"
device_identity = "wwn-0x5000-local-a"
managed_relative_path = "swap/ramshared-a.swap"
bytes = 1073741824
priority = -1

[[targets]]
kind = "linux_file_origin"
filesystem_uuid = "fs-uuid-a"
device_identity = "wwn-0x5000-local-a"
managed_relative_path = "origin/ramshared-a.img"
inode = 42
allocated_bytes = 3221225472
identity_field_hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[[targets]]
kind = "linux_file_origin"
filesystem_uuid = "fs-uuid-b"
device_identity = "wwn-0x5000-local-b"
managed_relative_path = "origin/ramshared-b.img"
inode = 84
allocated_bytes = 2147483648
identity_field_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
"#;

    let profile = ResourceProfile::parse(text)
        .expect("one profile can describe swap and origin targets on multiple volumes");
    profile
        .validate_for(ResourcePlatform::NativeLinux)
        .expect("all selected targets have stable native identities");
    let encoded = profile.to_toml().expect("multi-target profile serializes");
    let decoded = ResourceProfile::parse(&encoded).expect("multi-target profile round-trips");

    assert_eq!(decoded, profile);
    assert_eq!(encoded.matches("[[targets]]").count(), 3);

    let requirements = profile
        .required_free_bytes_by_volume()
        .expect("per-volume allocation totals fit in checked arithmetic");
    assert_eq!(requirements.len(), 2);
    assert_eq!(
        requirements.get(&StorageVolumeIdentity::Linux {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
        }),
        Some(&(DISK_RESERVE_FLOOR_BYTES + 4 * 1024 * 1024 * 1024))
    );
    assert_eq!(
        requirements.get(&StorageVolumeIdentity::Linux {
            filesystem_uuid: "fs-uuid-b".into(),
            device_identity: "wwn-0x5000-local-b".into(),
        }),
        Some(&(DISK_RESERVE_FLOOR_BYTES + 2 * 1024 * 1024 * 1024))
    );
}

#[test]
fn resource_profile_accepts_a_new_linux_origin_request_without_a_preexisting_inode() {
    let profile = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps {
            origin_bytes: Some(4 * 1024 * 1024 * 1024),
            ..TierCaps::default()
        },
        targets: vec![ResourceTarget::LinuxFileOriginRequest {
            filesystem_uuid: "fs-uuid-new".into(),
            device_identity: "wwn-local-nvme".into(),
            managed_relative_path: "origin/ramshared.img".into(),
            allocated_bytes: 4 * 1024 * 1024 * 1024,
        }],
    };

    profile
        .validate_for(ResourcePlatform::NativeLinux)
        .expect("a planned origin has no inode until the provider creates it");
    assert!(profile.validate_for(ResourcePlatform::Wsl2).is_err());

    let required = profile
        .required_free_bytes_by_volume()
        .expect("planned origin capacity is checked");
    assert_eq!(
        required.get(&StorageVolumeIdentity::Linux {
            filesystem_uuid: "fs-uuid-new".into(),
            device_identity: "wwn-local-nvme".into(),
        }),
        Some(&(DISK_RESERVE_FLOOR_BYTES + 4 * 1024 * 1024 * 1024))
    );

    let encoded = profile.to_toml().expect("origin request serializes");
    let decoded = ResourceProfile::parse(&encoded).expect("origin request roundtrips");
    assert_eq!(decoded, profile);

    let zero_sized = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: vec![ResourceTarget::LinuxFileOriginRequest {
            filesystem_uuid: "fs-uuid-new".into(),
            device_identity: "wwn-local-nvme".into(),
            managed_relative_path: "origin/ramshared.img".into(),
            allocated_bytes: 0,
        }],
    };
    assert!(
        zero_sized
            .validate_for(ResourcePlatform::NativeLinux)
            .is_err()
    );

    let duplicate_request_and_sealed = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: vec![
            ResourceTarget::LinuxFileOrigin {
                filesystem_uuid: "fs-uuid-new".into(),
                device_identity: "wwn-local-nvme".into(),
                managed_relative_path: "origin/ramshared.img".into(),
                inode: 12,
                allocated_bytes: 4 * 1024 * 1024 * 1024,
                identity_field_hash: "a".repeat(64),
            },
            ResourceTarget::LinuxFileOriginRequest {
                filesystem_uuid: "fs-uuid-new".into(),
                device_identity: "wwn-local-nvme".into(),
                managed_relative_path: "origin/ramshared.img".into(),
                allocated_bytes: 4 * 1024 * 1024 * 1024,
            },
        ],
    };
    assert!(
        duplicate_request_and_sealed
            .validate_for(ResourcePlatform::NativeLinux)
            .is_err()
    );
}

#[test]
fn resource_profile_rejects_duplicate_managed_paths_and_capacity_overflow() {
    let duplicate_path = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: vec![
            ResourceTarget::WslFallback {
                windows_volume_id: "volume-guid-a".into(),
                path: r"C:\wsl\swap.vhdx".into(),
                bytes: 1024,
            },
            ResourceTarget::WslOrigin {
                windows_volume_id: "VOLUME-GUID-A".into(),
                path: r"c:/WSL/SWAP.VHDX".into(),
                allocated_bytes: 2048,
            },
        ],
    };
    assert!(duplicate_path.validate_for(ResourcePlatform::Wsl2).is_err());

    let overflow = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: vec![
            ResourceTarget::WslFallback {
                windows_volume_id: "volume-guid-a".into(),
                path: r"C:\wsl\swap.vhdx".into(),
                bytes: u64::MAX,
            },
            ResourceTarget::WslOrigin {
                windows_volume_id: "volume-guid-a".into(),
                path: r"C:\wsl\origin.vhdx".into(),
                allocated_bytes: 1,
            },
        ],
    };
    overflow
        .validate_for(ResourcePlatform::Wsl2)
        .expect("target identity is valid before calculating capacity");
    assert_eq!(
        overflow.required_free_bytes_by_volume(),
        Err(ResourceProfileError::CapacityOverflow)
    );
}

#[test]
fn resource_profile_groups_windows_volume_ids_case_insensitively_for_capacity() {
    let profile = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: vec![
            ResourceTarget::WslFallback {
                windows_volume_id: "volume-guid-a".into(),
                path: r"C:\wsl\swap.vhdx".into(),
                bytes: 30 * 1024 * 1024 * 1024,
            },
            ResourceTarget::WslOrigin {
                windows_volume_id: "VOLUME-GUID-A".into(),
                path: r"C:\wsl\origin.vhdx".into(),
                allocated_bytes: 30 * 1024 * 1024 * 1024,
            },
        ],
    };

    let requirements = profile
        .required_free_bytes_by_volume()
        .expect("same Windows volume requirements add without overflow");

    assert_eq!(requirements.len(), 1);
    assert_eq!(
        requirements.get(&StorageVolumeIdentity::Windows {
            volume_id: "volume-guid-a".into(),
        }),
        Some(&(DISK_RESERVE_FLOOR_BYTES + 60 * 1024 * 1024 * 1024))
    );
}

#[test]
fn resource_profile_rejects_transient_mount_id_in_persisted_targets() {
    let text = r#"
schema_version = 1

[[targets]]
kind = "linux_swapfile"
filesystem_uuid = "fs-uuid-a"
device_identity = "wwn-0x5000-local-a"
mount_id = 27
managed_relative_path = "swap/ramshared-a.swap"
bytes = 1073741824
priority = -1
"#;

    assert!(ResourceProfile::parse(text).is_err());
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

[[targets]]
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
        targets: vec![ResourceTarget::LinuxFileOrigin {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            managed_relative_path: "origin/ramshared-a.img".into(),
            inode: 42,
            allocated_bytes: 3 * 1024 * 1024 * 1024,
            identity_field_hash: "a".repeat(64),
        }],
    };
    file_origin
        .validate_for(ResourcePlatform::NativeLinux)
        .expect("sealed native file-origin identity is valid");

    let volume_guid_path = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: vec![
            ResourceTarget::WslFallback {
                windows_volume_id: "volume-guid-b".into(),
                path: r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\wsl\swap.vhdx".into(),
                bytes: 4 * 1024 * 1024 * 1024,
            },
            ResourceTarget::WslOrigin {
                windows_volume_id: "volume-guid-b".into(),
                path: r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\ramshared\origin.vhdx"
                    .into(),
                allocated_bytes: 8 * 1024 * 1024 * 1024,
            },
        ],
    };
    volume_guid_path
        .validate_for(ResourcePlatform::Wsl2)
        .expect("volume-GUID swap and origin paths are absolute");
}

#[test]
fn resource_profile_rejects_platform_mismatch_unknown_fields_and_unsafe_paths() {
    let profile = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: vec![ResourceTarget::WslFallback {
            windows_volume_id: "volume-guid".into(),
            path: "I:\\wsl\\swap.vhdx".into(),
            bytes: 4 * 1024 * 1024 * 1024,
        }],
    };
    assert!(profile.validate_for(ResourcePlatform::NativeLinux).is_err());

    assert!(ResourceProfile::parse("schema_version = 1\nunknown = true\n").is_err());
    let unsupported = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION + 1,
        caps: TierCaps::default(),
        targets: Vec::new(),
    };
    assert!(
        unsupported
            .validate_for(ResourcePlatform::NativeLinux)
            .is_err()
    );

    let caps_only = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: Vec::new(),
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
        targets: vec![ResourceTarget::LinuxFileOrigin {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            managed_relative_path: "../outside/origin.img".into(),
            inode: 42,
            allocated_bytes: 1024,
            identity_field_hash: "a".repeat(64),
        }],
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
        targets: vec![ResourceTarget::LinuxSwapfile {
            filesystem_uuid: " ".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            managed_relative_path: "swap/ramshared-a.swap".into(),
            bytes: 0,
            priority: i32::MAX,
        }],
    };
    assert!(invalid.validate_for(ResourcePlatform::NativeLinux).is_err());

    let invalid_swap_targets = [
        ResourceTarget::LinuxSwapfile {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: " ".into(),
            managed_relative_path: "swap/ramshared-a.swap".into(),
            bytes: 1024,
            priority: -1,
        },
        ResourceTarget::LinuxSwapfile {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            managed_relative_path: "swap/ramshared-a.swap".into(),
            bytes: 0,
            priority: -1,
        },
        ResourceTarget::LinuxSwapfile {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            managed_relative_path: "swap/ramshared-a.swap".into(),
            bytes: 1024,
            priority: 32_768,
        },
    ];
    for target in invalid_swap_targets {
        let profile = ResourceProfile {
            schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
            caps: TierCaps::default(),
            targets: vec![target],
        };
        assert!(profile.validate_for(ResourcePlatform::NativeLinux).is_err());
    }

    let invalid_origin = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: vec![ResourceTarget::LinuxFileOrigin {
            filesystem_uuid: "fs-uuid-a".into(),
            device_identity: "wwn-0x5000-local-a".into(),
            managed_relative_path: "origin/ramshared-a.img".into(),
            inode: 0,
            allocated_bytes: 1024,
            identity_field_hash: "not-a-hash".into(),
        }],
    };
    assert!(
        invalid_origin
            .validate_for(ResourcePlatform::NativeLinux)
            .is_err()
    );

    let invalid_windows_path = ResourceProfile {
        schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
        caps: TierCaps::default(),
        targets: vec![ResourceTarget::WslFallback {
            windows_volume_id: "volume-guid-a".into(),
            path: r"relative\swap.vhdx".into(),
            bytes: 1024,
        }],
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
            targets: Vec::new(),
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
            targets: vec![ResourceTarget::LinuxSwapfile {
                filesystem_uuid: "fs-uuid-a".into(),
                device_identity: "wwn-0x5000-local-a".into(),
                managed_relative_path,
                bytes: 1024,
                priority: -1,
            }],
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
            targets: vec![ResourceTarget::WslFallback {
                windows_volume_id: "volume-guid-a".into(),
                path,
                bytes: 1024,
            }],
        };
        assert!(profile.validate_for(ResourcePlatform::Wsl2).is_err());
    }
}

#[test]
fn resource_profile_rejects_ambiguous_windows_target_paths() {
    let ambiguous_paths = [
        r"C:\wsl\\swap.vhdx",
        r"C:\wsl\.\swap.vhdx",
        r"C:\wsl\..\swap.vhdx",
        r"C:\wsl\swap.vhdx:stream",
        r"C:\wsl\swap.vhdx. ",
        r"C:\wsl\NUL.txt",
        r"C:\wsl\swap.vhdx\",
        r"C:\",
        r"C:\wsl/swap.vhdx",
        r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\wsl\..\swap.vhdx",
    ];

    for path in ambiguous_paths {
        let profile = ResourceProfile {
            schema_version: RESOURCE_PROFILE_SCHEMA_VERSION,
            caps: TierCaps::default(),
            targets: vec![ResourceTarget::WslFallback {
                windows_volume_id: "volume-guid-a".into(),
                path: path.into(),
                bytes: 1024,
            }],
        };

        assert!(
            profile.validate_for(ResourcePlatform::Wsl2).is_err(),
            "ambiguous Windows path must be rejected: {path:?}"
        );
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
