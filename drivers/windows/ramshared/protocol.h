/* SPDX-License-Identifier: MIT */
/*
 * RamShared Windows driver <-> service ABI (frozen for IMPL ITEM-4).
 * SPEC: docs/specs/no-milestone/windows-swap-driver/SPEC.md (DT-17, DT-18, DT-22).
 *
 * Layout is the single source of truth. The Rust mirror lives in
 * crates/ramshared-winsvc/src/proto.rs and must match byte-for-byte
 * (size asserts + golden-bytes tests).
 *
 * Do not log kernel virtual addresses from producers of this ABI (KASLR).
 */
#pragma once

/* Fixed-width types: kernel uses WDK basetsd; userspace uses stdint. */
#if defined(_KERNEL_MODE) || defined(_NTDDK_) || defined(_STORPORT_)
typedef unsigned __int64 ramshared_u64;
typedef unsigned __int32 ramshared_u32;
typedef signed __int32 ramshared_i32;
typedef unsigned char ramshared_u8;
#else
#include <stdint.h>
typedef uint64_t ramshared_u64;
typedef uint32_t ramshared_u32;
typedef int32_t ramshared_i32;
typedef uint8_t ramshared_u8;
#endif

#ifdef _MSC_VER
#pragma pack(push, 8)
#endif

#define RAMSHARED_ABI_VERSION 1u
#define RAMSHARED_MAX_QD 256u	/* queue_depth max; power of two */
#define RAMSHARED_MAX_IO (1u << 20) /* 1 MiB per bounce slot */
#define RAMSHARED_RING_MAGIC 0x52535244u /* 'RSRD' */

enum ramshared_op {
	RAMSHARED_OP_READ = 0,
	RAMSHARED_OP_WRITE = 1,
	RAMSHARED_OP_FLUSH = 2
};

/* status: 0 = OK; else errno-like aligned with ramshared-block */
#define RAMSHARED_ST_OK 0
#define RAMSHARED_ST_EIO 5
#define RAMSHARED_ST_EINVAL 22

/* driver -> service, 32 bytes */
typedef struct _RAMSHARED_SQE {
	ramshared_u64 tag;
	ramshared_u32 op;
	ramshared_u32 flags;
	ramshared_u64 offset;
	ramshared_u32 len;
	ramshared_u32 buf_slot;
} RAMSHARED_SQE, *PRAMSHARED_SQE;

/* service -> driver, 16 bytes */
typedef struct _RAMSHARED_CQE {
	ramshared_u64 tag;
	ramshared_i32 status;
	ramshared_u32 reserved;
} RAMSHARED_CQE, *PRAMSHARED_CQE;

/* precedes entries[]; SPSC */
typedef struct _RAMSHARED_RING_HDR {
	ramshared_u32 magic;
	ramshared_u32 entries; /* = queue_depth (power of two) */
	volatile ramshared_u32 head;
	volatile ramshared_u32 tail;
} RAMSHARED_RING_HDR, *PRAMSHARED_RING_HDR;

/* payload of IOCTL_RAMSHARED_REGISTER_QUEUE */
typedef struct _RAMSHARED_REGISTER {
	ramshared_u32 abi_version;
	ramshared_u32 disk_id;
	ramshared_u32 queue_depth;
	ramshared_u32 block_size;
	ramshared_u32 max_io_bytes;
	ramshared_u32 reserved;
	ramshared_u64 sq_ring_va;
	ramshared_u64 cq_ring_va;
	ramshared_u64 data_area_va;
	ramshared_u64 data_area_len;
	/* auxiliary (DT-22); primary wake path = COMMIT_AND_FETCH IRP */
	ramshared_u64 sq_event_handle;
	ramshared_u64 cq_event_handle;
} RAMSHARED_REGISTER, *PRAMSHARED_REGISTER;

/* IOCTL_RAMSHARED_CREATE_DISK */
typedef struct _RAMSHARED_DISK_PARAMS {
	ramshared_u64 size_bytes; /* multiple of block_size */
	ramshared_u32 block_size; /* 512 or 4096 */
	ramshared_u32 reserved;
	ramshared_u8 serial[16]; /* INQUIRY VPD / stable id */
} RAMSHARED_DISK_PARAMS, *PRAMSHARED_DISK_PARAMS;

/*
 * IOCTL function codes (N). Full CTL_CODE expansion is MSVC/WDK-only:
 *   CTL_CODE(FILE_DEVICE_MASS_STORAGE, 0x800|N, METHOD_BUFFERED,
 *            FILE_READ_ACCESS|FILE_WRITE_ACCESS)
 * N=0 REGISTER_QUEUE, 1 UNREGISTER_QUEUE, 2 COMMIT_AND_FETCH,
 *   3 CREATE_DISK, 4 DESTROY_DISK.
 */
#define RAMSHARED_IOCTL_FN_REGISTER_QUEUE 0u
#define RAMSHARED_IOCTL_FN_UNREGISTER_QUEUE 1u
#define RAMSHARED_IOCTL_FN_COMMIT_AND_FETCH 2u
#define RAMSHARED_IOCTL_FN_CREATE_DISK 3u
#define RAMSHARED_IOCTL_FN_DESTROY_DISK 4u

#ifdef __cplusplus
static_assert(sizeof(RAMSHARED_SQE) == 32, "RAMSHARED_SQE size");
static_assert(sizeof(RAMSHARED_CQE) == 16, "RAMSHARED_CQE size");
static_assert(sizeof(RAMSHARED_RING_HDR) == 16, "RAMSHARED_RING_HDR size");
static_assert(sizeof(RAMSHARED_REGISTER) == 72, "RAMSHARED_REGISTER size");
static_assert(sizeof(RAMSHARED_DISK_PARAMS) == 32, "RAMSHARED_DISK_PARAMS size");
#endif

#ifdef _MSC_VER
#pragma pack(pop)
#endif
