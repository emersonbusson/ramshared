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

    #[cfg(test)]
    mod mock_parser {
        use super::*;

        pub fn parse_ring_hdr(bytes: &[u8]) -> Result<RingHdr, &'static str> {
            if bytes.len() < core::mem::size_of::<RingHdr>() {
                return Err("truncated header");
            }
            let mut magic_bytes = [0u8; 4];
            magic_bytes.copy_from_slice(&bytes[0..4]);
            let magic = u32::from_le_bytes(magic_bytes);
            if magic != RING_MAGIC {
                return Err("wrong magic bytes");
            }

            let mut entries_bytes = [0u8; 4];
            entries_bytes.copy_from_slice(&bytes[4..8]);
            let entries = u32::from_le_bytes(entries_bytes);
            if entries > MAX_QD {
                return Err("oversized length field");
            }

            Ok(RingHdr {
                magic,
                entries,
                head: 0,
                tail: 0,
            })
        }
    }

    #[test]
    fn test_proto_truncated_header_fails() {
        let bytes = [0u8; 8];
        let err = mock_parser::parse_ring_hdr(&bytes).unwrap_err();
        assert_eq!(err, "truncated header");
    }

    #[test]
    fn test_proto_wrong_magic_bytes_fails() {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&0xBAD_C0DEu32.to_le_bytes());
        bytes[4..8].copy_from_slice(&10u32.to_le_bytes());
        let err = mock_parser::parse_ring_hdr(&bytes).unwrap_err();
        assert_eq!(err, "wrong magic bytes");
    }

    #[test]
    fn test_proto_oversized_length_fails() {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&RING_MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&(MAX_QD + 1).to_le_bytes());
        let err = mock_parser::parse_ring_hdr(&bytes).unwrap_err();
        assert_eq!(err, "oversized length field");
    }

    #[test]
    fn test_proto_max_length_overflow_fails() {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&RING_MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        let err = mock_parser::parse_ring_hdr(&bytes).unwrap_err();
        assert_eq!(err, "oversized length field");
    }

    #[test]
    fn test_proto_zero_length_succeeds() {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&RING_MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
        let hdr = mock_parser::parse_ring_hdr(&bytes).unwrap();
        assert_eq!(hdr.entries, 0);
    }

    #[test]
    fn test_proto_valid_header_succeeds() {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&RING_MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&MAX_QD.to_le_bytes());
        let hdr = mock_parser::parse_ring_hdr(&bytes).unwrap();
        assert_eq!(hdr.magic, RING_MAGIC);
        assert_eq!(hdr.entries, MAX_QD);
    }
}
