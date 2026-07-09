//! JSON-lines wire format of the agent↔broker protocol (RF-B1 / DT-1).
//!
//! One JSON object per line (`\n`, UTF-8). Low-rate control-plane (~1 msg/s/tenant),
//! debuggable with `nc`/`jq` (ADR-0005). The codec enforces line cap [`MAX_LINE_BYTES`] **before**
//! allocating (anti-DoS, mirrors NBD handshake `MAX_OPT_LEN`).

use std::io::{BufRead, Read, Write};

use crate::model::{PsiSample, Slice, SliceId, TenantId, TransportKind};

/// Protocol version; `Register` with `proto != PROTO_VERSION` is rejected by the broker (ITEM-8).
pub const PROTO_VERSION: u32 = 1;
/// Anti-DoS line cap (64 KiB) — `read_msg` never allocates beyond this.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Protocol message (internally tagged by `type`, in snake_case).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Msg {
    // agent/client → broker
    Register {
        proto: u32,
        tenant: String,
        transport: TransportKind,
    },
    Psi {
        sample: PsiSample,
        swaps: Vec<SwapEntry>,
        #[serde(default)]
        mem: Option<TenantMem>,
    },
    SwapOnDone {
        slice: SliceId,
        ok: bool,
        detail: String,
    },
    SwapOffDone {
        slice: SliceId,
        ok: bool,
        detail: String,
    },
    LeaseRequest {
        bytes: u64,
    },
    LeaseRelease {
        lease: u32,
    },
    Status,
    // broker → agent/client
    Registered {
        tenant_id: TenantId,
    },
    Ack,
    SwapOn {
        slice: SliceId,
        export: String,
        endpoint: NbdEndpoint,
        swap_prio: Option<i32>,
    },
    SwapOff {
        slice: SliceId,
    },
    DemoteAll,
    LeaseGranted {
        lease: u32,
        bytes: u64,
    },
    LeaseDenied {
        reason: String,
    },
    StatusReply {
        tenants: Vec<TenantStatus>,
        slices: Vec<Slice>,
        #[serde(default)]
        slice_io: Vec<SliceIo>,
        last_rebalance_secs: Option<u64>,
    },
    Error {
        reason: String,
    },
}

/// NBD endpoint that the agent receives in `SwapOn` (DT-25: Unix for local tenant, TCP for civm).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NbdEndpoint {
    Unix { path: String },
    Tcp { host: String, port: u16 },
}

/// Entry of `/proc/swaps` reported by the agent (reconciliation DT-9/DT-21; "most idle" DT-19).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SwapEntry {
    pub dev: String,
    pub prio: i32,
    pub size_kb: u64,
    pub used_kb: u64,
}

/// State of a tenant in `StatusReply` (RF-B4). `present=false` = session dropped (DT-20).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TenantStatus {
    pub id: TenantId,
    pub name: String,
    pub psi: PsiSample,
    pub slices: Vec<SliceId>,
    pub present: bool,
    /// Served bytes (accumulated) of the `Active` slices of this tenant (telemetria RF-1).
    #[serde(default)]
    pub bytes_served: u64,
}

/// Tenant memory telemetry reported in `Psi` (RF-2). `swap_current` comes from cgroup v2
/// (DT-10, optional); `diskstats_io` = read+written sectors (×512) of the nbd devices that the tenant
/// performed `swapon` on (DT-11).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TenantMem {
    pub swap_current: Option<u64>,
    pub diskstats_io: u64,
}

/// IO counters per slice in `StatusReply` (RF-1 telemetry; parallel to [`Slice`] to avoid touching
/// the state machine — DT-2). `id` = index of export `s{id}`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SliceIo {
    pub id: SliceId,
    pub bytes_served: u64,
    pub io_count: u64,
}

/// Serializes `msg` + `'\n'` and flushes (one message per line).
pub fn write_msg<W: Write>(w: &mut W, msg: &Msg) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

/// Reads a line (up to [`MAX_LINE_BYTES`]) and deserializes it.
///
/// `Ok(None)` on clean EOF; `Err` on giant line, invalid JSON or unknown shape.
/// `take(MAX_LINE_BYTES + 1)` ensures we never read/allocate beyond the cap (anti-DoS).
pub fn read_msg<R: BufRead>(r: &mut R) -> std::io::Result<Option<Msg>> {
    let mut buf = Vec::new();
    let n = r
        .by_ref()
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None); // clean EOF
    }
    let had_newline = buf.last() == Some(&b'\n');
    if !had_newline && buf.len() > MAX_LINE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "linha excede MAX_LINE_BYTES",
        ));
    }
    let line = buf.strip_suffix(b"\n").unwrap_or(&buf);
    let msg = serde_json::from_slice::<Msg>(line)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(msg))
}

#[cfg(test)]
mod tests {
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
    fn roundtrip_each_variant() {
        let msgs = vec![
            Msg::Register {
                proto: PROTO_VERSION,
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
            Msg::Registered { tenant_id: 2 },
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
    fn unknown_type_is_err() {
        let mut cur = Cursor::new(b"{\"type\":\"bogus\"}\n".to_vec());
        assert!(read_msg(&mut cur).is_err());
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
            other => panic!("esperava Psi, veio {other:?}"),
        }
    }

    #[test]
    fn status_reply_slice_io_defaults_empty() {
        // StatusReply without `slice_io` → empty vector (additive).
        let line = b"{\"type\":\"status_reply\",\"tenants\":[],\"slices\":[],\"last_rebalance_secs\":null}\n";
        let mut cur = Cursor::new(line.to_vec());
        match read_msg(&mut cur).unwrap().unwrap() {
            Msg::StatusReply { slice_io, .. } => assert!(slice_io.is_empty()),
            other => panic!("esperava StatusReply, veio {other:?}"),
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
    fn two_messages_one_stream() {
        let mut buf = Vec::new();
        write_msg(&mut buf, &Msg::Ack).unwrap();
        write_msg(&mut buf, &Msg::DemoteAll).unwrap();
        let mut cur = Cursor::new(buf);
        assert_eq!(read_msg(&mut cur).unwrap().unwrap(), Msg::Ack);
        assert_eq!(read_msg(&mut cur).unwrap().unwrap(), Msg::DemoteAll);
        assert!(read_msg(&mut cur).unwrap().is_none());
    }
}
