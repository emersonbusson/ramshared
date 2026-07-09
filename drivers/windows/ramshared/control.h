/* SPDX-License-Identifier: MIT */
#pragma once

#include <ntddk.h>
#include "protocol.h"

NTSTATUS
CtlCreateControlDevice(
	_In_ PDRIVER_OBJECT DriverObject,
	_In_ PCWSTR Sddl,
	_In_ const GUID *InterfaceGuid);

VOID CtlDeleteControlDevice(VOID);

/*
 * Install dispatch hooks: save StorPort originals, wrap for control device
 * only (DT-25). Call AFTER StorPortInitialize.
 */
NTSTATUS
CtlInstallDispatchHooks(_In_ PDRIVER_OBJECT DriverObject);

PDEVICE_OBJECT CtlGetControlDevice(VOID);
