//! Read-only discovery and display for the cross-platform resource settings UI.

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;

    #[test]
    fn platform_detection_distinguishes_native_linux_from_wsl2() {
        assert_eq!(
            classify_platform("6.18.40.1-microsoft-standard-WSL2", ""),
            RuntimePlatform::Wsl2
        );
        assert_eq!(
            classify_platform("6.8.0-45-generic", "Linux version 6.8.0-45-generic"),
            RuntimePlatform::NativeLinux
        );
    }

    #[test]
    fn meminfo_accepts_user_sized_ram_and_swap_without_product_minima() {
        // Deliberately varied parser fixtures; these are not product defaults.
        let small = parse_meminfo(
            "MemTotal: 262144 kB\nMemAvailable: 196608 kB\nSwapTotal: 131072 kB\nSwapFree: 65536 kB\n",
        );
        let large = parse_meminfo(
            "MemTotal: 50331648 kB\nMemAvailable: 40265318 kB\nSwapTotal: 20971520 kB\nSwapFree: 15728640 kB\n",
        );

        assert!(small.required_counters_available);
        assert!(large.required_counters_available);
        assert_eq!(small.total_bytes, Some(262_144 * 1024));
        assert_eq!(large.swap_total_bytes, Some(20_971_520 * 1024));
    }

    #[test]
    fn meminfo_missing_or_inconsistent_counters_are_unavailable() {
        let missing = parse_meminfo("MemTotal: 4096 kB\n");
        let inconsistent = parse_meminfo(
            "MemTotal: 4096 kB\nMemAvailable: 8192 kB\nSwapTotal: 4096 kB\nSwapFree: 8192 kB\n",
        );

        assert!(!missing.required_counters_available);
        assert!(!inconsistent.required_counters_available);
        assert_eq!(inconsistent.total_bytes, None);
        assert_eq!(inconsistent.swap_total_bytes, None);
    }

    #[test]
    fn swap_parser_keeps_all_devices_and_variable_capacities() {
        let swaps = parse_swap_table(
            "Filename Type Size Used Priority\n/dev/zram0 partition 524288 32768 100\n/mnt/c/swap\\040file file 12582912 0 -2\n",
        )
        .expect("valid swap inventory");

        assert_eq!(swaps.len(), 2);
        assert_eq!(swaps[0].size_kib, 524_288);
        assert_eq!(swaps[1].filename, "/mnt/c/swap\\040file");
        assert_eq!(swaps[1].size_kib, 12_582_912);
    }

    #[test]
    fn linux_block_inventory_preserves_mounted_and_unmounted_devices() {
        let devices = parse_lsblk_json(
            r#"{"blockdevices":[{"name":"nvme0n1","path":"/dev/nvme0n1","type":"disk","size":2000000000000,"mountpoints":[null],"children":[{"name":"nvme0n1p1","path":"/dev/nvme0n1p1","type":"part","size":1000000000000,"fstype":"ext4","uuid":"fs-uuid","mountpoints":["/data"],"pkname":"nvme0n1"}]},{"name":"sda","path":"/dev/sda","type":"disk","size":500000000000,"mountpoints":[null]}]}"#,
        )
        .expect("valid lsblk response");

        assert_eq!(devices.len(), 3);
        assert!(devices.iter().any(|device| device.name == "nvme0n1"));
        let unmounted = devices
            .iter()
            .find(|device| device.name == "sda")
            .expect("unmounted disk is visible");
        assert!(unmounted.mountpoints.is_empty());
        let mounted = devices
            .iter()
            .find(|device| device.name == "nvme0n1p1")
            .expect("mounted filesystem is visible");
        assert_eq!(mounted.filesystem.as_deref(), Some("ext4"));
        assert_eq!(mounted.mountpoints, vec!["/data"]);
        assert!(mounted.eligible_for_file_storage);
    }

    #[test]
    fn windows_snapshot_lists_multiple_host_volumes_and_live_memory() {
        let snapshot = parse_windows_snapshot(
            r#"{"observed_utc":"2026-09-27T22:00:00Z","host_memory":{"total_bytes":34359738368,"free_bytes":17179869184,"committed_bytes":25769803776,"commit_limit_bytes":60129542144},"volumes":[{"drive_letter":"C","label":"System","file_system":"NTFS","drive_type":"Fixed","size_bytes":500000000000,"free_bytes":200000000000,"volume_id":"vol-c"},{"drive_letter":"I","label":"Data","file_system":"NTFS","drive_type":"Fixed","size_bytes":1000000000000,"free_bytes":600000000000,"volume_id":"vol-i"}]}"#,
        )
        .expect("valid Windows snapshot");

        assert_eq!(snapshot.host_memory.free_bytes, Some(17_179_869_184));
        assert_eq!(snapshot.volumes.len(), 2);
        assert_eq!(snapshot.volumes[0].drive_letter.as_deref(), Some("C"));
        assert_eq!(snapshot.volumes[1].drive_letter.as_deref(), Some("I"));
    }

    #[test]
    fn resource_view_labels_guest_and_windows_host_memory_separately() {
        let snapshot = fixture_snapshot();
        let output = render_text(&snapshot);

        assert!(output.contains("WSL guest RAM"));
        assert!(output.contains("Windows host RAM"));
        assert!(output.contains("Windows commit headroom"));
        assert!(output.contains("C:"));
        assert!(output.contains("I:"));
    }

    #[test]
    fn resource_view_keeps_native_linux_memory_out_of_windows_scope() {
        let mut snapshot = fixture_snapshot();
        snapshot.platform = RuntimePlatform::NativeLinux;
        snapshot.windows = None;
        let output = render_text(&snapshot);

        assert!(output.contains("System RAM"));
        assert!(!output.contains("Windows host RAM"));
    }

    #[test]
    fn malformed_device_and_windows_payloads_fail_closed() {
        assert!(parse_lsblk_json("not json").is_err());
        assert!(parse_swap_table("Filename Type Size Used Priority\nbad row\n").is_err());
        assert!(parse_windows_snapshot("[]").is_err());
    }

    #[test]
    fn current_guest_config_show_is_read_only_before_and_after() {
        let swaps_before = std::fs::read_to_string("/proc/swaps").expect("guest swap table");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let exit = run(
            ConfigMode::Show { json: true },
            &mut stdout,
            &mut stderr,
        );

        let swaps_after = std::fs::read_to_string("/proc/swaps").expect("guest swap table");
        assert_eq!(exit, std::process::ExitCode::SUCCESS);
        assert_eq!(swaps_after, swaps_before);
        let json: serde_json::Value =
            serde_json::from_slice(&stdout).expect("resource snapshot JSON");
        assert!(json.get("platform").is_some());
        assert!(json.get("guest_memory").is_some());
    }

    fn fixture_snapshot() -> ResourceSnapshot {
        ResourceSnapshot {
            platform: RuntimePlatform::Wsl2,
            guest_memory: MemorySnapshot {
                total_bytes: Some(16 * 1024 * 1024 * 1024),
                available_bytes: Some(8 * 1024 * 1024 * 1024),
                swap_total_bytes: Some(4 * 1024 * 1024 * 1024),
                swap_free_bytes: Some(2 * 1024 * 1024 * 1024),
                required_counters_available: true,
            },
            swaps: Vec::new(),
            windows: Some(WindowsSnapshot {
                observed_utc: "2026-09-27T22:00:00Z".into(),
                host_memory: HostMemorySnapshot {
                    total_bytes: Some(32 * 1024 * 1024 * 1024),
                    free_bytes: Some(16 * 1024 * 1024 * 1024),
                    committed_bytes: Some(24 * 1024 * 1024 * 1024),
                    commit_limit_bytes: Some(56 * 1024 * 1024 * 1024),
                },
                volumes: vec![
                    WindowsVolume {
                        drive_letter: Some("C".into()),
                        label: Some("System".into()),
                        file_system: Some("NTFS".into()),
                        drive_type: "Fixed".into(),
                        size_bytes: Some(500_000_000_000),
                        free_bytes: Some(200_000_000_000),
                        volume_id: Some("vol-c".into()),
                    },
                    WindowsVolume {
                        drive_letter: Some("I".into()),
                        label: Some("Data".into()),
                        file_system: Some("NTFS".into()),
                        drive_type: "Fixed".into(),
                        size_bytes: Some(1_000_000_000_000),
                        free_bytes: Some(600_000_000_000),
                        volume_id: Some("vol-i".into()),
                    },
                ],
            }),
            block_devices: Vec::new(),
            gpu_budget_status: "not sampled by storage inventory".into(),
        }
    }
}
