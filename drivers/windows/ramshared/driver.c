/* SPDX-License-Identifier: MIT */
/*
 * RamShared StorPort virtual miniport — DriverEntry + HW callbacks.
 * SPEC ITEM-5 / RF-1 / DT-1 / DT-23 / DT-25.
 *
 * Day-0: virtual miniport + control device (IoCreateDeviceSecure).
 * Dispatch hooks forward non-control IRPs to StorPort (DT-25).
 */
#include "driver.h"
#include "control.h"
#include "virtdisk.h"
#include "queue.h"

#include <initguid.h>
/* {A5B3C1D0-8E4F-4A2B-9C7D-1E2F3A4B5C6D} - control device interface GUID */
DEFINE_GUID(GUID_DEVINTERFACE_RAMSHARED_CTL,
	0xa5b3c1d0, 0x8e4f, 0x4a2b, 0x9c, 0x7d, 0x1e, 0x2f, 0x3a, 0x4b, 0x5c, 0x6d);

/* SDDL: SYSTEM + Administrators only (RNF-4 / DT-1). */
static const WCHAR RamsharedSddl[] =
	L"D:P(A;;GA;;;SY)(A;;GA;;;BA)";

ULONG
HwStorFindAdapter(
	_In_ PVOID DeviceExtension,
	_In_ PVOID HwContext,
	_In_ PVOID BusInformation,
	_In_ PVOID LowerDevice,
	_In_ PCHAR ArgumentString,
	_Inout_ PPORT_CONFIGURATION_INFORMATION ConfigInfo,
	_In_ PBOOLEAN Again)
{
	UNREFERENCED_PARAMETER(HwContext);
	UNREFERENCED_PARAMETER(BusInformation);
	UNREFERENCED_PARAMETER(LowerDevice);
	UNREFERENCED_PARAMETER(ArgumentString);
	UNREFERENCED_PARAMETER(DeviceExtension);

	*Again = FALSE;

	/* One virtual bus / target / LUN — no real port I/O. */
	ConfigInfo->NumberOfBuses = 1;
	ConfigInfo->MaximumNumberOfTargets = 1;
	ConfigInfo->MaximumNumberOfLogicalUnits = 1;
	ConfigInfo->MaximumTransferLength = RAMSHARED_MAX_IO;
	ConfigInfo->NumberOfPhysicalBreaks = SP_UNINITIALIZED_VALUE;
	ConfigInfo->AlignmentMask = 0x1;
	ConfigInfo->CachesData = FALSE;
	ConfigInfo->MapBuffers = STOR_MAP_NON_READ_WRITE_BUFFERS;
	ConfigInfo->SynchronizationModel = StorSynchronizeFullDuplex;
	ConfigInfo->HwMSInterruptRoutine = NULL;
	ConfigInfo->AdapterInterfaceType = Internal;
	ConfigInfo->Dma64BitAddresses = SCSI_DMA64_MINIPORT_SUPPORTED;
	ConfigInfo->ResetTargetSupported = TRUE;
	ConfigInfo->VirtualDevice = TRUE;
	ConfigInfo->WmiDataProvider = FALSE;

	return SP_RETURN_FOUND;
}

BOOLEAN
HwStorInitialize(_In_ PVOID DeviceExtension)
{
	PRAMSHARED_ADAPTER_EXT ext = (PRAMSHARED_ADAPTER_EXT)DeviceExtension;

	ext->Disk = NULL;
	ext->Queue = NULL;
	ext->ControlDevice = NULL;
	ext->QueueRegistered = FALSE;
	VdSetAdapterExt(DeviceExtension);
	return TRUE;
}

BOOLEAN
HwStorResetBus(_In_ PVOID DeviceExtension, _In_ ULONG PathId)
{
	UNREFERENCED_PARAMETER(DeviceExtension);
	UNREFERENCED_PARAMETER(PathId);
	return TRUE;
}

BOOLEAN
HwStorStartIo(_In_ PVOID DeviceExtension, _In_ PSCSI_REQUEST_BLOCK Srb)
{
	PVIRTUAL_DISK disk = VdGetActive();

	/*
	 * LUN always present (DT-25). Without CREATE: INQUIRY/capacity handled
	 * with not-ready semantics inside VdTranslateSrb when disk is NULL —
	 * use a zero-size pseudo path via VdTranslateSrbNull.
	 */
	if (disk == NULL) {
		VdTranslateSrbNoDisk(DeviceExtension, Srb);
		return TRUE;
	}
	VdTranslateSrb(disk, DeviceExtension, Srb);
	return TRUE;
}

NTSTATUS
DriverEntry(_In_ PDRIVER_OBJECT DriverObject, _In_ PUNICODE_STRING RegistryPath)
{
	VIRTUAL_HW_INITIALIZATION_DATA hw;
	NTSTATUS status;

	RtlZeroMemory(&hw, sizeof(hw));
	hw.HwInitializationDataSize = sizeof(VIRTUAL_HW_INITIALIZATION_DATA);
	hw.AdapterInterfaceType = Internal;
	hw.HwInitialize = HwStorInitialize;
	hw.HwStartIo = HwStorStartIo;
	hw.HwFindAdapter = HwStorFindAdapter;
	hw.HwResetBus = HwStorResetBus;
	hw.DeviceExtensionSize = sizeof(RAMSHARED_ADAPTER_EXT);
	hw.MapBuffers = STOR_MAP_NON_READ_WRITE_BUFFERS;
	hw.TaggedQueuing = TRUE;
	hw.AutoRequestSense = TRUE;
	hw.MultipleRequestPerLu = TRUE;

	status = StorPortInitialize(DriverObject, RegistryPath,
				    (PHW_INITIALIZATION_DATA)&hw, NULL);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	/* Hook dispatch AFTER StorPort owns MajorFunction (DT-25). */
	status = CtlInstallDispatchHooks(DriverObject);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	status = CtlCreateControlDevice(DriverObject, RamsharedSddl,
					&GUID_DEVINTERFACE_RAMSHARED_CTL);
	return status;
}
