//! Wire format JSON-lines do protocolo agente↔broker (RF-B1 / DT-1).
//!
//! Um objeto JSON por linha (`\n`, UTF-8). Control-plane de baixa taxa (~1 msg/s/tenant),
//! debugável com `nc`/`jq` (ADR-0005). O codec impõe teto de linha [`MAX_LINE_BYTES`] **antes**
//! de alocar (anti-DoS, espelha `MAX_OPT_LEN` do handshake NBD).

use std::io::{BufRead, Read, Write};

use crate::model::{PsiSample, Slice, SliceId, TenantId, TransportKind};

/// Versão do protocolo; `Register` com `proto != PROTO_VERSION` é recusado pelo broker (ITEM-8).
pub const PROTO_VERSION: u32 = 1;
/// Teto anti-DoS por linha (64 KiB) — `read_msg` nunca aloca além disso.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Mensagem do protocolo (internamente etiquetada por `type`, em snake_case).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Msg {
    // agente/cliente → broker
    Register {
        proto: u32,
        tenant: String,
        transport: TransportKind,
    },
    Psi {
        sample: PsiSample,
        swaps: Vec<SwapEntry>,
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
    // broker → agente/cliente
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
        last_rebalance_secs: Option<u64>,
    },
    Error {
        reason: String,
    },
}

/// Endpoint NBD que o agente recebe no `SwapOn` (DT-25: Unix p/ tenant local, TCP p/ civm).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NbdEndpoint {
    Unix { path: String },
    Tcp { host: String, port: u16 },
}

/// Entrada de `/proc/swaps` reportada pelo agente (reconciliação DT-9/DT-21; "mais ociosas" DT-19).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SwapEntry {
    pub dev: String,
    pub prio: i32,
    pub size_kb: u64,
    pub used_kb: u64,
}

/// Estado de um tenant no `StatusReply` (RF-B4). `present=false` = sessão caída (DT-20).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TenantStatus {
    pub id: TenantId,
    pub name: String,
    pub psi: PsiSample,
    pub slices: Vec<SliceId>,
    pub present: bool,
}

/// Serializa `msg` + `'\n'` e dá flush (uma mensagem por linha).
pub fn write_msg<W: Write>(w: &mut W, msg: &Msg) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

/// Lê uma linha (teto [`MAX_LINE_BYTES`]) e desserializa.
///
/// `Ok(None)` em EOF limpo; `Err` em linha gigante, JSON inválido ou shape desconhecido.
/// O `take(MAX_LINE_BYTES + 1)` garante que nunca lemos/alocamos além do teto (anti-DoS).
pub fn read_msg<R: BufRead>(r: &mut R) -> std::io::Result<Option<Msg>> {
    let mut buf = Vec::new();
    let n = r
        .by_ref()
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None); // EOF limpo
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
        assert_eq!(buf.last(), Some(&b'\n'), "deve terminar em uma única linha");
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
                }],
                slices: vec![Slice {
                    id: 0,
                    offset: 0,
                    len: 64,
                    tenant: Some(1),
                    state: SliceState::Active,
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
    fn oversize_line_is_err() {
        // Linha > MAX_LINE_BYTES sem '\n' dentro do teto → Err (não tenta parsear gigante).
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
