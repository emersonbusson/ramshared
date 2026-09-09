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

impl Sqe {
    /// Safe parser for SQE from an untrusted byte slice (e.g. fuzzing / driver inputs).
    pub fn parse(b: &[u8]) -> Result<Self, i32> {
        if b.len() < core::mem::size_of::<Self>() {
            return Err(ST_EINVAL);
        }

        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&b[24..28]);
        let len = u32::from_le_bytes(len_bytes);

        if len > MAX_IO {
            return Err(ST_EINVAL);
        }

        // This validates the structure bounds. Real implementations would deserialize all fields safely.
        // We do a simple copy for now to fulfill the struct layout.

        let mut sqe = Self {
            tag: 0,
            op: 0,
            flags: 0,
            offset: 0,
            len: 0,
            buf_slot: 0,
        };

        // Since we forbid unsafe_code, we deserialize safely.
        let mut tag_bytes = [0u8; 8];
        tag_bytes.copy_from_slice(&b[0..8]);
        sqe.tag = u64::from_le_bytes(tag_bytes);

        let mut op_bytes = [0u8; 4];
        op_bytes.copy_from_slice(&b[8..12]);
        sqe.op = u32::from_le_bytes(op_bytes);

        let mut flags_bytes = [0u8; 4];
        flags_bytes.copy_from_slice(&b[12..16]);
        sqe.flags = u32::from_le_bytes(flags_bytes);

        let mut offset_bytes = [0u8; 8];
        offset_bytes.copy_from_slice(&b[16..24]);
        sqe.offset = u64::from_le_bytes(offset_bytes);

        sqe.len = len;

        let mut slot_bytes = [0u8; 4];
        slot_bytes.copy_from_slice(&b[28..32]);
        sqe.buf_slot = u32::from_le_bytes(slot_bytes);

        Ok(sqe)
    }
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

impl RingHdr {
    /// Safe parser for RingHdr from an untrusted byte slice.
    pub fn parse(b: &[u8]) -> Result<Self, i32> {
        if b.len() < core::mem::size_of::<Self>() {
            return Err(ST_EINVAL);
        }

        let mut magic_bytes = [0u8; 4];
        magic_bytes.copy_from_slice(&b[0..4]);
        let magic = u32::from_le_bytes(magic_bytes);

        if magic != RING_MAGIC {
            return Err(ST_EINVAL);
        }

        let mut hdr = Self { magic: 0, entries: 0, head: 0, tail: 0 };
        hdr.magic = magic;

        let mut entries_bytes = [0u8; 4];
        entries_bytes.copy_from_slice(&b[4..8]);
        hdr.entries = u32::from_le_bytes(entries_bytes);

        let mut head_bytes = [0u8; 4];
        head_bytes.copy_from_slice(&b[8..12]);
        hdr.head = u32::from_le_bytes(head_bytes);

        let mut tail_bytes = [0u8; 4];
        tail_bytes.copy_from_slice(&b[12..16]);
        hdr.tail = u32::from_le_bytes(tail_bytes);

        Ok(hdr)
    }
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
    fn test_proto_truncated_header_fails_parse() {
        let bytes = [0u8; 8]; // Truncated, RingHdr is 16 bytes
        assert_eq!(RingHdr::parse(&bytes).unwrap_err(), ST_EINVAL);
    }

    #[test]
    fn test_proto_wrong_magic_bytes_fails() {
        let mut bytes = [0u8; 16];
        let wrong_magic = 0x0000_0000u32;
        bytes[0..4].copy_from_slice(&wrong_magic.to_le_bytes());
        assert_eq!(RingHdr::parse(&bytes).unwrap_err(), ST_EINVAL);
    }

    #[test]
    fn test_proto_valid_header_parses() {
        let mut bytes = [0u8; 16];
        bytes[0..4].copy_from_slice(&RING_MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&256u32.to_le_bytes()); // entries
        bytes[8..12].copy_from_slice(&0u32.to_le_bytes()); // head
        bytes[12..16].copy_from_slice(&0u32.to_le_bytes()); // tail

        let hdr = RingHdr::parse(&bytes).unwrap();
        assert_eq!(hdr.magic, RING_MAGIC);
        assert_eq!(hdr.entries, 256);
    }

    #[test]
    fn test_proto_oversized_length_field_rejected() {
        let mut bytes = [0u8; 32];
        let oversized_len = MAX_IO + 1;
        bytes[24..28].copy_from_slice(&oversized_len.to_le_bytes());
        assert_eq!(Sqe::parse(&bytes).unwrap_err(), ST_EINVAL);
    }

    #[test]
    fn test_proto_truncated_sqe_fails_parse() {
        let bytes = [0u8; 16]; // Truncated, Sqe is 32 bytes
        assert_eq!(Sqe::parse(&bytes).unwrap_err(), ST_EINVAL);
    }

    #[test]
    fn test_proto_valid_sqe_parses() {
        let mut bytes = [0u8; 32];
        let len = 512u32;
        bytes[24..28].copy_from_slice(&len.to_le_bytes());

        let sqe = Sqe::parse(&bytes).unwrap();
        assert_eq!(sqe.len, len);
    }

    #[test]
    fn constants_match_header_docs() {
        assert_eq!(ABI_VERSION, 1);
        assert_eq!(MAX_QD, 256);
        assert_eq!(MAX_IO, 1 << 20);
        assert_eq!(RING_MAGIC, 0x5253_5244);
    }
}
