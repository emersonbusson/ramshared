use ramshared_block::handshake::*;
use std::io::{self, Read};

struct SleepyReader {
    step: usize,
}
impl Read for SleepyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.step == 0 {
            buf[0..4].copy_from_slice(&(ramshared_block::protocol::NBD_FLAG_FIXED_NEWSTYLE as u32).to_be_bytes());
            self.step += 1;
            Ok(4)
        } else {
            std::thread::sleep(std::time::Duration::from_millis(6000));
            Err(io::Error::new(io::ErrorKind::WouldBlock, "timeout"))
        }
    }
}

#[test]
fn test_handshake_timeout() {
    let mut r = SleepyReader { step: 0 };
    let mut w = std::io::sink();
    let exports = vec![Export { name: "default".into(), size: 4096 }];
    let res = server_handshake(&mut r, &mut w, &exports, 1);
    assert!(matches!(res, Err(HandshakeError::Timeout) | Err(HandshakeError::Io(_))));
}
