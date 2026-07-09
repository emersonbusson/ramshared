/* SPDX-License-Identifier: MIT */
/*
 * RamShared StorPort virtual miniport — driver entry surface.
 * SPEC: docs/specs/no-milestone/windows-swap-driver/SPEC.md ITEM-5 / DT-1 / DT-23.
 *
 * Build: WDK/EWDK MSBuild (ramshared.vcxproj). Not buildable on Linux hosts.
 */
#pragma once

#include <ntddk.h>
#include <storport.h>
#include "protocol.h"

/* Device extension for the StorPort adapter (virtual). */
typedef struct _RAMSHARED_ADAPTER_EXT {
	PVOID StorPortExt; /* reserved for StorPort-owned memory */
	struct _VIRTUAL_DISK *Disk;
	struct _RAMSHARED_QUEUE *Queue;
	PDEVICE_OBJECT ControlDevice;
	UNICODE_STRING ControlLink;
	BOOLEAN QueueRegistered;
} RAMSHARED_ADAPTER_EXT, *PRAMSHARED_ADAPTER_EXT;

DRIVER_INITIALIZE DriverEntry;

ULONG
HwStorFindAdapter(
	_In_ PVOID DeviceExtension,
	_In_ PVOID HwContext,
	_In_ PVOID BusInformation,
	_In_ PCHAR ArgumentString,
	_Inout_ PPORT_CONFIGURATION_INFORMATION ConfigInfo,
	_In_ PBOOLEAN Again);

BOOLEAN HwStorInitialize(_In_ PVOID DeviceExtension);
BOOLEAN HwStorResetBus(_In_ PVOID DeviceExtension, _In_ ULONG PathId);
BOOLEAN HwStorStartIo(_In_ PVOID DeviceExtension, _In_ PSCSI_REQUEST_BLOCK Srb);
