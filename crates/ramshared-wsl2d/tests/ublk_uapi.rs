#![allow(clippy::unwrap_used, clippy::expect_used)] // teste: unwrap/expect é idiomático

use std::mem::{align_of, size_of};

use ramshared_block::Command;
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
    assert_eq!(ublk::UBLK_U_CMD_ADD_DEV, 0xc020_7504);
    assert_eq!(ublk::UBLK_U_CMD_DEL_DEV, 0xc020_7505);
    assert_eq!(ublk::UBLK_U_CMD_GET_FEATURES, 0x8020_7513);

    assert_eq!(ublk::UBLK_IO_FETCH_REQ, 0x20);
    assert_eq!(ublk::UBLK_IO_COMMIT_AND_FETCH_REQ, 0x21);
    assert_eq!(ublk::UBLK_IO_NEED_GET_DATA, 0x22);
    assert_eq!(ublk::UBLK_U_IO_FETCH_REQ, 0xc010_7520);
    assert_eq!(ublk::UBLK_U_IO_COMMIT_AND_FETCH_REQ, 0xc010_7521);
    assert_eq!(ublk::UBLK_U_IO_NEED_GET_DATA, 0xc010_7522);
    assert_eq!(ublk::UBLK_U_CMD_START_DEV, 0xc020_7506);
    assert_eq!(ublk::UBLK_U_CMD_STOP_DEV, 0xc020_7507);
    assert_eq!(ublk::UBLK_U_CMD_SET_PARAMS, 0xc020_7508);
    assert_eq!(ublk::UBLK_U_CMD_GET_PARAMS, 0x8020_7509);
    assert_eq!(ublk::UBLK_IO_RES_OK, 0);
    assert_eq!(ublk::UBLK_IO_RES_NEED_GET_DATA, 1);

    assert_eq!(ublk::UBLKSRV_IO_BUF_OFFSET, 0x8000_0000);
    assert_eq!(ublk::UBLK_FEATURES_LEN, 8);
    assert_eq!(ublk::UBLK_QUEUE_ID_NONE, u16::MAX);
    assert_eq!(ublk::UBLK_DEV_ID_AUTO, u32::MAX);
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
        op_flags: ublk::UBLK_IO_OP_WRITE as u32 | ublk::UBLK_IO_F_FUA | ublk::UBLK_IO_F_SWAP,
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

#[test]
fn io_desc_read_maps_to_block_request_with_512b_sector_units() {
    let desc = ublk::IoDesc {
        op_flags: ublk::UBLK_IO_OP_READ as u32,
        nr_sectors_or_zones: 8,
        start_sector: 16,
        addr: 0x1000,
    };

    let req = desc.to_block_request(44).expect("READ deve mapear");

    assert_eq!(req.cmd, Command::Read);
    assert_eq!(req.handle, 44);
    assert_eq!(req.offset, 8192);
    assert_eq!(req.len, 4096);
    assert_eq!(req.flags, 0);
}

#[test]
fn io_desc_discard_maps_to_trim_and_flush_ignores_sector_range() {
    let discard = ublk::IoDesc {
        op_flags: ublk::UBLK_IO_OP_DISCARD as u32,
        nr_sectors_or_zones: 16,
        start_sector: 32,
        addr: 0,
    }
    .to_block_request(7)
    .expect("DISCARD must map to TRIM");

    assert_eq!(discard.cmd, Command::Trim);
    assert_eq!(discard.offset, 16_384);
    assert_eq!(discard.len, 8192);

    let flush = ublk::IoDesc {
        op_flags: ublk::UBLK_IO_OP_FLUSH as u32,
        nr_sectors_or_zones: 16,
        start_sector: 32,
        addr: 0,
    }
    .to_block_request(8)
    .expect("FLUSH deve mapear");

    assert_eq!(flush.cmd, Command::Flush);
    assert_eq!(flush.offset, 0);
    assert_eq!(flush.len, 0);
}

#[test]
fn io_desc_rejects_unsupported_ops_and_byte_length_overflow() {
    let unsupported = ublk::IoDesc {
        op_flags: ublk::UBLK_IO_OP_WRITE_ZEROES as u32,
        nr_sectors_or_zones: 8,
        start_sector: 0,
        addr: 0,
    };
    assert_eq!(
        unsupported.to_block_request(1),
        Err(ublk::IoRequestError::UnsupportedOp(
            ublk::UBLK_IO_OP_WRITE_ZEROES
        ))
    );

    let overflow = ublk::IoDesc {
        op_flags: ublk::UBLK_IO_OP_WRITE as u32,
        nr_sectors_or_zones: 8_388_608,
        start_sector: 0,
        addr: 0,
    };
    assert_eq!(
        overflow.to_block_request(1),
        Err(ublk::IoRequestError::LengthOverflow)
    );
}

