//! UAPI mínima do ublk usada pela Fase B.
//!
//! Fonte primária: `include/uapi/linux/ublk_cmd.h` do kernel WSL2 custom
//! `6.6.123.2-microsoft-standard-WSL2+`. Este módulo só espelha constantes,
//! layouts e helpers puros; abertura de `/dev/ublk-control` e `io_uring` ficam
//! para recortes posteriores.

use ramshared_block::{Command, Request};

pub const UBLK_SECTOR_SIZE: u64 = 512;

pub const UBLK_CMD_ADD_DEV: u32 = 0x04;
pub const UBLK_CMD_DEL_DEV: u32 = 0x05;
pub const UBLK_CMD_START_DEV: u32 = 0x06;
pub const UBLK_CMD_STOP_DEV: u32 = 0x07;
pub const UBLK_CMD_SET_PARAMS: u32 = 0x08;
pub const UBLK_CMD_GET_PARAMS: u32 = 0x09;
pub const UBLK_CMD_GET_DEV_INFO2: u32 = 0x12;
pub const UBLK_U_CMD_ADD_DEV: u32 = 0xc020_7504;
pub const UBLK_U_CMD_DEL_DEV: u32 = 0xc020_7505;
pub const UBLK_U_CMD_GET_FEATURES: u32 = 0x8020_7513;

pub const UBLK_IO_FETCH_REQ: u32 = 0x20;
pub const UBLK_IO_COMMIT_AND_FETCH_REQ: u32 = 0x21;
pub const UBLK_IO_NEED_GET_DATA: u32 = 0x22;

pub const UBLK_IO_RES_OK: i32 = 0;
pub const UBLK_IO_RES_NEED_GET_DATA: i32 = 1;
pub const UBLK_IO_RES_ABORT: i32 = -19; // -ENODEV
pub const UBLK_IO_RES_EINVAL: i32 = -22;

pub const UBLKSRV_CMD_BUF_OFFSET: u64 = 0;
pub const UBLKSRV_IO_BUF_OFFSET: u64 = 0x8000_0000;

pub const UBLK_FEATURES_LEN: u16 = 8;
pub const UBLK_QUEUE_ID_NONE: u16 = u16::MAX;
pub const UBLK_DEV_ID_AUTO: u32 = u32::MAX;
pub const UBLK_MAX_QUEUE_DEPTH: u64 = 4096;

pub const UBLK_IO_BUF_OFF: u32 = 0;
pub const UBLK_IO_BUF_BITS: u32 = 25;
pub const UBLK_IO_BUF_BITS_MASK: u64 = (1u64 << UBLK_IO_BUF_BITS) - 1;

pub const UBLK_TAG_OFF: u32 = UBLK_IO_BUF_BITS;
pub const UBLK_TAG_BITS: u32 = 16;
pub const UBLK_TAG_BITS_MASK: u64 = (1u64 << UBLK_TAG_BITS) - 1;

pub const UBLK_QID_OFF: u32 = UBLK_TAG_OFF + UBLK_TAG_BITS;
pub const UBLK_QID_BITS: u32 = 12;
pub const UBLK_QID_BITS_MASK: u64 = (1u64 << UBLK_QID_BITS) - 1;
pub const UBLK_MAX_NR_QUEUES: u64 = 1u64 << UBLK_QID_BITS;

pub const UBLKSRV_IO_BUF_TOTAL_BITS: u32 = UBLK_QID_OFF + UBLK_QID_BITS;
pub const UBLKSRV_IO_BUF_TOTAL_SIZE: u64 = 1u64 << UBLKSRV_IO_BUF_TOTAL_BITS;

pub const UBLK_F_SUPPORT_ZERO_COPY: u64 = 1u64 << 0;
pub const UBLK_F_URING_CMD_COMP_IN_TASK: u64 = 1u64 << 1;
pub const UBLK_F_NEED_GET_DATA: u64 = 1u64 << 2;
pub const UBLK_F_USER_RECOVERY: u64 = 1u64 << 3;
pub const UBLK_F_USER_RECOVERY_REISSUE: u64 = 1u64 << 4;
pub const UBLK_F_UNPRIVILEGED_DEV: u64 = 1u64 << 5;
pub const UBLK_F_CMD_IOCTL_ENCODE: u64 = 1u64 << 6;
pub const UBLK_F_USER_COPY: u64 = 1u64 << 7;
pub const UBLK_F_ZONED: u64 = 1u64 << 8;

