#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
use crate::model::{Slice, SliceState};
use std::io::Cursor;

fn rt(msg: &Msg) -> Msg {
    let mut buf = Vec::new();
    write_msg(&mut buf, msg).unwrap();
    assert_eq!(buf.last(), Some(&b'\n'), "must end in a single line");
    let mut cur = Cursor::new(buf);
    read_msg(&mut cur).unwrap().unwrap()
}

#[test]
fn test_protocol_error_source_none() {
    use std::error::Error;
    let e = ProtocolError::ConnectionClosed(std::io::Error::other("foo"));
    assert!(e.source().is_some());

    let e = ProtocolError::BadMagic("test".into());
    assert!(e.source().is_none());

    let e = ProtocolError::UnsupportedVersion(42);
    assert!(e.source().is_none());

    let e = ProtocolError::PayloadTooLarge;
    assert!(e.source().is_none());
}

#[test]
fn test_protocol_write_io_error_connection_closed() {
    struct FailingWriter;
    impl std::io::Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("write error"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut w = FailingWriter;
    let res = write_msg(&mut w, &Msg::Ack);
    assert!(matches!(
        res,
        Err(ProtocolError::ConnectionClosed(_))
    ));
}

#[test]
fn test_protocol_write_flush_error_connection_closed() {
    struct FailingFlushWriter;
    impl std::io::Write for FailingFlushWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("flush error"))
        }
    }
    let mut w = FailingFlushWriter;
    let res = write_msg(&mut w, &Msg::Ack);
    assert!(matches!(
        res,
        Err(ProtocolError::ConnectionClosed(_))
    ));
}

#[test]
fn test_protocol_read_io_error_connection_closed() {
    struct FailingReader;
    impl std::io::Read for FailingReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("read error"))
        }
    }
    impl std::io::BufRead for FailingReader {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            Err(std::io::Error::other("read error"))
        }
        fn consume(&mut self, _: usize) {}
    }
    let mut r = FailingReader;
    let res = read_msg(&mut r);
    assert!(matches!(
        res,
        Err(ProtocolError::ConnectionClosed(_))
    ));
}

#[test]
fn protocol_error_display() {
    let e = ProtocolError::BadMagic("test".into());
    assert_eq!(e.to_string(), "bad magic: test");

    let e = ProtocolError::UnsupportedVersion(42);
    assert_eq!(e.to_string(), "unsupported version: 42");

    let e = ProtocolError::PayloadTooLarge;
    assert_eq!(e.to_string(), "payload too large (exceeds MAX_LINE_BYTES)");

    let e = ProtocolError::ConnectionClosed(std::io::Error::other("foo"));
    assert_eq!(e.to_string(), "connection closed: foo");
}

#[test]
fn roundtrip_each_variant() {
    use crate::model::TransportKind;
    use crate::model::PsiSample;
    let msgs = vec![
        Msg::Register {
            header: VersionHeader {
                proto: PROTO_VERSION,
                features: vec![],
            },
            tenant: "wsl2".into(),
            transport: TransportKind::NbdTcp,
        },
        Msg::Psi {
            sample: PsiSample {
                avg10: 1.0,
                avg60: 2.0,
                stall_us: 3,
            },
            swaps: vec![SwapEntry {
                dev: "/dev/nbd0".into(),
                prio: -2,
                size_kb: 100,
                used_kb: 10,
            }],
            mem: Some(TenantMem {
                swap_current: Some(2048),
                diskstats_io: 100,
            }),
        },
        Msg::SwapOnDone {
            slice: 1,
            ok: true,
            detail: "ok".into(),
        },
        Msg::SwapOffDone {
            slice: 1,
            ok: false,
            detail: "eio".into(),
        },
        Msg::LeaseRequest { bytes: 1 << 20 },
        Msg::LeaseRelease { lease: 4 },
        Msg::Status,
        Msg::Registered { tenant_id: 2, features: vec![] },
        Msg::Ack,
        Msg::SwapOn {
            slice: 0,
            export: "s0".into(),
            endpoint: NbdEndpoint::Tcp {
                host: "10.0.0.1".into(),
                port: 10809,
            },
            swap_prio: None,
        },
        Msg::SwapOff { slice: 0 },
        Msg::DemoteAll,
        Msg::LeaseGranted {
            lease: 4,
            bytes: 1 << 20,
        },
        Msg::LeaseDenied {
            reason: "lease_em_andamento".into(),
        },
        Msg::StatusReply {
            tenants: vec![TenantStatus {
                id: 1,
                name: "wsl2".into(),
                psi: PsiSample::default(),
                slices: vec![0, 1],
                present: true,
                bytes_served: 4096,
            }],
            slices: vec![Slice {
                id: 0,
                offset: 0,
                len: 64,
                tenant: Some(1),
                state: SliceState::Active,
            }],
            slice_io: vec![SliceIo {
                id: 0,
                bytes_served: 4096,
                io_count: 1,
            }],
            last_rebalance_secs: Some(42),
        },
        Msg::Error { reason: "x".into() },
    ];
    for m in &msgs {
        assert_eq!(&rt(m), m);
    }
}