#[test]
fn io_work_carries_worker_request_and_ublk_identity() {
    let desc = ublk::IoDesc {
        op_flags: ublk::UBLK_IO_OP_WRITE as u32,
        nr_sectors_or_zones: 8,
        start_sector: 4,
        addr: 0x5000,
    };
    let payload = vec![0xA5; 4096];

    let work = ublk::IoWork::from_desc(3, 9, desc, payload.clone()).expect("work");

    assert_eq!(work.qid, 3);
    assert_eq!(work.tag, 9);
    assert_eq!(work.buffer_addr, 0x5000);
    assert_eq!(work.req.cmd, Command::Write);
    assert_eq!(work.req.handle, 9);
    assert_eq!(work.req.offset, 2048);
    assert_eq!(work.req.len, 4096);
    assert_eq!(work.payload, payload);
}

#[test]
fn io_completion_encodes_ok_commit_command() {
    let cmd = ublk::IoCompletion::ok(2, 11).to_io_cmd();

    assert_eq!(cmd.q_id, 2);
    assert_eq!(cmd.tag, 11);
    assert_eq!(cmd.result, ublk::UBLK_IO_RES_OK);
    assert_eq!(cmd.addr_or_zone_append_lba, 0);
}

#[test]
fn io_completion_maps_translation_error_to_negative_errno() {
    let err = ublk::IoWork::from_desc(
        2,
        11,
        ublk::IoDesc {
            op_flags: ublk::UBLK_IO_OP_WRITE_ZEROES as u32,
            nr_sectors_or_zones: 8,
            start_sector: 0,
            addr: 0,
        },
        Vec::new(),
    )
    .unwrap_err();

    let cmd = ublk::IoCompletion::from_request_error(2, 11, err).to_io_cmd();

    assert_eq!(cmd.q_id, 2);
    assert_eq!(cmd.tag, 11);
    assert_eq!(cmd.result, ublk::UBLK_IO_RES_EINVAL);
}

#[test]
fn io_cmd_fetch_builds_request_with_buffer_and_zero_result() {
    let fetch = ublk::IoCmd::fetch(2, 7, 0xdead_beef);

    assert_eq!(fetch.q_id, 2);
    assert_eq!(fetch.tag, 7);
    assert_eq!(fetch.result, 0);
    assert_eq!(fetch.addr_or_zone_append_lba, 0xdead_beef);
}

#[test]
fn io_cmd_serializes_to_16_byte_kernel_layout() {
    let bytes = ublk::IoCmd::fetch(2, 7, 0xdead_beef).to_bytes();

    assert_eq!(u16::from_ne_bytes([bytes[0], bytes[1]]), 2);
    assert_eq!(u16::from_ne_bytes([bytes[2], bytes[3]]), 7);
    assert_eq!(
        i32::from_ne_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        0
    );
    assert_eq!(
        u64::from_ne_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]),
        0xdead_beef
    );

    // Negative result (e.g. EINVAL) must serialize as two's complement i32.
    let abort = ublk::IoCompletion::from_request_error(1, 2, ublk::IoRequestError::LengthOverflow)
        .to_io_cmd()
        .to_bytes();
    assert_eq!(
        i32::from_ne_bytes([abort[4], abort[5], abort[6], abort[7]]),
        ublk::UBLK_IO_RES_EINVAL
    );
}

#[test]
fn io_desc_decodes_from_kernel_byte_layout() {
    assert_eq!(ublk::UBLK_IO_DESC_SIZE, size_of::<ublk::IoDesc>());

    let mut bytes = [0u8; ublk::UBLK_IO_DESC_SIZE];
    bytes[0..4].copy_from_slice(&(ublk::UBLK_IO_OP_WRITE as u32).to_ne_bytes());
    bytes[4..8].copy_from_slice(&8u32.to_ne_bytes());
    bytes[8..16].copy_from_slice(&16u64.to_ne_bytes());
    bytes[16..24].copy_from_slice(&0x1000u64.to_ne_bytes());

    let desc = ublk::IoDesc::from_ne_bytes(&bytes).expect("24 bytes decodificam");
    assert_eq!(desc.operation(), ublk::UBLK_IO_OP_WRITE);
    assert_eq!(desc.nr_sectors_or_zones, 8);
    assert_eq!(desc.start_sector, 16);
    assert_eq!(desc.addr, 0x1000);

    // Buffer smaller than an io-desc fails to decode.
    assert!(ublk::IoDesc::from_ne_bytes(&bytes[..23]).is_none());
}

#[test]
fn params_round_trips_through_kernel_byte_layout() {
    let params = ublk::Params::basic_disk(2048, 9, 12);
    assert_eq!(params.len, 112);
    assert_eq!(params.types, ublk::UBLK_PARAM_TYPE_BASIC);
    assert_eq!(params.basic.dev_sectors, 2048);

    let bytes = params.to_bytes();
    assert_eq!(ublk::Params::from_bytes(&bytes), params);

    // dev_sectors em offset absoluto 24 (basic@8 + dev_sectors@16); verificado via cc.
    assert_eq!(
        u64::from_ne_bytes([
            bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30], bytes[31],
        ]),
        2048
    );
}