pub const UBLK_S_DEV_DEAD: u16 = 0;
pub const UBLK_S_DEV_LIVE: u16 = 1;
pub const UBLK_S_DEV_QUIESCED: u16 = 2;

pub const UBLK_IO_OP_READ: u8 = 0;
pub const UBLK_IO_OP_WRITE: u8 = 1;
pub const UBLK_IO_OP_FLUSH: u8 = 2;
pub const UBLK_IO_OP_DISCARD: u8 = 3;
pub const UBLK_IO_OP_WRITE_SAME: u8 = 4;
pub const UBLK_IO_OP_WRITE_ZEROES: u8 = 5;
pub const UBLK_IO_OP_ZONE_OPEN: u8 = 10;
pub const UBLK_IO_OP_ZONE_CLOSE: u8 = 11;
pub const UBLK_IO_OP_ZONE_FINISH: u8 = 12;
pub const UBLK_IO_OP_ZONE_APPEND: u8 = 13;
pub const UBLK_IO_OP_ZONE_RESET_ALL: u8 = 14;
pub const UBLK_IO_OP_ZONE_RESET: u8 = 15;
pub const UBLK_IO_OP_REPORT_ZONES: u8 = 18;

pub const UBLK_IO_F_FAILFAST_DEV: u32 = 1u32 << 8;
pub const UBLK_IO_F_FAILFAST_TRANSPORT: u32 = 1u32 << 9;
pub const UBLK_IO_F_FAILFAST_DRIVER: u32 = 1u32 << 10;
pub const UBLK_IO_F_META: u32 = 1u32 << 11;
pub const UBLK_IO_F_FUA: u32 = 1u32 << 13;
pub const UBLK_IO_F_NOUNMAP: u32 = 1u32 << 15;
pub const UBLK_IO_F_SWAP: u32 = 1u32 << 16;

pub const UBLK_ATTR_READ_ONLY: u32 = 1u32 << 0;
pub const UBLK_ATTR_ROTATIONAL: u32 = 1u32 << 1;
pub const UBLK_ATTR_VOLATILE_CACHE: u32 = 1u32 << 2;
pub const UBLK_ATTR_FUA: u32 = 1u32 << 3;

