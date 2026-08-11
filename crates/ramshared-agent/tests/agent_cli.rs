#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use ramshared_broker::protocol::{Msg, read_msg, write_msg};

const AGENT: &str = env!("CARGO_BIN_EXE_ramshared-agent");

fn run_agent(args: &[&str]) -> Output {
    Command::new(AGENT)
        .args(args)
        .output()
        .expect("ramshared-agent child process must start")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn listener() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").expect("local test listener must bind")
}

fn status_request(stream: &TcpStream) {
    let mut reader = BufReader::new(stream.try_clone().expect("stream must clone"));
    assert_eq!(
        read_msg(&mut reader).expect("status request must decode"),
        Some(Msg::Status),
    );
}

#[test]
fn cli_help_prints_usage_and_exits_zero() {
    let output = run_agent(&["--help"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stdout).contains("Usage:"));
    assert!(output.stderr.is_empty());
}

#[test]
fn cli_missing_broker_refuses_with_exit_two() {
    let output = run_agent(&["--status"]);
    let stderr = text(&output.stderr);

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("--broker is required"));
    assert!(stderr.contains("Usage:"));
    assert!(output.stdout.is_empty());
}

#[test]
fn cli_invalid_transport_refuses_with_exit_two() {
    let output = run_agent(&["--broker", "127.0.0.1:9", "--status", "--transport", "rdma"]);
    let stderr = text(&output.stderr);

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("--transport is invalid: rdma (use tcp|unix)"));
    assert!(stderr.contains("Usage:"));
    assert!(output.stdout.is_empty());
}

#[test]
fn cli_missing_tenant_refuses_with_exit_two() {
    let output = run_agent(&["--broker", "127.0.0.1:9"]);
    let stderr = text(&output.stderr);

    assert_eq!(output.status.code(), Some(2));
    assert!(stderr.contains("--tenant is required in agent mode"));
    assert!(stderr.contains("Usage:"));
    assert!(output.stdout.is_empty());
}

#[test]
fn cli_status_reply_prints_public_status_and_exits_zero() {
    let listener = listener();
    let broker = listener
        .local_addr()
        .expect("listener address must be available")
        .to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("agent must connect");
        status_request(&stream);
        write_msg(
            &mut stream,
            &Msg::StatusReply {
                tenants: Vec::new(),
                slices: Vec::new(),
                slice_io: Vec::new(),
                last_rebalance_secs: None,
            },
        )
        .expect("status reply must write");
    });

    let output = run_agent(&["--broker", &broker, "--status"]);
    server.join().expect("status server must finish");
    let stdout = text(&output.stdout);

    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("tenants (0):"));
    assert!(stdout.contains("slices (0):"));
    assert!(stdout.contains("slice_io (0):"));
    assert!(stdout.contains("last_rebalance_secs=None"));
    assert!(output.stderr.is_empty());
}

#[test]
fn cli_status_broker_refusal_exits_one() {
    let listener = listener();
    let broker = listener
        .local_addr()
        .expect("listener address must be available")
        .to_string();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("agent must connect");
        status_request(&stream);
        write_msg(
            &mut stream,
            &Msg::Error {
                reason: "test broker refusal".to_string(),
            },
        )
        .expect("broker refusal must write");
    });

    let output = run_agent(&["--broker", &broker, "--status"]);
    server.join().expect("status server must finish");
    let stderr = text(&output.stderr);

    assert_eq!(output.status.code(), Some(1));
    assert!(stderr.contains("broker rejected status: test broker refusal"));
    assert!(output.stdout.is_empty());
}

#[test]
fn cli_status_timeout_exits_one_within_six_seconds() {
    let listener = listener();
    let broker = listener
        .local_addr()
        .expect("listener address must be available")
        .to_string();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("agent must connect");
        status_request(&stream);
        thread::sleep(Duration::from_secs(6));
    });

    let started = Instant::now();
    let output = run_agent(&["--broker", &broker, "--status"]);
    let elapsed = started.elapsed();
    server.join().expect("silent status server must finish");
    let stderr = text(&output.stderr);

    assert_eq!(output.status.code(), Some(1));
    assert!(elapsed <= Duration::from_secs(6));
    assert!(stderr.contains("broker status timed out after 5s"));
    assert!(output.stdout.is_empty());
}
