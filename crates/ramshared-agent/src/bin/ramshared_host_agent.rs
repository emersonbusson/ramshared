//! Native Windows host lease bridge.
//!
//! The binary is intentionally usable as a loopback service on Windows. It
//! does not expose swap commands to clients; it registers as `DccAgent` and
//! forwards only lease/status messages to the broker.
#![forbid(unsafe_code)]

use std::io::{BufReader, BufWriter};
use std::net::{TcpListener, TcpStream};

use ramshared_agent::local::{LocalMsg, LocalReply, read_json_line, write_json_line};
use ramshared_broker::model::TransportKind;
use ramshared_broker::protocol::{Msg, PROTO_VERSION, read_msg, write_msg};

fn usage() -> &'static str {
    "ramshared-host-agent --broker HOST:PORT [--listen HOST:PORT] [--tenant NAME]"
}

fn parse_args(args: &[String]) -> Result<(String, String, String), String> {
    let mut broker = None;
    let mut listen = "127.0.0.1:7788".to_string();
    let mut tenant = "dcc".to_string();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let value = |name: &str, it: &mut std::slice::Iter<'_, String>| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match arg.as_str() {
            "--broker" => broker = Some(value("--broker", &mut it)?),
            "--listen" => listen = value("--listen", &mut it)?,
            "--tenant" => tenant = value("--tenant", &mut it)?,
            "-h" | "--help" => return Err(usage().into()),
            other => return Err(format!("unknown argument: {other}\n{}", usage())),
        }
    }
    Ok((
        broker.ok_or_else(|| format!("--broker is required\n{}", usage()))?,
        listen,
        tenant,
    ))
}