pub const UBLK_PARAM_TYPE_BASIC: u32 = 1u32 << 0;
pub const UBLK_PARAM_TYPE_DISCARD: u32 = 1u32 << 1;
pub const UBLK_PARAM_TYPE_DEVT: u32 = 1u32 << 2;
pub const UBLK_PARAM_TYPE_ZONED: u32 = 1u32 << 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CtrlCmd {
    pub dev_id: u32,
    pub queue_id: u16,
    pub len: u16,
    pub addr: u64,
    pub data: [u64; 1],
    pub dev_path_len: u16,
    pub pad: u16,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CtrlDevInfo {
    pub nr_hw_queues: u16,
    pub queue_depth: u16,
    pub state: u16,
    pub pad0: u16,
    pub max_io_buf_bytes: u32,
    pub dev_id: u32,
    pub ublksrv_pid: i32,
    pub pad1: u32,
    pub flags: u64,
    pub ublksrv_flags: u64,
    pub owner_uid: u32,
    pub owner_gid: u32,
    pub reserved1: u64,
    pub reserved2: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IoDesc {
    pub op_flags: u32,
    pub nr_sectors_or_zones: u32,
    pub start_sector: u64,
    pub addr: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoRequestError {
    UnsupportedOp(u8),
    LengthOverflow,
    OffsetOverflow,
}

impl IoRequestError {
    pub fn ublk_result(self) -> i32 {
        match self {
            IoRequestError::UnsupportedOp(_)
            | IoRequestError::LengthOverflow
            | IoRequestError::OffsetOverflow => UBLK_IO_RES_EINVAL,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct IoWork {
    pub qid: u16,
    pub tag: u16,
    pub buffer_addr: u64,
    pub req: Request,
    pub payload: Vec<u8>,
}

impl IoWork {
    pub fn from_desc(
        qid: u16,
        tag: u16,
        desc: IoDesc,
        payload: Vec<u8>,
    ) -> Result<Self, IoRequestError> {
        Ok(Self {
            qid,
            tag,
            buffer_addr: desc.addr,
            req: desc.to_block_request(tag)?,
            payload,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoCompletion {
    pub qid: u16,
    pub tag: u16,
    pub result: i32,
}

impl IoCompletion {
    pub fn ok(qid: u16, tag: u16) -> Self {
        Self {
            qid,
            tag,
            result: UBLK_IO_RES_OK,
        }
    }

    pub fn from_request_error(qid: u16, tag: u16, err: IoRequestError) -> Self {
        Self {
            qid,
            tag,
            result: err.ublk_result(),
        }
    }

    pub fn to_io_cmd(self) -> IoCmd {
        IoCmd {
            q_id: self.qid,
            tag: self.tag,
            result: self.result,
            addr_or_zone_append_lba: 0,
        }
    }
}

impl IoDesc {
    pub fn operation(&self) -> u8 {
        (self.op_flags & 0xff) as u8
    }

    pub fn flags(&self) -> u32 {
        self.op_flags >> 8
    }

    pub fn to_block_request(&self, tag: u16) -> Result<Request, IoRequestError> {
        let op = self.operation();
        let (cmd, offset, len) = match op {
            UBLK_IO_OP_READ => {
                let (offset, len) = self.byte_range()?;
                (Command::Read, offset, len)
            }
            UBLK_IO_OP_WRITE => {
                let (offset, len) = self.byte_range()?;
                (Command::Write, offset, len)
            }
            UBLK_IO_OP_DISCARD => {
                let (offset, len) = self.byte_range()?;
                (Command::Trim, offset, len)
            }
            UBLK_IO_OP_FLUSH => (Command::Flush, 0, 0),
            other => return Err(IoRequestError::UnsupportedOp(other)),
        };

        Ok(Request {
            flags: 0,
            cmd,
            handle: u64::from(tag),
            offset,
            len,
        })
    }

    fn byte_range(&self) -> Result<(u64, u32), IoRequestError> {
        let offset = self
            .start_sector
            .checked_mul(UBLK_SECTOR_SIZE)
            .ok_or(IoRequestError::OffsetOverflow)?;
        let len_bytes = u64::from(self.nr_sectors_or_zones)
            .checked_mul(UBLK_SECTOR_SIZE)
            .ok_or(IoRequestError::LengthOverflow)?;
        let len = u32::try_from(len_bytes).map_err(|_| IoRequestError::LengthOverflow)?;

        Ok((offset, len))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IoCmd {
    pub q_id: u16,
    pub tag: u16,
    pub result: i32,
    pub addr_or_zone_append_lba: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParamBasic {
    pub attrs: u32,
    pub logical_bs_shift: u8,
    pub physical_bs_shift: u8,
    pub io_opt_shift: u8,
    pub io_min_shift: u8,
    pub max_sectors: u32,
    pub chunk_sectors: u32,
    pub dev_sectors: u64,
    pub virt_boundary_mask: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParamDiscard {
    pub discard_alignment: u32,
    pub discard_granularity: u32,
    pub max_discard_sectors: u32,
    pub max_write_zeroes_sectors: u32,
    pub max_discard_segments: u16,
    pub reserved0: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParamDevt {
    pub char_major: u32,
    pub char_minor: u32,
    pub disk_major: u32,
    pub disk_minor: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParamZoned {
    pub max_open_zones: u32,
    pub max_active_zones: u32,
    pub max_zone_append_sectors: u32,
    pub reserved: [u8; 20],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Params {
    pub len: u32,
    pub types: u32,
    pub basic: ParamBasic,
    pub discard: ParamDiscard,
    pub devt: ParamDevt,
    pub zoned: ParamZoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoBufferPosition {
    pub qid: u64,
    pub tag: u64,
    pub buffer_offset: u64,
}

pub fn io_buffer_position(qid: u64, tag: u64, buffer_offset: u64) -> Option<u64> {
    if qid >= UBLK_MAX_NR_QUEUES
        || tag > UBLK_TAG_BITS_MASK
        || buffer_offset > UBLK_IO_BUF_BITS_MASK
    {
        return None;
    }

    let raw = (qid << UBLK_QID_OFF) | (tag << UBLK_TAG_OFF) | buffer_offset;
    UBLKSRV_IO_BUF_OFFSET.checked_add(raw)
}

pub fn decode_io_buffer_position(pos: u64) -> Option<IoBufferPosition> {
    let raw = pos.checked_sub(UBLKSRV_IO_BUF_OFFSET)?;
    if raw >= UBLKSRV_IO_BUF_TOTAL_SIZE {
        return None;
    }

    Some(IoBufferPosition {
        qid: (raw >> UBLK_QID_OFF) & UBLK_QID_BITS_MASK,
        tag: (raw >> UBLK_TAG_OFF) & UBLK_TAG_BITS_MASK,
        buffer_offset: raw & UBLK_IO_BUF_BITS_MASK,
    })
}
