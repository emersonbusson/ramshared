use crate::protocol::{Command, NBD_ENOSPC, Request};
use crate::request::{serve, BlockBackend, IoError};

struct ChaosBackend;

impl BlockBackend for ChaosBackend {
    fn size_bytes(&self) -> u64 {
        4096
    }
    fn block_size(&self) -> u32 {
        4096
    }
    fn read_at(&mut self, _off: u64, _buf: &mut [u8]) -> Result<(), IoError> {
        Err(IoError::Retryable("chaos read network glitch".to_string()))
    }
    fn write_at(&mut self, _off: u64, _data: &[u8]) -> Result<(), IoError> {
        Err(IoError::Retryable("chaos write network glitch".to_string()))
    }
    fn flush(&mut self) -> Result<(), IoError> {
        Err(IoError::Retryable("chaos flush network glitch".to_string()))
    }
}

#[test]
fn retryable_error_returns_enospc() {
    let mut b = ChaosBackend;
    let req = Request {
        flags: 0,
        cmd: Command::Write,
        handle: 0x42,
        offset: 0,
        len: 4096,
    };
    let w = serve(&req, &vec![0; 4096], &mut b);
    assert_eq!(
        u32::from_be_bytes([w.reply[4], w.reply[5], w.reply[6], w.reply[7]]),
        NBD_ENOSPC
    );
}
