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

    // WRITE: o buffer (já preenchido) vai para o backend no offset 1024.
    let written =
        ublk_server::serve_request(&req(Command::Write, 1024, 512), &mut backend, &mut buf);
    assert_eq!(written, 512);

    // READ do mesmo offset: o backend preenche o buffer; deve bater byte a byte.
    let mut rbuf = vec![0u8; 512];
    let read = ublk_server::serve_request(&req(Command::Read, 1024, 512), &mut backend, &mut rbuf);
    assert_eq!(read, 512);
    assert_eq!(rbuf, buf);
}

#[test]
fn serve_request_handles_flush_and_rejects_oversized_or_oob() {
    let mut backend = RamBackend::new(1024);
    let mut buf = vec![0u8; 512];

    // FLUSH: sucesso com result 0.
    assert_eq!(
        ublk_server::serve_request(&req(Command::Flush, 0, 0), &mut backend, &mut buf),
        0
    );

    // Request maior que o buffer disponível => -EINVAL.
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 0, 1024), &mut backend, &mut buf),
        -22
    );

    // READ fora do backend => -EIO.
    assert_eq!(
        ublk_server::serve_request(&req(Command::Read, 51200, 512), &mut backend, &mut buf),
        -5
    );
}
