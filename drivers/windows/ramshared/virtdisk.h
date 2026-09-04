/* SPDX-License-Identifier: MIT */
#pragma once

#include <ntddk.h>
#include <storport.h>
#include "protocol.h"
#include "queue.h"

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

/*
 * Defensive Physical Check:
 * Validate sector-to-byte translations against 64-bit integer overflow and disk capacity.
 * LBA * block_size could overflow MAXULONG64. We avoid this by bounds-checking LBA.
 */
FORCEINLINE
BOOLEAN
VdCheckLbaBounds(
	_In_ PVIRTUAL_DISK Disk,
	_In_ UINT64 Lba,
	_In_ UINT32 Length,
	_Out_ UINT64 *Offset
	)
{
	UINT64 max_lba;

	if (Disk == NULL || Disk->block_size == 0 || Disk->size_bytes == 0)
		return FALSE;

	max_lba = Disk->size_bytes / Disk->block_size;

	if (Lba >= max_lba || (Length & (Disk->block_size - 1)) != 0)
		return FALSE;

	*Offset = Lba * Disk->block_size;

	if (*Offset > Disk->size_bytes || Length > Disk->size_bytes - *Offset)
		return FALSE;

	return TRUE;
}
