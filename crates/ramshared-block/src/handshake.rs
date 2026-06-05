//! NBD fixed-newstyle: handshake do lado **servidor** (negociação do export).
//! Genérico sobre `Read + Write` → testável sem socket/root. SPEC §10.1.
//!
//! Suporta `NBD_OPT_EXPORT_NAME` (simples) e `NBD_OPT_GO`/`NBD_OPT_INFO` (modernos,
//! usados por versões recentes do `nbd-client`). Ao final, o stream entra na fase
//! de transmissão (ver [`crate::serve`]).

use crate::protocol::{IHAVEOPT, NBD_FLAG_FIXED_NEWSTYLE, NBD_FLAG_NO_ZEROES, NBDMAGIC};
use core::fmt;
use std::io::{self, Read, Write};

pub const NBD_OPT_EXPORT_NAME: u32 = 1;
pub const NBD_OPT_ABORT: u32 = 2;
pub const NBD_OPT_INFO: u32 = 6;
pub const NBD_OPT_GO: u32 = 7;

const NBD_REP_MAGIC: u64 = 0x0003_e889_0455_65a9;
const NBD_REP_ACK: u32 = 1;
const NBD_REP_INFO: u32 = 3;
const NBD_REP_ERR_UNSUP: u32 = 0x8000_0001;
const NBD_INFO_EXPORT: u16 = 0;
const NBD_FLAG_C_NO_ZEROES: u32 = 1 << 1;

#[derive(Debug)]
pub enum HandshakeError {
    Io(io::Error),
    Aborted,
}

impl From<io::Error> for HandshakeError {
    fn from(e: io::Error) -> Self {
        HandshakeError::Io(e)
    }
}

impl fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeError::Io(e) => write!(f, "io no handshake: {e}"),
            HandshakeError::Aborted => f.write_str("cliente abortou o handshake (NBD_OPT_ABORT)"),
        }
    }
}

impl core::error::Error for HandshakeError {}

fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b))
}
fn read_u64<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_be_bytes(b))
}

fn write_opt_reply<W: Write>(w: &mut W, opt: u32, rep: u32, data: &[u8]) -> io::Result<()> {
    w.write_all(&NBD_REP_MAGIC.to_be_bytes())?;
    w.write_all(&opt.to_be_bytes())?;
    w.write_all(&rep.to_be_bytes())?;
    w.write_all(&(data.len() as u32).to_be_bytes())?;
    w.write_all(data)
}

fn write_export_info<W: Write>(w: &mut W, opt: u32, size: u64, tx_flags: u16) -> io::Result<()> {
    let mut info = Vec::with_capacity(12);
    info.extend_from_slice(&NBD_INFO_EXPORT.to_be_bytes());
    info.extend_from_slice(&size.to_be_bytes());
    info.extend_from_slice(&tx_flags.to_be_bytes());
    write_opt_reply(w, opt, NBD_REP_INFO, &info)?;
    write_opt_reply(w, opt, NBD_REP_ACK, &[])
}

