use ramshared_block::{BlockBackend, Command, IoError, Request};
use ramshared_wsl2d::{RamBackend, ublk_server};

fn req(cmd: Command, offset: u64, len: u32) -> Request {
    Request {
        flags: 0,
        cmd,
        handle: 0,
        offset,
        len,
    }
}

struct FailingBackend {
    fail_read: bool,
    fail_write: bool,
    fail_flush: bool,
}

impl BlockBackend for FailingBackend {
    fn size_bytes(&self) -> u64 {
        4096
    }

    fn block_size(&self) -> u32 {
        512
    }

    fn read_at(&mut self, _off: u64, _buf: &mut [u8]) -> Result<(), IoError> {
        if self.fail_read {
            Err(IoError("simulated read failure".into()))
        } else {
            Ok(())
        }
    }

    fn write_at(&mut self, _off: u64, _data: &[u8]) -> Result<(), IoError> {
        if self.fail_write {
            Err(IoError("simulated write failure".into()))
        } else {
            Ok(())
        }
    }

    fn flush(&mut self) -> Result<(), IoError> {
        if self.fail_flush {
            Err(IoError("simulated flush failure".into()))
        } else {
            Ok(())
        }
    }
}

#[test]
fn ram_backend_serves_write_then_read_roundtrip() {
    let mut backend = RamBackend::new(8192);
    let mut buf = vec![0u8; 512];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }

    // WRITE: the buffer (already filled) goes to the backend at offset 1024.
    let written =
        ublk_server::serve_request(&req(Command::Write, 1024, 512), &mut backend, &mut buf);
    assert_eq!(written, 512);

    // READ from the same offset: the backend fills the buffer; must match byte for byte.
    let mut rbuf = vec![0u8; 512];
    let read = ublk_server::serve_request(&req(Command::Read, 1024, 512), &mut backend, &mut rbuf);
    assert_eq!(read, 512);
    assert_eq!(rbuf, buf);
}

#[test]
fn serve_request_handles_flush_and_rejects_oversized_or_oob() {
    let mut backend = RamBackend::new(1024);
    let mut buf = vec![0u8; 512];

    // FLUSH: success with result 0.
    assert_eq!(
        ublk_server::serve_request(&req(Command::Flush, 0, 0), &mut backend, &mut buf),
        0
    );

    // Request larger than available buffer => -EINVAL.
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 0, 1024), &mut backend, &mut buf),
        -22
    );

    // READ outside the backend => -EIO.
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 51200, 512), &mut backend, &mut buf),
        -5
    );
}

#[test]
fn serve_request_propagates_backend_errors() {
    let mut buf = vec![0u8; 512];

    // Read error propagation -> -EIO (-5)
    let mut read_failing = FailingBackend {
        fail_read: true,
        fail_write: false,
        fail_flush: false,
    };
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 0, 512), &mut read_failing, &mut buf),
        -5
    );

    // Write error propagation -> -EIO (-5)
    let mut write_failing = FailingBackend {
        fail_read: false,
        fail_write: true,
        fail_flush: false,
    };
    assert_eq!(
        ublk_server::serve_request(&req(Command::Write, 0, 512), &mut write_failing, &mut buf),
        -5
    );

    // Flush error propagation -> -EIO (-5)
    let mut flush_failing = FailingBackend {
        fail_read: false,
        fail_write: false,
        fail_flush: true,
    };
    assert_eq!(
        ublk_server::serve_request(&req(Command::Flush, 0, 0), &mut flush_failing, &mut buf),
        -5
    );
}
