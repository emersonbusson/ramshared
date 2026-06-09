//! UAPI mínima do ublk usada pela Fase B.
//!
//! Fonte primária: `include/uapi/linux/ublk_cmd.h` do kernel WSL2 custom
//! `6.6.123.2-microsoft-standard-WSL2+`. Este módulo só espelha constantes,
//! layouts e helpers puros; abertura de `/dev/ublk-control` e `io_uring` ficam
//! para recortes posteriores.

use ramshared_block::{Command, Request};

pub const UBLK_SECTOR_SIZE: u64 = 512;

/// Tamanho de `struct ublksrv_io_desc` (4+4+8+8). Espelha `size_of::<IoDesc>()`.
pub const UBLK_IO_DESC_SIZE: usize = 24;

/// Tamanho de `struct ublk_params` (verificado via cc). Espelha `size_of::<Params>()`.
pub const UBLK_PARAMS_LEN: usize = 112;

pub const UBLK_CMD_ADD_DEV: u32 = 0x04;
pub const UBLK_CMD_DEL_DEV: u32 = 0x05;
pub const UBLK_CMD_START_DEV: u32 = 0x06;
pub const UBLK_CMD_STOP_DEV: u32 = 0x07;
pub const UBLK_CMD_SET_PARAMS: u32 = 0x08;
pub const UBLK_CMD_GET_PARAMS: u32 = 0x09;
pub const UBLK_CMD_GET_DEV_INFO2: u32 = 0x12;
pub const UBLK_U_CMD_ADD_DEV: u32 = 0xc020_7504;
pub const UBLK_U_CMD_DEL_DEV: u32 = 0xc020_7505;
pub const UBLK_U_CMD_START_DEV: u32 = 0xc020_7506;
pub const UBLK_U_CMD_STOP_DEV: u32 = 0xc020_7507;
pub const UBLK_U_CMD_SET_PARAMS: u32 = 0xc020_7508;
pub const UBLK_U_CMD_GET_PARAMS: u32 = 0x8020_7509;
pub const UBLK_U_CMD_GET_FEATURES: u32 = 0x8020_7513;

pub const UBLK_IO_FETCH_REQ: u32 = 0x20;
pub const UBLK_IO_COMMIT_AND_FETCH_REQ: u32 = 0x21;
pub const UBLK_IO_NEED_GET_DATA: u32 = 0x22;

// Ops de IO codificadas (`_IOWR('u', nr, struct ublksrv_io_cmd)`), exigidas quando
// o device e criado com `UBLK_F_CMD_IOCTL_ENCODE`. Valores verificados via `cc`
// contra `include/uapi/linux/ublk_cmd.h` do kernel custom 6.6.123.2.
pub const UBLK_U_IO_FETCH_REQ: u32 = 0xc010_7520;
pub const UBLK_U_IO_COMMIT_AND_FETCH_REQ: u32 = 0xc010_7521;
pub const UBLK_U_IO_NEED_GET_DATA: u32 = 0xc010_7522;

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
    /// Decodifica um `ublksrv_io_desc` a partir dos bytes mapeados do char device
    /// (layout `repr(C)` nativo). Retorna `None` se o buffer tiver menos de 24 bytes.
    pub fn from_ne_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < UBLK_IO_DESC_SIZE {
            return None;
        }
        Some(Self {
            op_flags: u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            nr_sectors_or_zones: u32::from_ne_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            start_sector: u64::from_ne_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
            addr: u64::from_ne_bytes([
                bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22],
                bytes[23],
            ]),
        })
    }

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

impl IoCmd {
    /// Monta o `ublksrv_io_cmd` de `FETCH_REQ`: aponta o `addr` para o buffer da
    /// tag e deixa `result` zerado (ignorado pelo driver no fetch inicial).
    pub fn fetch(q_id: u16, tag: u16, buffer_addr: u64) -> Self {
        Self {
            q_id,
            tag,
            result: 0,
            addr_or_zone_append_lba: buffer_addr,
        }
    }

    /// Serializa o comando no layout `repr(C)` de 16 bytes esperado pelo driver,
    /// para copia direta no campo `cmd` da SQE de `UringCmd80`.
    pub fn to_bytes(self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..2].copy_from_slice(&self.q_id.to_ne_bytes());
        bytes[2..4].copy_from_slice(&self.tag.to_ne_bytes());
        bytes[4..8].copy_from_slice(&self.result.to_ne_bytes());
        bytes[8..16].copy_from_slice(&self.addr_or_zone_append_lba.to_ne_bytes());
        bytes
    }
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

impl Params {
    /// Monta `ublk_params` só com o tipo BASIC: tamanho do disco em setores de 512 B
    /// e os shifts de block size lógico/físico. Demais tipos ficam zerados.
    pub fn basic_disk(dev_sectors: u64, logical_bs_shift: u8, physical_bs_shift: u8) -> Self {
        Self {
            len: UBLK_PARAMS_LEN as u32,
            types: UBLK_PARAM_TYPE_BASIC,
            basic: ParamBasic {
                logical_bs_shift,
                physical_bs_shift,
                io_opt_shift: physical_bs_shift,
                io_min_shift: logical_bs_shift,
                dev_sectors,
                ..ParamBasic::default()
            },
            ..Self::default()
        }
    }

