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
const NBD_REP_ERR_UNKNOWN: u32 = 0x8000_0006; // export desconhecido (GO/INFO)
const NBD_INFO_EXPORT: u16 = 0;
const NBD_FLAG_C_NO_ZEROES: u32 = 1 << 1;
const MAX_OPT_LEN: usize = 4096; // opções NBD são pequenas; teto anti-alloc.

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

/// Um export disponível para negociação (uma slice, RF-L1). `name == ""` nunca aparece na
/// tabela; nome vazio do cliente resolve para `exports[0]` (export default; compat Fase B).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub name: String,
    pub size: u64,
}

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

fn bad(msg: &'static str) -> HandshakeError {
    HandshakeError::Io(io::Error::new(io::ErrorKind::InvalidData, msg))
}

/// Extrai o nome do export do payload de `NBD_OPT_GO`/`NBD_OPT_INFO`:
/// `[u32 name_len][name][u16 n_info][...]`. Malformado/truncado ⇒ erro (fecha).
fn go_export_name(data: &[u8]) -> Result<&[u8], HandshakeError> {
    if data.len() < 4 {
        return Err(bad("GO/INFO sem campo de nome"));
    }
    let name_len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let name_end = 4usize
        .checked_add(name_len)
        .ok_or_else(|| bad("name_len overflow"))?;
    // precisa do nome + n_info (u16) depois dele.
    if data.len() < name_end + 2 {
        return Err(bad("GO/INFO truncado"));
    }
    Ok(&data[4..name_end])
}

/// Nomes de export são UTF-8.
fn name_utf8(name: &[u8]) -> Result<&str, HandshakeError> {
    core::str::from_utf8(name).map_err(|_| bad("nome de export não-UTF-8"))
}

/// Resolve o nome para um índice em `exports`; nome vazio ⇒ `exports[0]` (default, compat Fase B).
fn find_export(exports: &[Export], name: &str) -> Option<usize> {
    if name.is_empty() {
        return (!exports.is_empty()).then_some(0);
    }
    exports.iter().position(|e| e.name == name)
}

