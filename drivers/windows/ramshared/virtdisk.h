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
} VIRTUAL_DISK, *PVIRTUAL_DISK;

NTSTATUS VdCreate(_Out_ PVIRTUAL_DISK Disk, _In_ const RAMSHARED_DISK_PARAMS *Params);
VOID VdTranslateSrb(_Inout_ PVIRTUAL_DISK Disk, _Inout_ PSCSI_REQUEST_BLOCK Srb);
