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

    // READ outside the backend => -ERANGE.
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 51200, 512), &mut backend, &mut buf),
        -34
    );
}

#[test]
fn serve_request_guards_against_out_of_bounds_upfront() {
    let mut backend = RamBackend::new(4096);
    let mut buf = vec![0u8; 4096];

    // Exact fit
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 0, 4096), &mut backend, &mut buf),
        4096
    );
    assert_eq!(
        ublk_server::serve_request(&req(Command::Write, 2048, 2048), &mut backend, &mut buf),
        2048
    );

    // Overflow offset + len
    assert_eq!(
        ublk_server::serve_request(
            &req(Command::Read, u64::MAX - 511, 1024),
            &mut backend,
            &mut buf
        ),
        -34 // ERANGE
    );

    // Exceeds backend size bytes
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 4096, 512), &mut backend, &mut buf),
        -34 // ERANGE
    );
    assert_eq!(
        ublk_server::serve_request(&req(Command::Write, 2048, 4096), &mut backend, &mut buf),
        -34 // ERANGE
    );
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

#[derive(Debug, PartialEq, Eq)]
enum BackendCall {
    ReadAt { offset: u64, len: usize },
    WriteAt { offset: u64, len: usize, data_sample: Vec<u8> },
    Flush,
}

struct MockBackend {
    size_bytes: u64,
    block_size: u32,
    fail_read: bool,
    fail_write: bool,
    fail_flush: bool,
    calls: Vec<BackendCall>,
}

impl MockBackend {
    fn new(size_bytes: u64, block_size: u32) -> Self {
        Self {
            size_bytes,
            block_size,
            fail_read: false,
            fail_write: false,
            fail_flush: false,
            calls: Vec::new(),
        }
    }
}

impl BlockBackend for MockBackend {
    fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    fn block_size(&self) -> u32 {
        self.block_size
    }

    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), IoError> {
        self.calls.push(BackendCall::ReadAt {
            offset: off,
            len: buf.len(),
        });
        if self.fail_read {
            Err(IoError("mock read failure".into()))
        } else {
            buf.fill(0xaa);
            Ok(())
        }
    }

    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), IoError> {
        self.calls.push(BackendCall::WriteAt {
            offset: off,
            len: data.len(),
            data_sample: data.to_vec(),
        });
        if self.fail_write {
            Err(IoError("mock write failure".into()))
        } else {
            Ok(())
        }
    }

    fn flush(&mut self) -> Result<(), IoError> {
        self.calls.push(BackendCall::Flush);
        if self.fail_flush {
            Err(IoError("mock flush failure".into()))
        } else {
            Ok(())
        }
    }
}

#[test]
fn serve_request_block_alignment_and_zero_block_size() {
    // 1. Block size = 512
    let mut backend = MockBackend::new(4096, 512);
    let mut buf = vec![0u8; 1024];

    // Offset unaligned (256) -> -EINVAL (-22)
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 256, 512), &mut backend, &mut buf),
        -22
    );
    assert!(backend.calls.is_empty());

    // Len unaligned (256) -> -EINVAL (-22)
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 512, 256), &mut backend, &mut buf),
        -22
    );
    assert!(backend.calls.is_empty());

    // Both aligned -> success (512 bytes)
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 512, 512), &mut backend, &mut buf),
        512
    );
    assert_eq!(
        backend.calls,
        vec![BackendCall::ReadAt {
            offset: 512,
            len: 512
        }]
    );

    // 2. Zero block size allows arbitrary non-zero offset and len alignment
    let mut zero_bs_backend = MockBackend::new(4096, 0);
    assert_eq!(
        ublk_server::serve_request(
            &req(Command::Read, 3, 7),
            &mut zero_bs_backend,
            &mut buf
        ),
        7
    );
    assert_eq!(
        zero_bs_backend.calls,
        vec![BackendCall::ReadAt { offset: 3, len: 7 }]
    );
}

#[test]
fn serve_request_mock_backend_interaction_and_trim() {
    let mut backend = MockBackend::new(4096, 512);
    let mut buf = vec![0u8; 1024];
    buf[..512].fill(0x55);

    // Write: records WriteAt with exact payload and offset
    assert_eq!(
        ublk_server::serve_request(&req(Command::Write, 1024, 512), &mut backend, &mut buf),
        512
    );
    assert_eq!(
        backend.calls,
        vec![BackendCall::WriteAt {
            offset: 1024,
            len: 512,
            data_sample: vec![0x55; 512],
        }]
    );

    // Flush: records Flush
    assert_eq!(
        ublk_server::serve_request(&req(Command::Flush, 0, 0), &mut backend, &mut buf),
        0
    );

    // Trim: no-op discard, returns 0 without touching backend methods
    assert_eq!(
        ublk_server::serve_request(&req(Command::Trim, 2048, 512), &mut backend, &mut buf),
        0
    );

    assert_eq!(
        backend.calls,
        vec![
            BackendCall::WriteAt {
                offset: 1024,
                len: 512,
                data_sample: vec![0x55; 512],
            },
            BackendCall::Flush,
        ]
    );
}

#[test]
fn serve_request_unsupported_commands_and_overflow() {
    let mut backend = MockBackend::new(4096, 512);
    let mut buf = vec![0u8; 1024];

    // Command::Disc => -EINVAL (-22)
    assert_eq!(
        ublk_server::serve_request(&req(Command::Disc, 0, 512), &mut backend, &mut buf),
        -22
    );

    // Command::Unknown(99) => -EINVAL (-22)
    assert_eq!(
        ublk_server::serve_request(&req(Command::Unknown(99), 0, 512), &mut backend, &mut buf),
        -22
    );

    // Offset + length arithmetic overflow => -ERANGE (-34)
    assert_eq!(
        ublk_server::serve_request(
            &req(Command::Read, u64::MAX - 511, 1024),
            &mut backend,
            &mut buf
        ),
        -34
    );

    assert!(backend.calls.is_empty());
}
