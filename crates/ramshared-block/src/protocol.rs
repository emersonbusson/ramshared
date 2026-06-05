//! NBD fixed-newstyle: constantes + parse/encode da fase de transmissão.
//! Wire é big-endian. SPEC §10.1.

use core::fmt;

// Handshake.
pub const NBDMAGIC: u64 = 0x4e42_444d_4147_4943; // "NBDMAGIC"
pub const IHAVEOPT: u64 = 0x4948_4156_454f_5054; // "IHAVEOPT"
pub const NBD_FLAG_FIXED_NEWSTYLE: u16 = 1 << 0;
pub const NBD_FLAG_NO_ZEROES: u16 = 1 << 1;

// Flags de transmissão (export).
pub const NBD_FLAG_HAS_FLAGS: u16 = 1 << 0;
pub const NBD_FLAG_SEND_FLUSH: u16 = 1 << 2;

// Transmissão.
pub const NBD_REQUEST_MAGIC: u32 = 0x2560_9513;
pub const NBD_SIMPLE_REPLY_MAGIC: u32 = 0x6744_6698;
pub const REQUEST_LEN: usize = 28;
pub const SIMPLE_REPLY_LEN: usize = 16;

/// Comandos NBD (campo `type`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    Read,
    Write,
    Disc,
    Flush,
    Trim,
    Unknown(u16),
}

impl Command {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0 => Command::Read,
            1 => Command::Write,
            2 => Command::Disc,
            3 => Command::Flush,
            4 => Command::Trim,
            other => Command::Unknown(other),
        }
    }
}

/// Requisição NBD (cabeçalho de 28 bytes; payload de WRITE vem depois no socket).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request {
    pub flags: u16,
    pub cmd: Command,
    pub handle: u64,
    pub offset: u64,
    pub len: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    ShortBuffer { got: usize, need: usize },
    BadMagic(u32),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::ShortBuffer { got, need } => {
                write!(f, "buffer curto: {got} < {need}")
            }
            ProtocolError::BadMagic(m) => write!(f, "magic de requisição inválido: {m:#010x}"),
        }
    }
}

impl core::error::Error for ProtocolError {}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}
fn be16(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}
fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Faz o parse do cabeçalho de 28 bytes. Valida tamanho e magic.
pub fn parse_request(buf: &[u8]) -> Result<Request, ProtocolError> {
    if buf.len() < REQUEST_LEN {
        return Err(ProtocolError::ShortBuffer {
            got: buf.len(),
            need: REQUEST_LEN,
        });
    }
    let magic = be32(&buf[0..4]);
    if magic != NBD_REQUEST_MAGIC {
        return Err(ProtocolError::BadMagic(magic));
    }
    Ok(Request {
        flags: be16(&buf[4..6]),
        cmd: Command::from_u16(be16(&buf[6..8])),
        handle: be64(&buf[8..16]),
        offset: be64(&buf[16..24]),
        len: be32(&buf[24..28]),
    })
}

/// Codifica a simple reply de 16 bytes (magic + error + handle ecoado).
pub fn encode_simple_reply(error: u32, handle: u64) -> [u8; SIMPLE_REPLY_LEN] {
    let mut out = [0u8; SIMPLE_REPLY_LEN];
    out[0..4].copy_from_slice(&NBD_SIMPLE_REPLY_MAGIC.to_be_bytes());
    out[4..8].copy_from_slice(&error.to_be_bytes());
    out[8..16].copy_from_slice(&handle.to_be_bytes());
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn build_request(cmd: u16, handle: u64, off: u64, len: u32) -> [u8; REQUEST_LEN] {
        let mut b = [0u8; REQUEST_LEN];
        b[0..4].copy_from_slice(&NBD_REQUEST_MAGIC.to_be_bytes());
        b[4..6].copy_from_slice(&0u16.to_be_bytes());
        b[6..8].copy_from_slice(&cmd.to_be_bytes());
        b[8..16].copy_from_slice(&handle.to_be_bytes());
        b[16..24].copy_from_slice(&off.to_be_bytes());
        b[24..28].copy_from_slice(&len.to_be_bytes());
        b
    }

    #[test]
    fn parses_a_well_formed_write_request() {
        let raw = build_request(1, 0xdead_beef, 4096, 8192);
        let r = parse_request(&raw).expect("deve parsear");
        assert_eq!(r.cmd, Command::Write);
        assert_eq!(r.handle, 0xdead_beef);
        assert_eq!(r.offset, 4096);
        assert_eq!(r.len, 8192);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut raw = build_request(0, 1, 0, 4096);
        raw[0] = 0xff;
        assert!(matches!(
            parse_request(&raw),
            Err(ProtocolError::BadMagic(_))
        ));
    }

    #[test]
    fn rejects_short_buffer() {
        assert!(matches!(
            parse_request(&[0u8; 10]),
            Err(ProtocolError::ShortBuffer { need: 28, .. })
        ));
    }

    #[test]
    fn reply_round_trips_magic_and_handle() {
        let r = encode_simple_reply(5, 0x1122_3344_5566_7788);
        assert_eq!(&r[0..4], &NBD_SIMPLE_REPLY_MAGIC.to_be_bytes());
        assert_eq!(u32::from_be_bytes([r[4], r[5], r[6], r[7]]), 5);
        assert_eq!(&r[8..16], &0x1122_3344_5566_7788u64.to_be_bytes());
    }

    #[test]
    fn maps_command_codes() {
        assert_eq!(Command::from_u16(0), Command::Read);
        assert_eq!(Command::from_u16(2), Command::Disc);
        assert_eq!(Command::from_u16(99), Command::Unknown(99));
    }
}
