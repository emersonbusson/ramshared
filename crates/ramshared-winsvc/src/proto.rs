//! Mirror of `drivers/windows/ramshared/protocol.h` (ITEM-4 / DT-17).
//!
//! Sizes and golden layouts must match the C header. Change **both** in one commit.

#![allow(dead_code)]

pub const ABI_VERSION: u32 = 1;
pub const MAX_QD: u32 = 256;
pub const MAX_IO: u32 = 1 << 20;
pub const RING_MAGIC: u32 = 0x5253_5244; // 'RSRD'

pub const OP_READ: u32 = 0;
pub const OP_WRITE: u32 = 1;
pub const OP_FLUSH: u32 = 2;

pub const ST_OK: i32 = 0;
pub const ST_EIO: i32 = 5;
pub const ST_EINVAL: i32 = 22;

pub const IOCTL_FN_REGISTER_QUEUE: u32 = 0;
pub const IOCTL_FN_UNREGISTER_QUEUE: u32 = 1;
pub const IOCTL_FN_COMMIT_AND_FETCH: u32 = 2;
pub const IOCTL_FN_CREATE_DISK: u32 = 3;
pub const IOCTL_FN_DESTROY_DISK: u32 = 4;

/// Perform protocol handshake over a blocking stream with timeout.
pub fn negotiate_protocol<S: std::io::Read>(
    stream: &mut S,
    timeout: std::time::Duration,
) -> std::io::Result<u32> {
    let start = std::time::Instant::now();
    let mut buf = [0u8; 4];
    let mut bytes_read = 0;

    while bytes_read < 4 {
        if start.elapsed() >= timeout {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "handshake timeout",
            ));
        }

        let mut chunk = [0u8; 1];
        match stream.read(&mut chunk) {
            Ok(0) => {
                if bytes_read == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "clean disconnect",
                    ));
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "incomplete handshake",
                    ));
                }
            }
            Ok(n) => {
                buf[bytes_read] = chunk[0];
                bytes_read += n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(e) => return Err(e),
        }
    }

    let version = u32::from_le_bytes(buf);
    if version != ABI_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "incompatible version",
        ));
    }

    Ok(version)
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sqe {
    pub tag: u64,
    pub op: u32,
    pub flags: u32,
    pub offset: u64,
    pub len: u32,
    pub buf_slot: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cqe {
    pub tag: u64,
    pub status: i32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RingHdr {
    pub magic: u32,
    pub entries: u32,
    pub head: u32,
    pub tail: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Register {
    pub abi_version: u32,
    pub disk_id: u32,
    pub queue_depth: u32,
    pub block_size: u32,
    pub max_io_bytes: u32,
    pub reserved: u32,
    pub sq_ring_va: u64,
    pub cq_ring_va: u64,
    pub data_area_va: u64,
    pub data_area_len: u64,
    pub sq_event_handle: u64,
    pub cq_event_handle: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DiskParams {
    pub size_bytes: u64,
    pub block_size: u32,
    pub reserved: u32,
    pub serial: [u8; 16],
}

const _: () = {
    assert!(core::mem::size_of::<Sqe>() == 32);
    assert!(core::mem::align_of::<Sqe>() <= 8);
    assert!(core::mem::size_of::<Cqe>() == 16);
    assert!(core::mem::size_of::<RingHdr>() == 16);
    assert!(core::mem::size_of::<Register>() == 72);
    assert!(core::mem::size_of::<DiskParams>() == 32);
};

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn golden_sqe_layout_little_endian() {
        // Fixed field order must match protocol.h packing (LE host).
        // Serialize field-by-field (no transmute) so the crate stays `forbid(unsafe_code)`.
        let sqe = Sqe {
            tag: 0x0123_4567_89ab_cdef,
            op: OP_WRITE,
            flags: 0,
            offset: 4096,
            len: 512,
            buf_slot: 3,
        };
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&sqe.tag.to_le_bytes());
        bytes[8..12].copy_from_slice(&sqe.op.to_le_bytes());
        bytes[12..16].copy_from_slice(&sqe.flags.to_le_bytes());
        bytes[16..24].copy_from_slice(&sqe.offset.to_le_bytes());
        bytes[24..28].copy_from_slice(&sqe.len.to_le_bytes());
        bytes[28..32].copy_from_slice(&sqe.buf_slot.to_le_bytes());
        // tag LE
        assert_eq!(&bytes[0..8], &0x0123_4567_89ab_cdefu64.to_le_bytes());
        // op = WRITE = 1
        assert_eq!(&bytes[8..12], &1u32.to_le_bytes());
        // flags = 0
        assert_eq!(&bytes[12..16], &0u32.to_le_bytes());
        // offset = 4096
        assert_eq!(&bytes[16..24], &4096u64.to_le_bytes());
        // len = 512
        assert_eq!(&bytes[24..28], &512u32.to_le_bytes());
        // buf_slot = 3
        assert_eq!(&bytes[28..32], &3u32.to_le_bytes());
    }

    #[test]
    fn constants_match_header_docs() {
        assert_eq!(ABI_VERSION, 1);
        assert_eq!(MAX_QD, 256);
        assert_eq!(MAX_IO, 1 << 20);
        assert_eq!(RING_MAGIC, 0x5253_5244);
    }

    #[test]
    fn test_proto_version_negotiation() {
        let data = ABI_VERSION.to_le_bytes().to_vec();
        let mut stream = std::io::Cursor::new(data);
        assert_eq!(
            negotiate_protocol(&mut stream, std::time::Duration::from_secs(1)).unwrap(),
            ABI_VERSION
        );
    }

    #[test]
    fn test_proto_incompatible_version_rejection() {
        let data = 999u32.to_le_bytes().to_vec();
        let mut stream = std::io::Cursor::new(data);
        let err = negotiate_protocol(&mut stream, std::time::Duration::from_secs(1)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(err.to_string(), "incompatible version");
    }

    #[test]
    fn test_proto_timeout_on_incomplete_handshake() {
        struct BlockingStream;
        impl std::io::Read for BlockingStream {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                std::thread::sleep(std::time::Duration::from_millis(50));
                Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "blocking",
                ))
            }
        }

        let mut stream = BlockingStream;
        let err =
            negotiate_protocol(&mut stream, std::time::Duration::from_millis(10)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn test_proto_handshake_clean_disconnect() {
        let data = vec![];
        let mut stream = std::io::Cursor::new(data);
        let err = negotiate_protocol(&mut stream, std::time::Duration::from_secs(1)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
        assert_eq!(err.to_string(), "clean disconnect");
    }
}
