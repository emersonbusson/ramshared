#![allow(clippy::unwrap_used)]

use std::fs;
use std::process::{Command, ExitStatus, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_FILE_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn run_cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ramshared"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_json_decision(status: &ExitStatus, value: &serde_json::Value) {
    let decision = value
        .get("decision")
        .and_then(serde_json::Value::as_str)
        .unwrap();
    assert!(matches!(decision, "ready" | "blocked"));
    assert_eq!(
        status.code(),
        Some(if decision == "ready" { 0 } else { 1 }),
        "the process exit must agree with its reported decision"
    );
}

fn assert_text_decision(status: &ExitStatus, text: &str) {
    let decision = text
        .lines()
        .find_map(|line| line.strip_prefix("Decision: "))
        .unwrap();
    assert!(matches!(decision, "ready" | "blocked"));
    assert_eq!(
        status.code(),
        Some(if decision == "ready" { 0 } else { 1 }),
        "the process exit must agree with its reported decision"
    );
}

#[test]
fn cli_help_and_unknown_command() {
    let help = run_cli(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(stderr(&help).contains("usage:"));

    let unknown = run_cli(&["not-a-command"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("unsupported command: not-a-command"));
    assert!(stderr(&unknown).contains("usage:"));
}

#[test]
fn cli_check_and_doctor_report_decision_json_and_text() {
    let check_json = run_cli(&["check", "--json"]);
    let check_value: serde_json::Value = serde_json::from_slice(&check_json.stdout).unwrap();
    assert_json_decision(&check_json.status, &check_value);

    let check_text = run_cli(&["check"]);
    assert_text_decision(&check_text.status, &stdout(&check_text));

    let doctor_json = run_cli(&["doctor", "--json"]);
    let doctor_value: serde_json::Value = serde_json::from_slice(&doctor_json.stdout).unwrap();
    let doctor_check = doctor_value.get("check").unwrap();
    assert_json_decision(&doctor_json.status, doctor_check);
    assert!(
        doctor_value
            .get("recommendations")
            .and_then(serde_json::Value::as_array)
            .is_some()
    );

    let doctor_text = run_cli(&["doctor"]);
    assert_text_decision(&doctor_text.status, &stdout(&doctor_text));
    assert!(stdout(&doctor_text).contains("Recommendations:"));
}

#[test]
fn cli_status_flags_are_exact_and_read_only() {
    let status_json = run_cli(&["status", "--json"]);
    assert_eq!(status_json.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&status_json.stdout).unwrap();
    assert_eq!(
        value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64),
        Some(4)
    );
    assert!(
        value
            .get("phase")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
    assert!(
        value
            .get("order_ok")
            .and_then(serde_json::Value::as_bool)
            .is_some()
    );
    for plane in [
        "control_state",
        "cache_state",
        "origin_state",
        "guardian_state",
        "overall_state",
    ] {
        assert!(
            value
                .get(plane)
                .and_then(serde_json::Value::as_str)
                .is_some()
        );
    }

    for args in [
        &["status", "--unsafe"][..],
        &["status", "--json", "--json"][..],
    ] {
        let refusal = run_cli(args);
        assert_eq!(refusal.status.code(), Some(2));
        assert!(stderr(&refusal).contains("invalid status option"));
        assert!(refusal.stdout.is_empty());
    }
}

#[test]
fn cli_monitor_once_emits_typed_read_only_observation() {
    let output = run_cli(&["monitor", "--jsonl", "--once"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 4);
    assert_eq!(value["sample_age_ms"], 0);
    assert!(value["mem"]["available_kib"].as_u64().is_some());
    assert!(value["control_plane"]["memory_psi_full_avg10"].is_number());
    assert!(value["control_plane"]["memory_psi_full_avg60"].is_number());
    assert!(value["control_plane"]["memory_psi_full_avg300"].is_number());
    assert!(value["tiers"]["vram"]["used_kib"].as_u64().is_some());
    assert!(value["activation"].get("disk_growth_kib").is_some());
}

#[test]
fn cli_diagnose_forwards_event_args() {
    let suffix = TEMP_FILE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "ramshared-cli-dispatch-{}-{suffix}.jsonl",
        std::process::id()
    ));
    fs::write(&path, "{\"t\":7,\"canario_demotes\":1,\"flag\":\"none\"}\n").unwrap();

    let path_string = path.to_string_lossy().into_owned();
    let output = run_cli(&["diagnose", "--events", &path_string, "--json"]);
    let _ = fs::remove_file(&path);

    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value.get("samples").and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        value.get("demotes").and_then(serde_json::Value::as_u64),
        Some(1)
    );
}

#[test]
fn cli_up_and_down_refuse_before_mutation() {
    let up_refusal = run_cli(&["up", "--zram", "invalid"]);
    assert_eq!(up_refusal.status.code(), Some(1));
    assert!(!stderr(&up_refusal).is_empty());

    let down_refusal = run_cli(&["down", "--force"]);
    assert_eq!(down_refusal.status.code(), Some(2));
    assert!(stderr(&down_refusal).contains("invalid down option"));
}
