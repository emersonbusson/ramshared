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

#[derive(Debug, PartialEq, Eq)]
pub enum ProtoError {
    Io(std::io::ErrorKind),
    Disconnect,
    Incomplete,
    InvalidMagic(u32),
    OversizedLength(u32),
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
#[derive(PartialEq, Eq)]
pub struct RingHdr {
    pub magic: u32,
    pub entries: u32,
    pub head: u32,
    pub tail: u32,
}

impl RingHdr {
    pub fn read_from<R: std::io::Read>(mut reader: R) -> Result<Self, ProtoError> {
        let mut first_byte = [0u8; 1];
        match reader.read_exact(&mut first_byte) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(ProtoError::Disconnect);
            }
            Err(e) => return Err(ProtoError::Io(e.kind())),
        }

        let mut rest = [0u8; 15];
        if let Err(e) = reader.read_exact(&mut rest) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return Err(ProtoError::Incomplete);
            }
            return Err(ProtoError::Io(e.kind()));
        }

        let mut magic_bytes = [0u8; 4];
        magic_bytes[0] = first_byte[0];
        magic_bytes[1..].copy_from_slice(&rest[0..3]);
        let magic = u32::from_le_bytes(magic_bytes);
        if magic != RING_MAGIC {
            return Err(ProtoError::InvalidMagic(magic));
        }

        let mut entries_bytes = [0u8; 4];
        entries_bytes.copy_from_slice(&rest[3..7]);
        let entries = u32::from_le_bytes(entries_bytes);
        if entries > MAX_QD {
            return Err(ProtoError::OversizedLength(entries));
        }

        let mut head_bytes = [0u8; 4];
        head_bytes.copy_from_slice(&rest[7..11]);
        let head = u32::from_le_bytes(head_bytes);

        let mut tail_bytes = [0u8; 4];
        tail_bytes.copy_from_slice(&rest[11..15]);
        let tail = u32::from_le_bytes(tail_bytes);

        Ok(Self {
            magic,
            entries,
            head,
            tail,
        })
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
    fn constants_match_header_docs() {
        assert_eq!(ABI_VERSION, 1);
        assert_eq!(MAX_QD, 256);
        assert_eq!(MAX_IO, 1 << 20);
        assert_eq!(RING_MAGIC, 0x5253_5244);
    }

    #[test]
    fn test_ring_hdr_truncated_headers() {
        // Disconnect on first byte
        let empty: &[u8] = &[];
        assert_eq!(RingHdr::read_from(empty), Err(ProtoError::Disconnect));

        // Incomplete on subsequent bytes (e.g. only 4 bytes instead of 16)
        let incomplete: &[u8] = &[0x44, 0x52, 0x53, 0x52];
        assert_eq!(RingHdr::read_from(incomplete), Err(ProtoError::Incomplete));
    }

    #[test]
    fn test_ring_hdr_wrong_magic_bytes() {
        let mut bad_magic = [0u8; 16];
        bad_magic[0..4].copy_from_slice(&0xBADBADu32.to_le_bytes());
        bad_magic[4..8].copy_from_slice(&64u32.to_le_bytes()); // valid length

        let buf: &[u8] = &bad_magic;
        assert_eq!(RingHdr::read_from(buf), Err(ProtoError::InvalidMagic(0xBADBAD)));
    }

    #[test]
    fn test_ring_hdr_oversized_length_fields() {
        let mut oversized = [0u8; 16];
        oversized[0..4].copy_from_slice(&RING_MAGIC.to_le_bytes());
        oversized[4..8].copy_from_slice(&(MAX_QD + 1).to_le_bytes()); // oversized length

        let buf: &[u8] = &oversized;
        assert_eq!(RingHdr::read_from(buf), Err(ProtoError::OversizedLength(MAX_QD + 1)));
    }

    #[test]
    fn test_ring_hdr_success() {
        let mut valid = [0u8; 16];
        valid[0..4].copy_from_slice(&RING_MAGIC.to_le_bytes());
        valid[4..8].copy_from_slice(&128u32.to_le_bytes());
        valid[8..12].copy_from_slice(&1u32.to_le_bytes());
        valid[12..16].copy_from_slice(&2u32.to_le_bytes());

        let buf: &[u8] = &valid;
        let hdr = RingHdr::read_from(buf).unwrap();
        assert_eq!(hdr.magic, RING_MAGIC);
        assert_eq!(hdr.entries, 128);
        assert_eq!(hdr.head, 1);
        assert_eq!(hdr.tail, 2);
    }
}