    /// Serializa no layout `repr(C)` de 112 B de `struct ublk_params` (offsets
    /// verificados via `cc`). Sem `unsafe`.
    pub fn to_bytes(&self) -> [u8; UBLK_PARAMS_LEN] {
        let mut b = [0u8; UBLK_PARAMS_LEN];
        b[0..4].copy_from_slice(&self.len.to_ne_bytes());
        b[4..8].copy_from_slice(&self.types.to_ne_bytes());
        // basic @ 8
        b[8..12].copy_from_slice(&self.basic.attrs.to_ne_bytes());
        b[12] = self.basic.logical_bs_shift;
        b[13] = self.basic.physical_bs_shift;
        b[14] = self.basic.io_opt_shift;
        b[15] = self.basic.io_min_shift;
        b[16..20].copy_from_slice(&self.basic.max_sectors.to_ne_bytes());
        b[20..24].copy_from_slice(&self.basic.chunk_sectors.to_ne_bytes());
        b[24..32].copy_from_slice(&self.basic.dev_sectors.to_ne_bytes());
        b[32..40].copy_from_slice(&self.basic.virt_boundary_mask.to_ne_bytes());
        // discard @ 40
        b[40..44].copy_from_slice(&self.discard.discard_alignment.to_ne_bytes());
        b[44..48].copy_from_slice(&self.discard.discard_granularity.to_ne_bytes());
        b[48..52].copy_from_slice(&self.discard.max_discard_sectors.to_ne_bytes());
        b[52..56].copy_from_slice(&self.discard.max_write_zeroes_sectors.to_ne_bytes());
        b[56..58].copy_from_slice(&self.discard.max_discard_segments.to_ne_bytes());
        b[58..60].copy_from_slice(&self.discard.reserved0.to_ne_bytes());
        // devt @ 60
        b[60..64].copy_from_slice(&self.devt.char_major.to_ne_bytes());
        b[64..68].copy_from_slice(&self.devt.char_minor.to_ne_bytes());
        b[68..72].copy_from_slice(&self.devt.disk_major.to_ne_bytes());
        b[72..76].copy_from_slice(&self.devt.disk_minor.to_ne_bytes());
        // zoned @ 76
        b[76..80].copy_from_slice(&self.zoned.max_open_zones.to_ne_bytes());
        b[80..84].copy_from_slice(&self.zoned.max_active_zones.to_ne_bytes());
        b[84..88].copy_from_slice(&self.zoned.max_zone_append_sectors.to_ne_bytes());
        b[88..108].copy_from_slice(&self.zoned.reserved);
        // 108..112: padding de alinhamento (zerado).
        b
    }

    /// Decodifica `struct ublk_params` (inverso de [`Params::to_bytes`]).
    pub fn from_bytes(b: &[u8; UBLK_PARAMS_LEN]) -> Self {
        let mut reserved = [0u8; 20];
        reserved.copy_from_slice(&b[88..108]);
        Self {
            len: u32::from_ne_bytes([b[0], b[1], b[2], b[3]]),
            types: u32::from_ne_bytes([b[4], b[5], b[6], b[7]]),
            basic: ParamBasic {
                attrs: u32::from_ne_bytes([b[8], b[9], b[10], b[11]]),
                logical_bs_shift: b[12],
                physical_bs_shift: b[13],
                io_opt_shift: b[14],
                io_min_shift: b[15],
                max_sectors: u32::from_ne_bytes([b[16], b[17], b[18], b[19]]),
                chunk_sectors: u32::from_ne_bytes([b[20], b[21], b[22], b[23]]),
                dev_sectors: u64::from_ne_bytes([
                    b[24], b[25], b[26], b[27], b[28], b[29], b[30], b[31],
                ]),
                virt_boundary_mask: u64::from_ne_bytes([
                    b[32], b[33], b[34], b[35], b[36], b[37], b[38], b[39],
                ]),
            },
            discard: ParamDiscard {
                discard_alignment: u32::from_ne_bytes([b[40], b[41], b[42], b[43]]),
                discard_granularity: u32::from_ne_bytes([b[44], b[45], b[46], b[47]]),
                max_discard_sectors: u32::from_ne_bytes([b[48], b[49], b[50], b[51]]),
                max_write_zeroes_sectors: u32::from_ne_bytes([b[52], b[53], b[54], b[55]]),
                max_discard_segments: u16::from_ne_bytes([b[56], b[57]]),
                reserved0: u16::from_ne_bytes([b[58], b[59]]),
            },
            devt: ParamDevt {
                char_major: u32::from_ne_bytes([b[60], b[61], b[62], b[63]]),
                char_minor: u32::from_ne_bytes([b[64], b[65], b[66], b[67]]),
                disk_major: u32::from_ne_bytes([b[68], b[69], b[70], b[71]]),
                disk_minor: u32::from_ne_bytes([b[72], b[73], b[74], b[75]]),
            },
            zoned: ParamZoned {
                max_open_zones: u32::from_ne_bytes([b[76], b[77], b[78], b[79]]),
                max_active_zones: u32::from_ne_bytes([b[80], b[81], b[82], b[83]]),
                max_zone_append_sectors: u32::from_ne_bytes([b[84], b[85], b[86], b[87]]),
                reserved,
            },
        }
    }
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
