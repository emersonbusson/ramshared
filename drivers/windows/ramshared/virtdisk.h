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
	/* DT-5: CREATE requestor; balanced ObReference/Dereference. */
	PEPROCESS OwnerProcess;
} VIRTUAL_DISK, *PVIRTUAL_DISK;

NTSTATUS VdCreate(_Out_ PVIRTUAL_DISK Disk, _In_ const RAMSHARED_DISK_PARAMS *Params);
VOID VdDestroy(_Inout_ PVIRTUAL_DISK Disk);
VOID VdTranslateSrb(_Inout_ PVIRTUAL_DISK Disk, _In_ PVOID DevExt,
		    _Inout_ PSCSI_REQUEST_BLOCK Srb);
/* LUN present but no CREATE_DISK yet (DT-25). */
VOID VdTranslateSrbNoDisk(_In_ PVOID DevExt, _Inout_ PSCSI_REQUEST_BLOCK Srb);

VOID VdSetAdapterExt(_In_opt_ PVOID DeviceExtension);
PVOID VdGetAdapterExt(VOID);
NTSTATUS VdActivate(_In_ const RAMSHARED_DISK_PARAMS *Params);
VOID VdDeactivate(VOID);
PVIRTUAL_DISK VdGetActive(VOID);
BOOLEAN VdIsActive(VOID);
BOOLEAN VdOwnerMatches(_In_ PEPROCESS Process);
