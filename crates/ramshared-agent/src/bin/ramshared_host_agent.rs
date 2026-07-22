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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (broker, listen, tenant) = parse_args(&args).map_err(std::io::Error::other)?;
    let listener = TcpListener::bind(&listen)?;
    eprintln!("[host-agent] listening={listen} broker={broker} tenant={tenant}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle(stream, &broker, &tenant) {
                    eprintln!("[host-agent] request_error={error}");
                }
            }
            Err(error) => eprintln!("[host-agent] accept_error={error}"),
        }
    }
    Ok(())
}
