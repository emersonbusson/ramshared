use ramshared_block::{Command, Request};
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

    // READ outside the backend => -EIO.
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 51200, 512), &mut backend, &mut buf),
        -5
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
        ublk_server::serve_request(&req(Command::Read, u64::MAX, 1024), &mut backend, &mut buf),
        -5 // EIO
    );

    // Exceeds backend size bytes
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 4096, 512), &mut backend, &mut buf),
        -5 // EIO
    );
    assert_eq!(
        ublk_server::serve_request(&req(Command::Write, 2048, 4096), &mut backend, &mut buf),
        -5 // EIO
    );
}
