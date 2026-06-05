//! Despacho de requisição NBD → [`BlockBackend`], com validação da §8
//! (alinhamento ao block size, faixa) e mapeamento de erro → errno NBD.

use crate::protocol::{Command, Request, SIMPLE_REPLY_LEN, encode_simple_reply};

// errno na simple reply (campo error).
pub const NBD_OK: u32 = 0;
pub const NBD_EIO: u32 = 5;
pub const NBD_EINVAL: u32 = 22;

/// Erro do backend de armazenamento (ex.: falha CUDA no hot path).
#[derive(Debug)]
pub struct IoError(pub String);

/// Armazenamento por trás do device NBD (a VRAM, no nosso caso).
pub trait BlockBackend {
    fn size_bytes(&self) -> u64;
    /// Block size lógico (múltiplo de 512; 4096 no MVP — SPEC §8).
    fn block_size(&self) -> u32;
    fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<(), IoError>;
    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError>;
    fn flush(&mut self) -> Result<(), IoError>;
}

/// Resultado do despacho: bytes da reply, dados de leitura (se READ) e se o
/// cliente pediu desconexão (`NBD_CMD_DISC`).
pub struct ServeOutcome {
    pub reply: [u8; SIMPLE_REPLY_LEN],
    pub read_data: Vec<u8>,
    pub disconnect: bool,
}

fn errno_of(r: Result<(), IoError>) -> u32 {
    match r {
        Ok(()) => NBD_OK,
        Err(_) => NBD_EIO,
    }
}

/// Valida alinhamento e faixa (SPEC §8): desalinhado ou fora de faixa = EINVAL,
/// **antes** de tocar o backend.
fn validate<B: BlockBackend + ?Sized>(req: &Request, backend: &B) -> Result<(), u32> {
    let bs = backend.block_size() as u64;
    if bs == 0 || !req.offset.is_multiple_of(bs) || !(req.len as u64).is_multiple_of(bs) {
        return Err(NBD_EINVAL);
    }
    match req.offset.checked_add(req.len as u64) {
        Some(end) if end <= backend.size_bytes() => Ok(()),
        _ => Err(NBD_EINVAL),
    }
}

/// Despacha uma requisição já parseada. `payload` é o dado de WRITE (vazio nos
/// demais). Não faz I/O de socket — só lógica (testável sem root).
pub fn serve<B: BlockBackend + ?Sized>(
    req: &Request,
    payload: &[u8],
    backend: &mut B,
) -> ServeOutcome {
    let reply = |error: u32| encode_simple_reply(error, req.handle);
    let plain = |error: u32| ServeOutcome {
        reply: reply(error),
        read_data: Vec::new(),
        disconnect: false,
    };

    match req.cmd {
        Command::Disc => ServeOutcome {
            reply: reply(NBD_OK),
            read_data: Vec::new(),
            disconnect: true,
        },
        Command::Flush => plain(errno_of(backend.flush())),
        Command::Trim => plain(NBD_OK), // no-op seguro no MVP
        Command::Unknown(_) => plain(NBD_EINVAL),
        Command::Read => {
            if let Err(e) = validate(req, backend) {
                return plain(e);
            }
            let mut buf = vec![0u8; req.len as usize];
            match backend.read_at(req.offset, &mut buf) {
                Ok(()) => ServeOutcome {
                    reply: reply(NBD_OK),
                    read_data: buf,
                    disconnect: false,
                },
                Err(_) => plain(NBD_EIO),
            }
        }
        Command::Write => {
            if let Err(e) = validate(req, backend) {
                return plain(e);
            }
            if payload.len() != req.len as usize {
                return plain(NBD_EINVAL);
            }
            plain(errno_of(backend.write_at(req.offset, payload)))
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::protocol::Request;

    struct MemBackend {
        data: Vec<u8>,
        bs: u32,
    }
    impl BlockBackend for MemBackend {
        fn size_bytes(&self) -> u64 {
            self.data.len() as u64
        }
        fn block_size(&self) -> u32 {
            self.bs
        }
        fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
            let o = off as usize;
            buf.copy_from_slice(&self.data[o..o + buf.len()]);
            Ok(())
        }
        fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
            let o = off as usize;
            self.data[o..o + data.len()].copy_from_slice(data);
            Ok(())
        }
        fn flush(&mut self) -> Result<(), IoError> {
            Ok(())
        }
    }

    fn req(cmd: Command, off: u64, len: u32) -> Request {
        Request {
            flags: 0,
            cmd,
            handle: 7,
            offset: off,
            len,
        }
    }

    #[test]
    fn write_then_read_round_trips() {
        let mut b = MemBackend {
            data: vec![0u8; 1 << 20],
            bs: 4096,
        };
        let payload = vec![0xABu8; 4096];
        let w = serve(&req(Command::Write, 4096, 4096), &payload, &mut b);
        assert_eq!(
            u32::from_be_bytes([w.reply[4], w.reply[5], w.reply[6], w.reply[7]]),
            NBD_OK
        );

        let r = serve(&req(Command::Read, 4096, 4096), &[], &mut b);
        assert_eq!(r.read_data, payload);
    }

    #[test]
    fn out_of_range_is_einval_not_corruption() {
        let mut b = MemBackend {
            data: vec![0u8; 8192],
            bs: 4096,
        };
        let r = serve(&req(Command::Read, 8192, 4096), &[], &mut b);
        assert_eq!(
            u32::from_be_bytes([r.reply[4], r.reply[5], r.reply[6], r.reply[7]]),
            NBD_EINVAL
        );
        assert!(r.read_data.is_empty());
    }

    #[test]
    fn unaligned_is_rejected_before_backend() {
        let mut b = MemBackend {
            data: vec![0u8; 1 << 16],
            bs: 4096,
        };
        let r = serve(&req(Command::Write, 100, 4096), &vec![0u8; 4096], &mut b);
        assert_eq!(
            u32::from_be_bytes([r.reply[4], r.reply[5], r.reply[6], r.reply[7]]),
            NBD_EINVAL
        );
    }

    #[test]
    fn write_payload_length_mismatch_is_einval() {
        let mut b = MemBackend {
            data: vec![0u8; 1 << 16],
            bs: 4096,
        };
        // len diz 4096 mas payload tem 8 bytes
        let r = serve(&req(Command::Write, 0, 4096), &[0u8; 8], &mut b);
        assert_eq!(
            u32::from_be_bytes([r.reply[4], r.reply[5], r.reply[6], r.reply[7]]),
            NBD_EINVAL
        );
    }

    #[test]
    fn disc_signals_disconnect() {
        let mut b = MemBackend {
            data: vec![0u8; 4096],
            bs: 4096,
        };
        let r = serve(&req(Command::Disc, 0, 0), &[], &mut b);
        assert!(r.disconnect);
    }
}
