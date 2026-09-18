//! Public daemon refusal tests. These children exercise argv → `main` → exit
//! code only; each command is rejected before CUDA, Vulkan, swap, NBD, or ublk.

#![allow(clippy::expect_used)]

use std::process::Command;

fn daemon(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ramsharedd"))
        .args(args)
        .output()
        .expect("start ramsharedd refusal child")
}

#[test]
fn daemon_process_refusals_exit_before_backend() {
    for args in [
        ["--unknown"].as_slice(),
        ["--backend", "ram"].as_slice(),
        ["--transport", "ublk", "--backend", "vulkan"].as_slice(),
        ["--slices", "1", "--slice-mb", "1"].as_slice(),
    ] {
        let output = daemon(args);
        assert_eq!(output.status.code(), Some(1), "args={args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("[ramsharedd] error:"),
            "args={args:?}: {stderr}"
        );
    }
}

#[test]
fn daemon_process_refuses_origin_manifest_size_bounds() {
    for args in [
        ["--origin-manifest", "/etc/ramshared/origin.conf", "--size", "512"].as_slice(),
        ["--origin-manifest", "/etc/ramshared/origin.conf", "--size", "30000"].as_slice(),
    ] {
        let output = daemon(args);
        assert_eq!(output.status.code(), Some(1), "args={args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("origin-cache logical size must be between 1024 and 24576 MiB"),
            "args={args:?}: {stderr}"
        );
    }
}

#[test]
fn daemon_process_refuses_invalid_origin_manifest_path() {
    let args = ["--origin-manifest", "/tmp/invalid.json"].as_slice();
    let output = daemon(args);
    assert_eq!(output.status.code(), Some(1), "args={args:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--origin-manifest must use the sealed /etc/ramshared/origin.conf path"),
        "args={args:?}: {stderr}"
    );
}