/// Roda o handshake do servidor negociando o export **pelo nome** (RF-L1). Retorna o índice do
/// export negociado em `exports` quando o stream entra na fase de transmissão. Nome vazio do
/// cliente ⇒ `exports[0]` (wire **byte-idêntico** ao da Fase B; RNF-4).
pub fn server_handshake<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    exports: &[Export],
    tx_flags: u16,
) -> Result<usize, HandshakeError> {
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
        if len > MAX_OPT_LEN {
            return Err(bad("opção NBD com len excessivo"));
        }
        let mut data = vec![0u8; len];
        r.read_exact(&mut data)?;

        match opt {
            NBD_OPT_EXPORT_NAME => {
                // payload inteiro é o nome (vazio = default). EXPORT_NAME não tem reply de erro:
                // export desconhecido ⇒ fecha a conexão (Io).
                let name = name_utf8(&data)?;
                let idx = find_export(exports, name).ok_or_else(|| bad("export desconhecido"))?;
                w.write_all(&exports[idx].size.to_be_bytes())?;
                w.write_all(&tx_flags.to_be_bytes())?;
                if !no_zeroes {
                    w.write_all(&[0u8; 124])?;
                }
                w.flush()?;
                return Ok(idx);
            }
            NBD_OPT_GO | NBD_OPT_INFO => {
                let name = name_utf8(go_export_name(&data)?)?;
                match find_export(exports, name) {
                    Some(idx) => {
                        write_export_info(w, opt, exports[idx].size, tx_flags)?;
                        w.flush()?;
                        if opt == NBD_OPT_GO {
                            return Ok(idx); // GO transiciona; INFO continua negociando.
                        }
                    }
                    None => {
                        // GO/INFO têm reply de erro: nome desconhecido ⇒ ERR_UNKNOWN, segue.
                        write_opt_reply(w, opt, NBD_REP_ERR_UNKNOWN, &[])?;
                        w.flush()?;
                    }
                }
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

    /// Stream com várias opções em sequência.
    fn stream_opts(client_flags: u32, opts: &[(u32, Vec<u8>)]) -> Cursor<Vec<u8>> {
        let mut v = Vec::new();
        v.extend_from_slice(&client_flags.to_be_bytes());
        for (opt, data) in opts {
            v.extend_from_slice(&IHAVEOPT.to_be_bytes());
            v.extend_from_slice(&opt.to_be_bytes());
            v.extend_from_slice(&(data.len() as u32).to_be_bytes());
            v.extend_from_slice(data);
        }
        Cursor::new(v)
    }

    /// Payload de GO/INFO com um nome de export.
    fn go_data(name: &[u8]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&(name.len() as u32).to_be_bytes());
        d.extend_from_slice(name);
        d.extend_from_slice(&0u16.to_be_bytes()); // n_info=0
        d
    }

    /// Tabela de 1 export "default" (compat Fase B: nome vazio do cliente resolve p/ índice 0).
    fn one(size: u64) -> Vec<Export> {
        vec![Export {
            name: "default".to_string(),
            size,
        }]
    }

    fn has_rep(out: &[u8], rep: u32) -> bool {
        out.windows(4).any(|w| w == rep.to_be_bytes())
    }

    #[test]
    fn greeting_then_export_name_no_zeroes() {
        let mut r = client_stream(NBD_FLAG_C_NO_ZEROES, NBD_OPT_EXPORT_NAME, b"");
        let mut out = Vec::new();
        server_handshake(&mut r, &mut out, &one(1 << 20), 1).unwrap();
        // greeting: NBDMAGIC + IHAVEOPT + flags(u16)
        assert_eq!(&out[0..8], &NBDMAGIC.to_be_bytes());
        assert_eq!(&out[8..16], &IHAVEOPT.to_be_bytes());
        assert_eq!(
            u16::from_be_bytes([out[16], out[17]]),
            NBD_FLAG_FIXED_NEWSTYLE | NBD_FLAG_NO_ZEROES
        );
        // export reply: size(u64) + tx_flags(u16), SEM 124 zeros (NO_ZEROES) — byte-compat Fase B
        assert_eq!(u64::from_be_bytes(out[18..26].try_into().unwrap()), 1 << 20);
        assert_eq!(u16::from_be_bytes([out[26], out[27]]), 1);
        assert_eq!(out.len(), 28, "NO_ZEROES => sem padding de 124");
    }

    #[test]
    fn export_name_with_zeroes_pads_124() {
        let mut r = client_stream(0, NBD_OPT_EXPORT_NAME, b"");
        let mut out = Vec::new();
        server_handshake(&mut r, &mut out, &one(4096), 1).unwrap();
        assert_eq!(out.len(), 18 + 8 + 2 + 124);
    }

    #[test]
    fn go_replies_info_then_ack_and_transitions() {
        let mut r = client_stream(NBD_FLAG_C_NO_ZEROES, NBD_OPT_GO, &go_data(b""));
        let mut out = Vec::new();
        server_handshake(&mut r, &mut out, &one(4096), 1).unwrap();
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
        let res = server_handshake(&mut r, &mut out, &one(4096), 1);
        assert!(matches!(res, Err(HandshakeError::Aborted)));
    }

    #[test]
    fn rejects_oversized_option_len() {
        // opção com len gigante deve falhar ANTES de alocar (M4 anti-DoS).
        let mut v = Vec::new();
        v.extend_from_slice(&0u32.to_be_bytes()); // client_flags
        v.extend_from_slice(&IHAVEOPT.to_be_bytes()); // opt magic
        v.extend_from_slice(&NBD_OPT_INFO.to_be_bytes()); // opt
        v.extend_from_slice(&u32::MAX.to_be_bytes()); // len absurdo
        let mut r = Cursor::new(v);
        let mut out = Vec::new();
        let res = server_handshake(&mut r, &mut out, &one(4096), 1);
        assert!(matches!(res, Err(HandshakeError::Io(_))));
    }

    #[test]
    fn go_named_export_returns_index_and_size() {
        let exports = vec![
            Export {
                name: "s0".to_string(),
                size: 4096,
            },
            Export {
                name: "s1".to_string(),
                size: 8192,
            },
        ];
        let mut r = client_stream(NBD_FLAG_C_NO_ZEROES, NBD_OPT_GO, &go_data(b"s1"));
        let mut out = Vec::new();
        let idx = server_handshake(&mut r, &mut out, &exports, 1).unwrap();
        assert_eq!(idx, 1);
        // INFO export: size(u64) em offset 40 (greeting 18 + rep header 16 + INFO_EXPORT u16)
        assert_eq!(u64::from_be_bytes(out[40..48].try_into().unwrap()), 8192);
    }

    #[test]
    fn go_unknown_name_replies_err_unknown_and_continues() {
        // GO com nome inexistente ⇒ ERR_UNKNOWN e NÃO transiciona; segue até o ABORT.
        let mut r = stream_opts(
            0,
            &[(NBD_OPT_GO, go_data(b"nope")), (NBD_OPT_ABORT, vec![])],
        );
        let mut out = Vec::new();
        let res = server_handshake(&mut r, &mut out, &one(4096), 1);
        assert!(matches!(res, Err(HandshakeError::Aborted)));
        assert!(has_rep(&out, NBD_REP_ERR_UNKNOWN));
    }

    #[test]
    fn export_name_unknown_closes() {
        // EXPORT_NAME não tem reply de erro: nome desconhecido ⇒ fecha (Io).
        let mut r = client_stream(0, NBD_OPT_EXPORT_NAME, b"nope");
        let mut out = Vec::new();
        let res = server_handshake(&mut r, &mut out, &one(4096), 1);
        assert!(matches!(res, Err(HandshakeError::Io(_))));
    }

    #[test]
    fn export_name_non_utf8_errors() {
        let mut r = client_stream(0, NBD_OPT_EXPORT_NAME, &[0xff, 0xfe]);
        let mut out = Vec::new();
        let res = server_handshake(&mut r, &mut out, &one(4096), 1);
        assert!(matches!(res, Err(HandshakeError::Io(_))));
    }

    #[test]
    fn empty_name_resolves_first_export() {
        let exports = vec![
            Export {
                name: "s0".to_string(),
                size: 4096,
            },
            Export {
                name: "s1".to_string(),
                size: 8192,
            },
        ];
        let mut r = client_stream(NBD_FLAG_C_NO_ZEROES, NBD_OPT_EXPORT_NAME, b"");
        let mut out = Vec::new();
        let idx = server_handshake(&mut r, &mut out, &exports, 1).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(u64::from_be_bytes(out[18..26].try_into().unwrap()), 4096);
    }
}
