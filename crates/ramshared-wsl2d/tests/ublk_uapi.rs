use std::mem::{align_of, size_of};

use ramshared_wsl2d::ublk;

#[test]
fn ublk_uapi_constants_match_kernel_header() {
    assert_eq!(ublk::UBLK_CMD_ADD_DEV, 0x04);
    assert_eq!(ublk::UBLK_CMD_DEL_DEV, 0x05);
    assert_eq!(ublk::UBLK_CMD_START_DEV, 0x06);
    assert_eq!(ublk::UBLK_CMD_STOP_DEV, 0x07);
    assert_eq!(ublk::UBLK_CMD_SET_PARAMS, 0x08);
    assert_eq!(ublk::UBLK_CMD_GET_PARAMS, 0x09);
    assert_eq!(ublk::UBLK_CMD_GET_DEV_INFO2, 0x12);

    assert_eq!(ublk::UBLK_IO_FETCH_REQ, 0x20);
    assert_eq!(ublk::UBLK_IO_COMMIT_AND_FETCH_REQ, 0x21);
    assert_eq!(ublk::UBLK_IO_NEED_GET_DATA, 0x22);
    assert_eq!(ublk::UBLK_IO_RES_OK, 0);
    assert_eq!(ublk::UBLK_IO_RES_NEED_GET_DATA, 1);

    assert_eq!(ublk::UBLKSRV_IO_BUF_OFFSET, 0x8000_0000);
    assert_eq!(ublk::UBLK_MAX_QUEUE_DEPTH, 4096);
    assert_eq!(ublk::UBLK_IO_BUF_BITS, 25);
    assert_eq!(ublk::UBLK_TAG_BITS, 16);
    assert_eq!(ublk::UBLK_QID_BITS, 12);
    assert_eq!(ublk::UBLK_TAG_OFF, 25);
    assert_eq!(ublk::UBLK_QID_OFF, 41);
    assert_eq!(ublk::UBLK_MAX_NR_QUEUES, 4096);
}

#[test]
fn io_desc_extracts_operation_and_flags_like_kernel_inline_helpers() {
    let desc = ublk::IoDesc {
        op_flags: ublk::UBLK_IO_OP_WRITE as u32
            | ublk::UBLK_IO_F_FUA
            | ublk::UBLK_IO_F_SWAP,
        nr_sectors_or_zones: 8,
        start_sector: 16,
        addr: 0x1000,
    };

    assert_eq!(desc.operation(), ublk::UBLK_IO_OP_WRITE);
    assert_eq!(
        desc.flags(),
        (ublk::UBLK_IO_F_FUA | ublk::UBLK_IO_F_SWAP) >> 8
    );
}

#[test]
fn io_buffer_position_roundtrips_with_driver_bit_layout() {
    let pos = ublk::io_buffer_position(7, 33, 4096).expect("valid position");

    assert_eq!(
        pos,
        ublk::UBLKSRV_IO_BUF_OFFSET + (7 << ublk::UBLK_QID_OFF) + (33 << ublk::UBLK_TAG_OFF) + 4096
    );

    let decoded = ublk::decode_io_buffer_position(pos).expect("decodable position");
    assert_eq!(decoded.qid, 7);
    assert_eq!(decoded.tag, 33);
    assert_eq!(decoded.buffer_offset, 4096);
}

#[test]
fn io_buffer_position_rejects_out_of_range_parts() {
    assert!(ublk::io_buffer_position(ublk::UBLK_MAX_NR_QUEUES, 0, 0).is_none());
    assert!(ublk::io_buffer_position(0, ublk::UBLK_TAG_BITS_MASK + 1, 0).is_none());
    assert!(ublk::io_buffer_position(0, 0, ublk::UBLK_IO_BUF_BITS_MASK + 1).is_none());
    assert!(ublk::decode_io_buffer_position(ublk::UBLKSRV_IO_BUF_OFFSET - 1).is_none());
}

#[test]
fn repr_c_layouts_match_local_kernel_header() {
    assert_eq!(size_of::<ublk::CtrlCmd>(), 32);
    assert_eq!(align_of::<ublk::CtrlCmd>(), 8);

    assert_eq!(size_of::<ublk::CtrlDevInfo>(), 64);
    assert_eq!(align_of::<ublk::CtrlDevInfo>(), 8);

    assert_eq!(size_of::<ublk::IoDesc>(), 24);
    assert_eq!(align_of::<ublk::IoDesc>(), 8);

    assert_eq!(size_of::<ublk::IoCmd>(), 16);
    assert_eq!(align_of::<ublk::IoCmd>(), 8);

    assert_eq!(size_of::<ublk::Params>(), 112);
    assert_eq!(align_of::<ublk::Params>(), 8);
}
