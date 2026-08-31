/* SPDX-License-Identifier: MIT */
#pragma once

#include <ntddk.h>
#include <storport.h>
#include "protocol.h"
#include "queue.h"

#define VD_SECTOR_SIZE_512  512u
#define VD_SECTOR_SIZE_4096 4096u

/*
 * Type-safe helper for LBA to byte offset conversion with 64-bit overflow prevention.
 * Returns STATUS_SUCCESS on success, or STATUS_INVALID_PARAMETER on overflow.
 */
static inline NTSTATUS VdLbaToByteOffset(UINT64 Lba, UINT32 BlockSize, UINT64 *ByteOffset)
{
	if (!ByteOffset)
		return STATUS_INVALID_PARAMETER;

	if (BlockSize == 0)
		return STATUS_INVALID_PARAMETER;

	if (Lba > (~0ULL) / BlockSize)
		return STATUS_INVALID_PARAMETER;

	*ByteOffset = Lba * BlockSize;
	return STATUS_SUCCESS;
}

typedef enum _VD_STATE {
	VdStateNone = 0,
	VdStateCreated,
	VdStateOnline,
	VdStateFailed,
} VD_STATE;

typedef struct _VIRTUAL_DISK {
	UINT64 size_bytes;
	UINT32 block_size;
	UCHAR serial[16];
	RAMSHARED_QUEUE queue;
	volatile LONG state;
	volatile LONG InquirySeen;
	volatile LONG AppliedQueueDepth;
	UCHAR InquiryPathId;
	UCHAR InquiryTargetId;
	UCHAR InquiryLun;
	/* DT-5: CREATE requestor; balanced ObReference/Dereference. */
	PEPROCESS OwnerProcess;
} VIRTUAL_DISK, *PVIRTUAL_DISK;

NTSTATUS VdCreate(_Out_ PVIRTUAL_DISK Disk, _In_ const RAMSHARED_DISK_PARAMS *Params);
VOID VdDestroy(_Inout_ PVIRTUAL_DISK Disk);
VOID VdTranslateSrb(_Inout_ PVIRTUAL_DISK Disk, _In_ PVOID DevExt,
		    _Inout_ PSCSI_REQUEST_BLOCK Srb);
/* Control path present, but no LUN/PDO until CREATE_DISK (DT-25). */
VOID VdTranslateSrbNoDisk(_In_ PVOID DevExt, _Inout_ PSCSI_REQUEST_BLOCK Srb);

VOID VdSetAdapterExt(_In_opt_ PVOID DeviceExtension);
PVOID VdGetAdapterExt(VOID);
NTSTATUS VdActivate(_In_ const RAMSHARED_DISK_PARAMS *Params);
VOID VdDeactivate(VOID);
PVIRTUAL_DISK VdGetActive(VOID);
BOOLEAN VdIsActive(VOID);
BOOLEAN VdOwnerMatches(_In_ PEPROCESS Process);
BOOLEAN VdOwnerExited(VOID);
BOOLEAN VdApplyRegisteredQueueDepth(_Inout_ PVIRTUAL_DISK Disk,
				    _In_ PVOID DevExt);
