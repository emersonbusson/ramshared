/* SPDX-License-Identifier: MIT */
/*
 * RamShared Windows driver ↔ service ABI (frozen for IMPL ITEM-4).
 * SPEC: docs/specs/no-milestone/windows-swap-driver/SPEC.md (DT-17, DT-18, DT-22).
 *
 * Layout is the single source of truth. The Rust mirror lives in
 * crates/ramshared-winsvc/src/proto.rs and must match byte-for-byte
 * (size asserts + golden-bytes tests).
 *
 * Do not log kernel virtual addresses from producers of this ABI (KASLR).
 */
#pragma once

#include <stdint.h>

#ifdef _MSC_VER
#pragma pack(push, 8)
#endif

#define RAMSHARED_ABI_VERSION 1u
#define RAMSHARED_MAX_QD 256u      /* queue_depth max; power of two */
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
	uint64_t tag;
	uint32_t op;
	uint32_t flags;
	uint64_t offset;
	uint32_t len;
	uint32_t buf_slot;
} RAMSHARED_SQE;

/* service -> driver, 16 bytes */
typedef struct _RAMSHARED_CQE {
	uint64_t tag;
	int32_t status;
	uint32_t reserved;
} RAMSHARED_CQE;

/* precedes entries[]; SPSC */
typedef struct _RAMSHARED_RING_HDR {
	uint32_t magic;
	uint32_t entries; /* = queue_depth (power of two) */
	volatile uint32_t head;
	volatile uint32_t tail;
} RAMSHARED_RING_HDR;

/* payload of IOCTL_RAMSHARED_REGISTER_QUEUE */
typedef struct _RAMSHARED_REGISTER {
	uint32_t abi_version;
	uint32_t disk_id;
	uint32_t queue_depth;
	uint32_t block_size;
	uint32_t max_io_bytes;
	uint32_t reserved;
	uint64_t sq_ring_va;
	uint64_t cq_ring_va;
	uint64_t data_area_va;
	uint64_t data_area_len;
	/* auxiliary (DT-22); primary wake path = COMMIT_AND_FETCH IRP */
	uint64_t sq_event_handle;
	uint64_t cq_event_handle;
} RAMSHARED_REGISTER;

/* IOCTL_RAMSHARED_CREATE_DISK */
typedef struct _RAMSHARED_DISK_PARAMS {
	uint64_t size_bytes; /* multiple of block_size */
	uint32_t block_size; /* 512 or 4096 */
	uint32_t reserved;
	unsigned char serial[16]; /* INQUIRY VPD / stable id */
} RAMSHARED_DISK_PARAMS;

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
/* C++ compile-time size checks when available outside WDK */
static_assert(sizeof(RAMSHARED_SQE) == 32, "RAMSHARED_SQE size");
static_assert(sizeof(RAMSHARED_CQE) == 16, "RAMSHARED_CQE size");
static_assert(sizeof(RAMSHARED_RING_HDR) == 16, "RAMSHARED_RING_HDR size");
static_assert(sizeof(RAMSHARED_REGISTER) == 72, "RAMSHARED_REGISTER size");
static_assert(sizeof(RAMSHARED_DISK_PARAMS) == 32, "RAMSHARED_DISK_PARAMS size");
#endif

#ifdef _MSC_VER
#pragma pack(pop)
#endif
