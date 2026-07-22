//! Local loopback protocol used by DCC adapters.
//!
//! This is intentionally distinct from the broker wire protocol (DT-35). The
//! addon can ask for a lease and receive a compact evidence-backed snapshot,
//! without gaining access to swap or broker control messages.
#![forbid(unsafe_code)]

use std::io::{BufRead, Write};

pub const MAX_LINE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LocalMsg {
    Status,
    LeaseRequest { bytes: u64, client: String },
    LeaseRelease { lease: u32 },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LocalReply {
    Status {
        vram_free: Option<u64>,
        vram_total: Option<u64>,
        lease: Option<u32>,
        evidence: Vec<String>,
    },
    LeaseGranted {
        lease: u32,
        bytes: u64,
    },
    LeaseDenied {
        reason: String,
    },
    Released {
        lease: u32,
    },
    Error {
        reason: String,
    },
}

pub fn write_json_line<W: Write, T: serde::Serialize>(w: &mut W, value: &T) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    if line.len() > MAX_LINE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "local message too large",
        ));
    }
    line.push(b'\n');
    w.write_all(&line)
}

pub fn read_json_line<R: BufRead, T: serde::de::DeserializeOwned>(
    r: &mut R,
) -> std::io::Result<Option<T>> {
    let mut line = String::new();
    let n = r.read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    if line.len() > MAX_LINE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "local message too large",
        ));
    }
    serde_json::from_str(line.trim_end())
        .map(Some)
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use std::io::Cursor;

    #[test]
    fn local_protocol_roundtrip() {
        let msg = LocalMsg::LeaseRequest {
            bytes: 4096,
            client: "dcc-test".into(),
        };
        let mut buf = Vec::new();
        write_json_line(&mut buf, &msg).expect("write");
        let decoded: LocalMsg = read_json_line(&mut Cursor::new(buf))
            .expect("read")
            .expect("msg");
        assert_eq!(decoded, msg);
    }
}
