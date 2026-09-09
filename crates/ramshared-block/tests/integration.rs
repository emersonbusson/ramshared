use ramshared_block::{
    BlockBackend, Command, Inflight, Request,
    serve,
};

struct DummyBackend {
    data: Vec<u8>,
    bs: u32,
}

impl BlockBackend for DummyBackend {
    fn size_bytes(&self) -> u64 {
        self.data.len() as u64
    }
    fn block_size(&self) -> u32 {
        self.bs
    }
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> Result<(), ramshared_block::IoError> {
        let o = off as usize;
        buf.copy_from_slice(&self.data[o..o + buf.len()]);
        Ok(())
    }
    fn write_at(&mut self, off: u64, data: &[u8]) -> Result<(), ramshared_block::IoError> {
        let o = off as usize;
        self.data[o..o + data.len()].copy_from_slice(data);
        Ok(())
    }
    fn flush(&mut self) -> Result<(), ramshared_block::IoError> {
        Ok(())
    }
}

fn make_req(cmd: Command, offset: u64, len: u32) -> Request {
    Request {
        flags: 0,
        cmd,
        handle: 123,
        offset,
        len,
    }
}

#[test]
fn test_block_lifecycle_init_io_teardown() {
    let mut backend = DummyBackend {
        data: vec![0u8; 8192],
        bs: 4096,
    };

    // Configure: Inflight tracking
    let mut inflight = Inflight::new();
    assert!(inflight.try_insert(0, 4096)); // Returns true if successfully inserted

    // Serve I/O
    let write_req = make_req(Command::Write, 0, 4096);
    let payload = vec![0xAB; 4096];
    let write_outcome = serve(&write_req, &payload, &mut backend);
    assert_eq!(&write_outcome.reply[4..8], &[0, 0, 0, 0]); // NBD_OK

    let read_req = make_req(Command::Read, 0, 4096);
    let read_outcome = serve(&read_req, &[], &mut backend);
    assert_eq!(read_outcome.read_data, vec![0xAB; 4096]);

    // Teardown
    inflight.remove(0, 4096);

    let disc_req = make_req(Command::Disc, 0, 0);
    let disc_outcome = serve(&disc_req, &[], &mut backend);
    assert!(disc_outcome.disconnect);
}

#[test]
fn test_block_lifecycle_invalid_teardown() {
    let mut backend = DummyBackend {
        data: vec![0u8; 8192],
        bs: 4096,
    };
    let mut inflight = Inflight::new();
    assert!(inflight.try_insert(0, 4096));

    // Don't remove from inflight
    // Teardown with inflight I/O
    let disc_req = make_req(Command::Disc, 0, 0);
    let disc_outcome = serve(&disc_req, &[], &mut backend);
    assert!(disc_outcome.disconnect); // NBD disc does not care about inflight here, just protocol
}

#[test]
fn test_block_lifecycle_zero_len_io() {
    let mut backend = DummyBackend {
        data: vec![0u8; 8192],
        bs: 4096,
    };

    // Serve I/O zero length -> NBD allows 0 len for some commands or returns NBD_OK. Let's just expect NBD_OK for read.
    let read_req = make_req(Command::Read, 0, 0);
    let read_outcome = serve(&read_req, &[], &mut backend);
    assert_eq!(&read_outcome.reply[4..8], &[0, 0, 0, 0]); // NBD_OK (0)
}

#[test]
fn test_block_lifecycle_boundary_io() {
    let mut backend = DummyBackend {
        data: vec![0u8; 8192],
        bs: 4096,
    };

    // Serve I/O out of bounds
    let write_req = make_req(Command::Write, 8192, 4096);
    let payload = vec![0xAB; 4096];
    let write_outcome = serve(&write_req, &payload, &mut backend);
    assert_eq!(&write_outcome.reply[4..8], &[0, 0, 0, 34]); // NBD_ERANGE (34)
}

#[test]
fn test_block_lifecycle_unaligned_io() {
    let mut backend = DummyBackend {
        data: vec![0u8; 8192],
        bs: 4096,
    };

    // Serve I/O unaligned length
    let read_req = make_req(Command::Read, 0, 123);
    let read_outcome = serve(&read_req, &[], &mut backend);
    assert_eq!(&read_outcome.reply[4..8], &[0, 0, 0, 22]); // NBD_EINVAL (22)

    // Serve I/O unaligned offset
    let read_req_off = make_req(Command::Read, 123, 4096);
    let read_outcome_off = serve(&read_req_off, &[], &mut backend);
    assert_eq!(&read_outcome_off.reply[4..8], &[0, 0, 0, 22]); // NBD_EINVAL (22)
}

#[test]
fn test_block_lifecycle_write_fua() {
    struct FlushBackend {
        flushes: usize,
    }

    impl BlockBackend for FlushBackend {
        fn size_bytes(&self) -> u64 { 4096 }
        fn block_size(&self) -> u32 { 4096 }
        fn read_at(&mut self, _off: u64, _buf: &mut [u8]) -> Result<(), ramshared_block::IoError> { Ok(()) }
        fn write_at(&mut self, _off: u64, _data: &[u8]) -> Result<(), ramshared_block::IoError> { Ok(()) }
        fn flush(&mut self) -> Result<(), ramshared_block::IoError> {
            self.flushes += 1;
            Ok(())
        }
    }

    let mut backend = FlushBackend { flushes: 0 };

    let mut req = make_req(Command::Write, 0, 4096);
    req.flags = ramshared_block::protocol::NBD_CMD_FLAG_FUA;

    let outcome = serve(&req, &[0u8; 4096], &mut backend);
    assert_eq!(&outcome.reply[4..8], &[0, 0, 0, 0]); // NBD_OK
    assert_eq!(backend.flushes, 1);
}

#[test]
fn test_block_lifecycle_read_only() {
    struct RoBackend;

    impl BlockBackend for RoBackend {
        fn size_bytes(&self) -> u64 { 4096 }
        fn block_size(&self) -> u32 { 4096 }
        fn is_read_only(&self) -> bool { true }
        fn read_at(&mut self, _off: u64, _buf: &mut [u8]) -> Result<(), ramshared_block::IoError> { Ok(()) }
        fn write_at(&mut self, _off: u64, _data: &[u8]) -> Result<(), ramshared_block::IoError> { Ok(()) }
        fn flush(&mut self) -> Result<(), ramshared_block::IoError> { Ok(()) }
    }

    let mut backend = RoBackend;

    let req = make_req(Command::Write, 0, 4096);
    let outcome = serve(&req, &[0u8; 4096], &mut backend);
    assert_eq!(&outcome.reply[4..8], &[0, 0, 0, 13]); // NBD_EACCES (13)
}