#[test]
fn nbd_endpoint_unix_roundtrips() {
    let m = Msg::SwapOn {
        slice: 2,
        export: "s2".into(),
        endpoint: NbdEndpoint::Unix {
            path: "/run/x.sock".into(),
        },
        swap_prio: Some(-3),
    };
    assert_eq!(rt(&m), m);
}

#[test]
fn eof_is_none() {
    let mut cur = Cursor::new(Vec::new());
    assert!(read_msg(&mut cur).unwrap().is_none());
}

#[test]
fn missing_type_tag_is_err() {
    let mut cur = Cursor::new(b"{\"foo\":1}\n".to_vec());
    assert!(read_msg(&mut cur).is_err());
}

#[test]
fn psi_mem_defaults_to_none() {
    // Psi without `mem` (previous format) → deserializes with mem=None (additive, DT-9).
    let line =
        b"{\"type\":\"psi\",\"sample\":{\"avg10\":0.0,\"avg60\":0.0,\"stall_us\":0},\"swaps\":[]}\n";
    let mut cur = Cursor::new(line.to_vec());
    match read_msg(&mut cur).unwrap().unwrap() {
        Msg::Psi { mem, .. } => assert_eq!(mem, None),
        other => panic!("expected Psi, got {other:?}"),
    }
}

#[test]
fn status_reply_slice_io_defaults_empty() {
    // StatusReply without `slice_io` → empty vector (additive).
    let line = b"{\"type\":\"status_reply\",\"tenants\":[],\"slices\":[],\"last_rebalance_secs\":null}\n";
    let mut cur = Cursor::new(line.to_vec());
    match read_msg(&mut cur).unwrap().unwrap() {
        Msg::StatusReply { slice_io, .. } => assert!(slice_io.is_empty()),
        other => panic!("expected StatusReply, got {other:?}"),
    }
}

#[test]
fn oversize_line_is_err() {
    // Line > MAX_LINE_BYTES without '\n' within the cap → Err (does not try to parse giant).
    let mut data = vec![b'x'; MAX_LINE_BYTES + 100];
    data.push(b'\n');
    let mut cur = Cursor::new(data);
    assert!(read_msg(&mut cur).is_err());
}

#[test]
fn oversize_line_with_newline_is_err() {
    // If a line is exactly MAX_LINE_BYTES + 1, ending with a newline.
    // It must be rejected, because it exceeds MAX_LINE_BYTES.
    let mut data = vec![b'x'; MAX_LINE_BYTES + 1];
    data[MAX_LINE_BYTES] = b'\n';
    let mut cur = Cursor::new(data);
    assert!(read_msg(&mut cur).is_err());
}

#[test]
fn two_messages_one_stream() {
    let mut buf = Vec::new();
    write_msg(&mut buf, &Msg::Ack).unwrap();
    write_msg(&mut buf, &Msg::DemoteAll).unwrap();
    let mut cur = Cursor::new(buf);
    assert_eq!(read_msg(&mut cur).unwrap().unwrap(), Msg::Ack);
    assert_eq!(read_msg(&mut cur).unwrap().unwrap(), Msg::DemoteAll);
    assert!(read_msg(&mut cur).unwrap().is_none());
}

#[test]
fn unknown_type_parses_as_unknown() {
    let mut cur = Cursor::new(b"{\"type\":\"bogus\"}\n".to_vec());
    let res = read_msg(&mut cur).unwrap().unwrap();
    assert!(matches!(res, Msg::Unknown));
}