fn connect_broker(
    addr: &str,
    tenant: &str,
) -> Result<(BufReader<TcpStream>, BufWriter<TcpStream>), String> {
    let stream = TcpStream::connect(addr).map_err(|e| format!("broker connect: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let reader_stream = stream.try_clone().map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(stream);
    write_msg(
        &mut writer,
        &Msg::Register {
            proto: PROTO_VERSION,
            tenant: tenant.into(),
            transport: TransportKind::DccAgent,
        },
    )
    .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(reader_stream);
    loop {
        match read_msg(&mut reader).map_err(|e| e.to_string())? {
            Some(Msg::Registered { .. }) => return Ok((reader, writer)),
            Some(Msg::Error { reason }) => return Err(reason),
            Some(_) => continue,
            None => return Err("broker closed during register".into()),
        }
    }
}

fn handle(local: TcpStream, broker_addr: &str, tenant: &str) -> Result<(), String> {
    local
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let reader_stream = local.try_clone().map_err(|e| e.to_string())?;
    let mut input = BufReader::new(reader_stream);
    let mut output = BufWriter::new(local);
    let Some(request) = read_json_line::<_, LocalMsg>(&mut input).map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let (mut broker_in, mut broker_out) = connect_broker(broker_addr, tenant)?;
    let message = match request {
        LocalMsg::Status => Msg::Status,
        LocalMsg::LeaseRequest { bytes, .. } => Msg::LeaseRequest { bytes },
        LocalMsg::LeaseRelease { lease } => Msg::LeaseRelease { lease },
    };
    write_msg(&mut broker_out, &message).map_err(|e| e.to_string())?;
    loop {
        let Some(reply) = read_msg(&mut broker_in).map_err(|e| e.to_string())? else {
            return Err("broker closed without a reply".into());
        };
        let local_reply = match reply {
            Msg::LeaseGranted { lease, bytes } => LocalReply::LeaseGranted { lease, bytes },
            Msg::LeaseDenied { reason } => LocalReply::LeaseDenied { reason },
            Msg::StatusReply {
                last_rebalance_secs,
                ..
            } => LocalReply::Status {
                vram_free: None,
                vram_total: None,
                lease: None,
                evidence: vec![format!("last_rebalance_secs={last_rebalance_secs:?}")],
            },
            Msg::Ack => continue,
            Msg::Error { reason } => LocalReply::Error { reason },
            _ => continue,
        };
        write_json_line(&mut output, &local_reply).map_err(|e| e.to_string())?;
        return Ok(());
    }
}

/// Serves every accepted connection through `handle`, logging request and
/// accept failures without ending the service. Taking the accept stream as an
/// iterator keeps the lifecycle testable without a live listener.
fn serve(incoming: impl Iterator<Item = std::io::Result<TcpStream>>, broker: &str, tenant: &str) {
    for stream in incoming {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle(stream, broker, tenant) {
                    eprintln!("[host-agent] request_error={error}");
                }
            }
            Err(error) => eprintln!("[host-agent] accept_error={error}"),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (broker, listen, tenant) = parse_args(&args).map_err(std::io::Error::other)?;
    let listener = TcpListener::bind(&listen)?;
    eprintln!("[host-agent] listening={listen} broker={broker} tenant={tenant}");
    serve(listener.incoming(), &broker, &tenant);
    Ok(())
}

#[cfg(test)]
mod tests {
    // unwrap/expect allowed in tests only (coding.md rules).
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::thread;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    fn loopback() -> (TcpListener, String) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test listener must bind");
        let addr = listener.local_addr().expect("addr").to_string();
        (listener, addr)
    }

    /// Accepts one broker connection and answers the register handshake with
    /// `reply`, then serves `after` on the same stream.
    fn fake_broker(
        listener: TcpListener,
        reply: Msg,
        after: impl FnOnce(&mut TcpStream) + Send + 'static,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("client must connect");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let got = read_msg(&mut reader).expect("register must decode");
            assert!(
                matches!(got, Some(Msg::Register { .. })),
                "first frame must be Register, got {got:?}"
            );
            write_msg(&mut stream, &reply).expect("reply must write");
            after(&mut stream);
        })
    }

    #[test]
    fn usage_names_every_required_flag() {
        let u = usage();
        assert!(u.contains("ramshared-host-agent"));
        assert!(u.contains("--broker"));
        assert!(u.contains("--listen"));
        assert!(u.contains("--tenant"));
    }

    #[test]
    fn parse_args_applies_defaults_and_reads_every_flag() {
        let (broker, listen, tenant) = parse_args(&args(&["--broker", "127.0.0.1:9000"])).unwrap();
        assert_eq!(broker, "127.0.0.1:9000");
        assert_eq!(listen, "127.0.0.1:7788");
        assert_eq!(tenant, "dcc");

        let (broker, listen, tenant) = parse_args(&args(&[
            "--broker", "b:1", "--listen", "l:2", "--tenant", "t3",
        ]))
        .unwrap();
        assert_eq!(broker, "b:1");
        assert_eq!(listen, "l:2");
        assert_eq!(tenant, "t3");
    }

    #[test]
    fn parse_args_rejects_missing_broker_unknown_flag_and_truncated_values() {
        let e = parse_args(&args(&["--listen", "l:2"])).unwrap_err();
        assert!(e.contains("--broker is required"), "got: {e}");

        let e = parse_args(&args(&["--broker", "b:1", "--nope"])).unwrap_err();
        assert!(e.contains("unknown argument: --nope"), "got: {e}");

        for flag in ["--broker", "--listen", "--tenant"] {
            let e = parse_args(&args(&[flag])).unwrap_err();
            assert!(e.contains(&format!("{flag} requires a value")), "got: {e}");
        }
    }

    #[test]
    fn parse_args_help_returns_usage_as_the_error() {
        for flag in ["-h", "--help"] {
            let e = parse_args(&args(&[flag])).unwrap_err();
            assert_eq!(e, usage());
        }
    }

    #[test]
    fn connect_broker_registers_and_returns_the_pair() {
        let (listener, addr) = loopback();
        let server = fake_broker(listener, Msg::Registered { tenant_id: 7 }, |_| {});

        let (mut reader, mut writer) = connect_broker(&addr, "dcc").unwrap();
        // The pair is live: a second round-trip on the same socket works.
        write_msg(&mut writer, &Msg::Status).unwrap();
        write_msg(&mut reader.get_mut(), &Msg::Ack).ok();
        server.join().unwrap();
    }

    #[test]
    fn connect_broker_surfaces_register_refusal_and_close() {
        let (listener, addr) = loopback();
        let server = fake_broker(
            listener,
            Msg::Error {
                reason: "tenant refused".into(),
            },
            |_| {},
        );
        let e = connect_broker(&addr, "dcc").unwrap_err();
        server.join().unwrap();
        assert_eq!(e, "tenant refused");

        let (listener, addr) = loopback();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("client must connect");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let _ = read_msg(&mut reader).expect("register must decode");
            // Close without ever sending Registered.
            drop(stream);
        });
        let e = connect_broker(&addr, "dcc").unwrap_err();
        server.join().unwrap();
        assert!(e.contains("broker closed during register"), "got: {e}");
    }

    #[test]
    fn connect_broker_reports_unreachable_broker() {
        let e = connect_broker("127.0.0.1:1", "dcc").unwrap_err();
        assert!(e.contains("broker connect:"), "got: {e}");
    }

    /// Spawns the local accept side of `handle` and returns a connected client
    /// plus the join handle of the handler thread.
    fn serve_local(
        broker_addr: &str,
        tenant: &str,
    ) -> (TcpStream, thread::JoinHandle<Result<(), String>>) {
        let (listener, _) = loopback();
        let addr = listener.local_addr().expect("addr");
        let broker = broker_addr.to_string();
        let tenant = tenant.to_string();
        let handler = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("local client must connect");
            handle(stream, &broker, &tenant)
        });
        let client = TcpStream::connect(addr).expect("client must connect");
        (client, handler)
    }

    fn write_local(client: &TcpStream, msg: &LocalMsg) {
        let mut writer = BufWriter::new(client.try_clone().expect("clone"));
        write_json_line(&mut writer, msg).expect("local request must write");
    }

    fn read_local(client: &TcpStream) -> LocalReply {
        let mut reader = BufReader::new(client.try_clone().expect("clone"));
        read_json_line::<_, LocalReply>(&mut reader)
            .expect("local reply must decode")
            .expect("local reply must be present")
    }

    #[test]
    fn handle_closes_quietly_on_empty_local_request() {
        let (client, handler) = serve_local("127.0.0.1:1", "dcc");
        drop(client);
        assert!(handler.join().unwrap().is_ok());
    }

    #[test]
    fn handle_forwards_status_and_reports_rebalance() {
        let (listener, broker) = loopback();
        let server = fake_broker(listener, Msg::Registered { tenant_id: 1 }, |stream| {
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let got = read_msg(&mut reader).expect("forwarded frame");
            assert!(matches!(got, Some(Msg::Status)), "got {got:?}");
            write_msg(
                stream,
                &Msg::StatusReply {
                    tenants: Vec::new(),
                    slices: Vec::new(),
                    slice_io: Vec::new(),
                    last_rebalance_secs: Some(42),
                },
            )
            .expect("status reply must write");
        });

        let (client, handler) = serve_local(&broker, "dcc");
        write_local(&client, &LocalMsg::Status);
        let reply = read_local(&client);
        server.join().unwrap();
        handler.join().unwrap().unwrap();

        match reply {
            LocalReply::Status {
                vram_free,
                vram_total,
                lease,
                evidence,
            } => {
                assert!(vram_free.is_none() && vram_total.is_none() && lease.is_none());
                assert_eq!(evidence, vec!["last_rebalance_secs=Some(42)"]);
            }
            other => panic!("expected Status reply, got {other:?}"),
        }
    }

    #[test]
    fn handle_forwards_lease_lifecycle_and_ignores_acks() {
        let (listener, broker) = loopback();
        let server = fake_broker(listener, Msg::Registered { tenant_id: 1 }, |stream| {
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let got = read_msg(&mut reader).expect("forwarded frame");
            assert!(
                matches!(got, Some(Msg::LeaseRequest { bytes: 4096 })),
                "got {got:?}"
            );
            // An Ack must not be mistaken for the reply.
            write_msg(stream, &Msg::Ack).expect("ack must write");
            write_msg(
                stream,
                &Msg::LeaseGranted {
                    lease: 9,
                    bytes: 4096,
                },
            )
            .expect("granted must write");
        });

        let (client, handler) = serve_local(&broker, "dcc");
        write_local(
            &client,
            &LocalMsg::LeaseRequest {
                bytes: 4096,
                client: "game".into(),
            },
        );
        let reply = read_local(&client);
        server.join().unwrap();
        handler.join().unwrap().unwrap();
        assert!(
            matches!(
                reply,
                LocalReply::LeaseGranted {
                    lease: 9,
                    bytes: 4096
                }
            ),
            "got {reply:?}"
        );

        let (listener, broker) = loopback();
        let server = fake_broker(listener, Msg::Registered { tenant_id: 1 }, |stream| {
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let got = read_msg(&mut reader).expect("forwarded frame");
            assert!(
                matches!(got, Some(Msg::LeaseRelease { lease: 9 })),
                "got {got:?}"
            );
            write_msg(
                stream,
                &Msg::LeaseDenied {
                    reason: "not held".into(),
                },
            )
            .expect("denied must write");
        });

        let (client, handler) = serve_local(&broker, "dcc");
        write_local(&client, &LocalMsg::LeaseRelease { lease: 9 });
        let reply = read_local(&client);
        server.join().unwrap();
        handler.join().unwrap().unwrap();
        let shown = format!("{reply:?}");
        assert!(
            matches!(reply, LocalReply::LeaseDenied { reason } if reason == "not held"),
            "got {shown}"
        );
    }

    #[test]
    fn handle_reports_broker_error_and_missing_reply() {
        let (listener, broker) = loopback();
        let server = fake_broker(listener, Msg::Registered { tenant_id: 1 }, |stream| {
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let _ = read_msg(&mut reader).expect("forwarded frame");
            write_msg(
                stream,
                &Msg::Error {
                    reason: "broker broke".into(),
                },
            )
            .expect("error must write");
        });

        let (client, handler) = serve_local(&broker, "dcc");
        write_local(&client, &LocalMsg::Status);
        let reply = read_local(&client);
        server.join().unwrap();
        handler.join().unwrap().unwrap();
        let shown = format!("{reply:?}");
        assert!(
            matches!(reply, LocalReply::Error { reason } if reason == "broker broke"),
            "got {shown}"
        );

        let (listener, broker) = loopback();
        let server = fake_broker(listener, Msg::Registered { tenant_id: 1 }, |stream| {
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let _ = read_msg(&mut reader).expect("forwarded frame");
            stream
                .shutdown(std::net::Shutdown::Both)
                .expect("close must succeed");
        });

        let (client, handler) = serve_local(&broker, "dcc");
        write_local(&client, &LocalMsg::Status);
        let e = handler.join().unwrap().unwrap_err();
        server.join().unwrap();
        drop(client);
        assert!(e.contains("broker closed without a reply"), "got: {e}");
    }

    #[test]
    fn connect_broker_skips_unexpected_frames_until_registered() {
        let (listener, addr) = loopback();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("client must connect");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let got = read_msg(&mut reader).expect("register must decode");
            assert!(matches!(got, Some(Msg::Register { .. })), "got {got:?}");
            // A frame that is not Registered must be skipped, not treated as
            // the handshake answer.
            write_msg(&mut stream, &Msg::Ack).expect("ack must write");
            write_msg(&mut stream, &Msg::Registered { tenant_id: 3 })
                .expect("registered must write");
        });
        let (mut reader, mut writer) = connect_broker(&addr, "dcc").unwrap();
        server.join().unwrap();
        // The pair is live after the skip: a further frame still decodes.
        write_msg(&mut writer, &Msg::Status).expect("status must write");
        let _ = &mut reader;
    }

    #[test]
    fn handle_skips_unexpected_broker_replies_until_a_known_one() {
        let (listener, broker) = loopback();
        let server = fake_broker(listener, Msg::Registered { tenant_id: 1 }, |stream| {
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let got = read_msg(&mut reader).expect("forwarded frame");
            assert!(matches!(got, Some(Msg::Status)), "got {got:?}");
            // Not a reply this bridge translates; it must be skipped.
            write_msg(stream, &Msg::DemoteAll).expect("unexpected frame must write");
            write_msg(stream, &Msg::LeaseGranted { lease: 5, bytes: 1 })
                .expect("granted must write");
        });

        let (client, handler) = serve_local(&broker, "dcc");
        write_local(&client, &LocalMsg::Status);
        let reply = read_local(&client);
        server.join().unwrap();
        handler.join().unwrap().unwrap();
        let shown = format!("{reply:?}");
        assert!(
            matches!(reply, LocalReply::LeaseGranted { lease: 5, bytes: 1 }),
            "got {shown}"
        );
    }

    #[test]
    fn serve_accepts_ok_streams_logs_accept_errors_and_survives_request_failures() {
        let (listener, broker) = loopback();
        let server = fake_broker(listener, Msg::Registered { tenant_id: 1 }, |stream| {
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let _ = read_msg(&mut reader).expect("forwarded frame");
            write_msg(
                stream,
                &Msg::Error {
                    reason: "late failure".into(),
                },
            )
            .expect("error must write");
        });

        // Pair a client with the stream `serve` will accept, so the Ok arm
        // runs a real request that fails at the broker and is only logged.
        let (local_listener, local_addr) = loopback();
        let client = TcpStream::connect(local_addr).expect("client must connect");
        let (accepted, _) = local_listener
            .accept()
            .expect("serve must accept the client");
        write_local(&client, &LocalMsg::Status);
        let err_stream: std::io::Result<TcpStream> =
            Err(std::io::Error::other("synthetic accept failure"));
        serve(vec![Ok(accepted), err_stream].into_iter(), &broker, "dcc");
        server.join().unwrap();

        // The failure was reported to the local client as an Error reply.
        let mut reader = BufReader::new(client.try_clone().expect("clone"));
        let reply = read_json_line::<_, LocalReply>(&mut reader)
            .expect("local reply must decode")
            .expect("local reply must be present");
        let shown = format!("{reply:?}");
        assert!(
            matches!(reply, LocalReply::Error { reason } if reason == "late failure"),
            "got {shown}"
        );
    }
}
