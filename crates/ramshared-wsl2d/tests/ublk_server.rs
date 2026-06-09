use ramshared_wsl2d::{ublk, ublk_server};

fn iod(op: u8, nr_sectors: u32, start_sector: u64) -> ublk::IoDesc {
    ublk::IoDesc {
        op_flags: u32::from(op),
        nr_sectors_or_zones: nr_sectors,
        start_sector,
        addr: 0,
    }
}

#[test]
fn ram_backend_serves_write_then_read_roundtrip() {
    let mut backend = ublk_server::RamBackend::new(8192);
    let mut buf = vec![0u8; 512];
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i % 251) as u8;
    }

    // WRITE: o buffer (que o kernel ja preencheu) vai para o backend no setor 2.
    let written =
        ublk_server::serve_request(&mut backend, &iod(ublk::UBLK_IO_OP_WRITE, 1, 2), &mut buf);
    assert_eq!(written, 512);

    // READ do mesmo setor: o backend preenche o buffer; deve bater byte a byte.
    let mut rbuf = vec![0u8; 512];
    let read =
        ublk_server::serve_request(&mut backend, &iod(ublk::UBLK_IO_OP_READ, 1, 2), &mut rbuf);
    assert_eq!(read, 512);
    assert_eq!(rbuf, buf);
}

#[test]
fn serve_request_handles_flush_and_rejects_unsupported_or_oob() {
    let mut backend = ublk_server::RamBackend::new(1024);
    let mut buf = vec![0u8; 512];

    // FLUSH: sem dados, sucesso com result 0.
    assert_eq!(
        ublk_server::serve_request(&mut backend, &iod(ublk::UBLK_IO_OP_FLUSH, 0, 0), &mut buf),
        0
    );

    // WRITE_ZEROES nao tem equivalencia segura => -EINVAL.
    assert_eq!(
        ublk_server::serve_request(
            &mut backend,
            &iod(ublk::UBLK_IO_OP_WRITE_ZEROES, 1, 0),
            &mut buf
        ),
        -22
    );

    // READ fora do backend => -EIO.
    assert_eq!(
        ublk_server::serve_request(&mut backend, &iod(ublk::UBLK_IO_OP_READ, 1, 100), &mut buf),
        -5
    );
}
