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

DRIVER_DISPATCH CtlDeviceControl;
DRIVER_DISPATCH CtlCreateClose;
DRIVER_DISPATCH CtlCleanup;
