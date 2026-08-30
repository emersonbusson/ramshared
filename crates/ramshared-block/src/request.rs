//! NBD request dispatch → [`BlockBackend`], with §8 validation
//! (block size alignment, range check) and error mapping → NBD errno.

use crate::protocol::{Command, NBD_CMD_FLAG_FUA, Request, SIMPLE_REPLY_LEN, encode_simple_reply};

// errno in simple reply (error field).
pub const NBD_OK: u32 = 0;
pub const NBD_EIO: u32 = 5;
pub const NBD_EINVAL: u32 = 22;

/// Storage backend error (e.g., CUDA failure in the hot path).
#[derive(Debug)]
pub struct IoError(pub String);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WriteOptions {
    pub fua: bool,
}

/// Storage behind the NBD device (VRAM, in our case).
pub trait BlockBackend {
    fn size_bytes(&self) -> u64;
    /// Logical block size (multiple of 512; 4096 in the MVP — SPEC §8).
    fn block_size(&self) -> u32;
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), IoError>;
    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError>;
    fn write_at_with_options(
        &mut self,
        off: u64,
        data: &[u8],
        options: WriteOptions,
    ) -> Result<(), IoError> {
        self.write_at(off, data)?;
        if options.fua {
            self.flush()?;
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<(), IoError>;
}

/// Outcome of the dispatch: reply bytes, read data (if READ) and whether the
/// client requested disconnect (`NBD_CMD_DISC`).
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

/// Validates alignment and range (SPEC §8): unaligned or out of range = EINVAL,
/// **before** touching the backend.
fn validate<B: BlockBackend + ?Sized>(req: &Request, backend: &B) -> Result<(), u32> {
    let bs = backend.block_size() as u64;
    if bs == 0 {
        return Err(NBD_EINVAL);
    }
    if !req.offset.is_multiple_of(bs) {
        return Err(NBD_EINVAL);
    }
    if !(req.len as u64).is_multiple_of(bs) {
        return Err(NBD_EINVAL);
    }

    let Some(end) = req.offset.checked_add(req.len as u64) else {
        return Err(NBD_EINVAL);
    };
    if end > backend.size_bytes() {
        return Err(NBD_EINVAL);
    }

    Ok(())
}

fn validate_command_flags(req: &Request) -> Result<WriteOptions, u32> {
    match req.cmd {
        Command::Write if req.flags & !NBD_CMD_FLAG_FUA == 0 => Ok(WriteOptions {
            fua: req.flags & NBD_CMD_FLAG_FUA != 0,
        }),
        Command::Write => Err(NBD_EINVAL),
        _ if req.flags == 0 => Ok(WriteOptions::default()),
        _ => Err(NBD_EINVAL),
    }
}

/// Dispatches an already parsed request. `payload` is the WRITE data (empty for
/// others). Does no socket I/O — only logic (testable without root).
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

    let options = match validate_command_flags(req) {
        Ok(options) => options,
        Err(error) => return plain(error),
    };

    match req.cmd {
        Command::Disc => ServeOutcome {
            reply: reply(NBD_OK),
            read_data: Vec::new(),
            disconnect: true,
        },
        Command::Flush => plain(errno_of(backend.flush())),
        Command::Trim => plain(NBD_OK), // safe no-op in the MVP
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
            plain(errno_of(
                backend.write_at_with_options(req.offset, payload, options),
            ))
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
        fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
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

    struct CountingBackend {
        writes: usize,
        flushes: usize,
    }

    impl BlockBackend for CountingBackend {
        fn size_bytes(&self) -> u64 {
            4096
        }

        fn block_size(&self) -> u32 {
            4096
        }

        fn read_at(&mut self, _off: u64, buf: &mut [u8]) -> Result<(), IoError> {
            buf.fill(0);
            Ok(())
        }

        fn write_at(&mut self, _off: u64, _data: &[u8]) -> Result<(), IoError> {
            self.writes += 1;
            Ok(())
        }

        fn flush(&mut self) -> Result<(), IoError> {
            self.flushes += 1;
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
        // len says 4096 but payload has 8 bytes
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

    #[test]
    fn unknown_write_flags_refuse_before_mutation() {
        let mut backend = CountingBackend {
            writes: 0,
            flushes: 0,
        };
        let mut request = req(Command::Write, 0, 4096);
        request.flags = 1 << 15;

        let outcome = serve(&request, &[0; 4096], &mut backend);

        assert_eq!(
            u32::from_be_bytes([
                outcome.reply[4],
                outcome.reply[5],
                outcome.reply[6],
                outcome.reply[7],
            ]),
            NBD_EINVAL
        );
        assert_eq!(backend.writes, 0);
        assert_eq!(backend.flushes, 0);
    }

    #[test]
    fn fua_write_reaches_backend_flush_before_success() {
        let mut backend = CountingBackend {
            writes: 0,
            flushes: 0,
        };
        let mut request = req(Command::Write, 0, 4096);
        request.flags = NBD_CMD_FLAG_FUA;

        let outcome = serve(&request, &[0; 4096], &mut backend);

        assert_eq!(
            u32::from_be_bytes([
                outcome.reply[4],
                outcome.reply[5],
                outcome.reply[6],
                outcome.reply[7],
            ]),
            NBD_OK
        );
        assert_eq!(backend.writes, 1);
        assert_eq!(backend.flushes, 1);
    }

    #[test]
    fn zero_block_size_is_einval() {
        let mut b = MemBackend {
            data: vec![0u8; 4096],
            bs: 0,
        };
        let r = serve(&req(Command::Read, 0, 0), &[], &mut b);
        assert_eq!(
            u32::from_be_bytes([r.reply[4], r.reply[5], r.reply[6], r.reply[7]]),
            NBD_EINVAL
        );
    }

    #[test]
    fn unaligned_length_is_einval() {
        let mut b = MemBackend {
            data: vec![0u8; 8192],
            bs: 4096,
        };
        let r = serve(&req(Command::Read, 0, 100), &[], &mut b);
        assert_eq!(
            u32::from_be_bytes([r.reply[4], r.reply[5], r.reply[6], r.reply[7]]),
            NBD_EINVAL
        );
    }

    #[test]
    fn extent_overflow_is_einval() {
        let mut b = MemBackend {
            data: vec![0u8; 4096],
            bs: 4096,
        };
        let r = serve(&req(Command::Read, u64::MAX - 4095, 8192), &[], &mut b);
        assert_eq!(
            u32::from_be_bytes([r.reply[4], r.reply[5], r.reply[6], r.reply[7]]),
            NBD_EINVAL
        );
    }
}