/// Roda o handshake do servidor. Retorna `Ok` quando o stream está pronto para a
/// fase de transmissão (export negociado).
pub fn server_handshake<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    export_size: u64,
    tx_flags: u16,
) -> Result<(), HandshakeError> {
    // Greeting: NBDMAGIC + IHAVEOPT + handshake flags.
    w.write_all(&NBDMAGIC.to_be_bytes())?;
    w.write_all(&IHAVEOPT.to_be_bytes())?;
    w.write_all(&(NBD_FLAG_FIXED_NEWSTYLE | NBD_FLAG_NO_ZEROES).to_be_bytes())?;
    w.flush()?;

    let client_flags = read_u32(r)?;
    let no_zeroes = client_flags & NBD_FLAG_C_NO_ZEROES != 0;

    loop {
        let _opt_magic = read_u64(r)?; // IHAVEOPT (ignorado: confiamos no fluxo)
        let opt = read_u32(r)?;
        let len = read_u32(r)? as usize;
        let mut data = vec![0u8; len];
        r.read_exact(&mut data)?;

        match opt {
            NBD_OPT_EXPORT_NAME => {
                w.write_all(&export_size.to_be_bytes())?;
                w.write_all(&tx_flags.to_be_bytes())?;
                if !no_zeroes {
                    w.write_all(&[0u8; 124])?;
                }
                w.flush()?;
                return Ok(());
            }
            NBD_OPT_GO => {
                write_export_info(w, opt, export_size, tx_flags)?;
                w.flush()?;
                return Ok(());
            }
            NBD_OPT_INFO => {
                write_export_info(w, opt, export_size, tx_flags)?;
                w.flush()?;
                // INFO não transiciona: continua negociando.
            }
            NBD_OPT_ABORT => {
                write_opt_reply(w, opt, NBD_REP_ACK, &[])?;
                w.flush()?;
                return Err(HandshakeError::Aborted);
            }
            _ => {
                write_opt_reply(w, opt, NBD_REP_ERR_UNSUP, &[])?;
                w.flush()?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::io::Cursor;

    /// Monta um stream de cliente: client_flags + uma opção.
    fn client_stream(client_flags: u32, opt: u32, data: &[u8]) -> Cursor<Vec<u8>> {
        let mut v = Vec::new();
        v.extend_from_slice(&client_flags.to_be_bytes());
        v.extend_from_slice(&IHAVEOPT.to_be_bytes());
        v.extend_from_slice(&opt.to_be_bytes());
        v.extend_from_slice(&(data.len() as u32).to_be_bytes());
        v.extend_from_slice(data);
        Cursor::new(v)
    }

    #[test]
    fn greeting_then_export_name_no_zeroes() {
        let mut r = client_stream(NBD_FLAG_C_NO_ZEROES, NBD_OPT_EXPORT_NAME, b"");
        let mut out = Vec::new();
        server_handshake(&mut r, &mut out, 1 << 20, 1).unwrap();
        // greeting: NBDMAGIC + IHAVEOPT + flags(u16)
        assert_eq!(&out[0..8], &NBDMAGIC.to_be_bytes());
        assert_eq!(&out[8..16], &IHAVEOPT.to_be_bytes());
        assert_eq!(
            u16::from_be_bytes([out[16], out[17]]),
            NBD_FLAG_FIXED_NEWSTYLE | NBD_FLAG_NO_ZEROES
        );
        // export reply: size(u64) + tx_flags(u16), SEM 124 zeros (NO_ZEROES)
        assert_eq!(u64::from_be_bytes(out[18..26].try_into().unwrap()), 1 << 20);
        assert_eq!(u16::from_be_bytes([out[26], out[27]]), 1);
        assert_eq!(out.len(), 28, "NO_ZEROES => sem padding de 124");
    }

    #[test]
    fn export_name_with_zeroes_pads_124() {
        let mut r = client_stream(0, NBD_OPT_EXPORT_NAME, b"");
        let mut out = Vec::new();
        server_handshake(&mut r, &mut out, 4096, 1).unwrap();
        assert_eq!(out.len(), 18 + 8 + 2 + 124);
    }

    #[test]
    fn go_replies_info_then_ack_and_transitions() {
        // NBD_OPT_GO data: nome(len u32=0) + n_info(u16=0)
        let mut godata = Vec::new();
        godata.extend_from_slice(&0u32.to_be_bytes());
        godata.extend_from_slice(&0u16.to_be_bytes());
        let mut r = client_stream(NBD_FLAG_C_NO_ZEROES, NBD_OPT_GO, &godata);
        let mut out = Vec::new();
        server_handshake(&mut r, &mut out, 4096, 1).unwrap();
        // após o greeting (18B), 1ª reply é NBD_REP_MAGIC
        assert_eq!(
            u64::from_be_bytes(out[18..26].try_into().unwrap()),
            NBD_REP_MAGIC
        );
    }

    #[test]
    fn abort_returns_err() {
        let mut r = client_stream(0, NBD_OPT_ABORT, b"");
        let mut out = Vec::new();
        let res = server_handshake(&mut r, &mut out, 4096, 1);
        assert!(matches!(res, Err(HandshakeError::Aborted)));
    }
}
