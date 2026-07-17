/* SPDX-License-Identifier: MIT */
/*
 * RamShared StorPort virtual miniport — DriverEntry + HW callbacks.
 * SPEC ITEM-5 / RF-1 / DT-1 / DT-23 / DT-25.
 *
 * Day-0: virtual miniport (STOR_FEATURE_VIRTUAL_MINIPORT) + control device.
 * Dispatch hooks forward non-control IRPs to StorPort (DT-25).
 *
 * Win8+ uses consolidated HW_INITIALIZATION_DATA with FeatureSupport flag
 * (not legacy VIRTUAL_HW_INITIALIZATION_DATA alone). Required virtual entry
 * points: HwAdapterControl, HwFreeAdapterResources.
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
	PRAMSHARED_ADAPTER_EXT ext = (PRAMSHARED_ADAPTER_EXT)DeviceExtension;

	UNREFERENCED_PARAMETER(HwContext);
	UNREFERENCED_PARAMETER(BusInformation);
	UNREFERENCED_PARAMETER(LowerDevice);
	UNREFERENCED_PARAMETER(ArgumentString);

	*Again = FALSE;

	if (ext != NULL)
		ext->AdapterStopped = FALSE;

	/*
	 * Complete PORT_CONFIGURATION_INFORMATION only — do NOT zero it and do
	 * NOT clear Master/ScatterGather/NeedPhysicalAddresses/TaggedQueuing
	 * (Storport pre-initializes those to TRUE; forcing FALSE causes
	 * CM_PROB_FAILED_START / STATUS_DEVICE_CONFIGURATION_ERROR).
	 *
	 * VirtualDevice=TRUE is the required virtual-miniport marker so storport
	 * creates LU child PDOs for Get-Disk / VPD identity.
	 */
	ConfigInfo->VirtualDevice = TRUE;
	ConfigInfo->NumberOfBuses = 1;
	ConfigInfo->MaximumNumberOfTargets = 1;
	ConfigInfo->MaximumNumberOfLogicalUnits = 1;
	ConfigInfo->MaximumTransferLength = RAMSHARED_MAX_IO;
	ConfigInfo->NumberOfPhysicalBreaks = SP_UNINITIALIZED_VALUE;
	ConfigInfo->AlignmentMask = 0; /* byte aligned; no HBA constraint */
	ConfigInfo->CachesData = FALSE;
	ConfigInfo->MapBuffers = STOR_MAP_NON_READ_WRITE_BUFFERS;
	ConfigInfo->SynchronizationModel = StorSynchronizeFullDuplex;
	ConfigInfo->HwMSInterruptRoutine = NULL;
	/* Initiator distinct from target 0; keep port value if already assigned. */
	if ((UCHAR)ConfigInfo->InitiatorBusId[0] == 0xFF)
		ConfigInfo->InitiatorBusId[0] = (CCHAR)7;

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
	ext->AdapterStopped = FALSE;
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
	PRAMSHARED_ADAPTER_EXT ext = (PRAMSHARED_ADAPTER_EXT)DeviceExtension;
	PVIRTUAL_DISK disk;
	PSCSI_PNP_REQUEST_BLOCK pnp;
	PSTOR_DEVICE_CAPABILITIES caps;

	if (Srb == NULL)
		return TRUE;

	if (ext != NULL && ext->AdapterStopped) {
		Srb->SrbStatus = SRB_STATUS_NO_DEVICE;
		StorPortNotification(RequestComplete, DeviceExtension, Srb);
		return TRUE;
	}

	/*
	 * Non-SCSI SRBs must not fall into CDB decode — mis-handling PnP/Power
	 * prevents LU child PDO creation (adapter OK, Get-Disk empty).
	 */
	switch (Srb->Function) {
	case SRB_FUNCTION_EXECUTE_SCSI:
		break;

	case SRB_FUNCTION_PNP:
		pnp = (PSCSI_PNP_REQUEST_BLOCK)Srb;
		if (pnp->PnPAction == StorQueryCapabilities &&
		    pnp->DataBuffer != NULL &&
		    pnp->DataTransferLength >= sizeof(STOR_DEVICE_CAPABILITIES)) {
			caps = (PSTOR_DEVICE_CAPABILITIES)pnp->DataBuffer;
			RtlZeroMemory(caps, sizeof(*caps));
			caps->Version = 1;
			caps->Removable = 0;
			caps->UniqueID = 1;
			caps->SilentInstall = 1;
			caps->SurpriseRemovalOK = 1;
			caps->NoDisplayInUI = 0;
		}
		Srb->SrbStatus = SRB_STATUS_SUCCESS;
		StorPortNotification(RequestComplete, DeviceExtension, Srb);
		return TRUE;

	case SRB_FUNCTION_POWER:
	case SRB_FUNCTION_RESET_BUS:
	case SRB_FUNCTION_RESET_DEVICE:
	case SRB_FUNCTION_RESET_LOGICAL_UNIT:
	case SRB_FUNCTION_FLUSH:
	case SRB_FUNCTION_SHUTDOWN:
	case SRB_FUNCTION_WMI:
	case SRB_FUNCTION_IO_CONTROL:
		Srb->SrbStatus = SRB_STATUS_SUCCESS;
		StorPortNotification(RequestComplete, DeviceExtension, Srb);
		return TRUE;

	default:
		Srb->SrbStatus = SRB_STATUS_INVALID_REQUEST;
		StorPortNotification(RequestComplete, DeviceExtension, Srb);
		return TRUE;
	}

	disk = VdGetActive();

	/*
	 * The control device exists independently. Before CREATE, REPORT LUNS is
	 * empty and INQUIRY returns NO_DEVICE so Windows cannot cache placeholder
	 * serial/capacity in a child PDO. CREATE + BusChangeDetected enumerates
	 * the real LUN atomically with its run-specific identity (DT-25/RF-4).
	 */
	if (disk == NULL) {
		VdTranslateSrbNoDisk(DeviceExtension, Srb);
		return TRUE;
	}
	VdTranslateSrb(disk, DeviceExtension, Srb);
	return TRUE;
}

SCSI_ADAPTER_CONTROL_STATUS
HwStorAdapterControl(
	_In_ PVOID DeviceExtension,
	_In_ SCSI_ADAPTER_CONTROL_TYPE ControlType,
	_In_ PVOID Parameters)
{
	PRAMSHARED_ADAPTER_EXT ext = (PRAMSHARED_ADAPTER_EXT)DeviceExtension;
	PSCSI_SUPPORTED_CONTROL_TYPE_LIST list;

	switch (ControlType) {
	case ScsiQuerySupportedControlTypes:
		if (Parameters == NULL)
			return ScsiAdapterControlUnsuccessful;
		list = (PSCSI_SUPPORTED_CONTROL_TYPE_LIST)Parameters;
		if (list->MaxControlType >= (ULONG)ScsiQuerySupportedControlTypes)
			list->SupportedTypeList[ScsiQuerySupportedControlTypes] = TRUE;
		if (list->MaxControlType >= (ULONG)ScsiStopAdapter)
			list->SupportedTypeList[ScsiStopAdapter] = TRUE;
		if (list->MaxControlType >= (ULONG)ScsiRestartAdapter)
			list->SupportedTypeList[ScsiRestartAdapter] = TRUE;
		return ScsiAdapterControlSuccess;

	case ScsiStopAdapter:
		if (ext != NULL)
			ext->AdapterStopped = TRUE;
		/* Do not tear mapped queues here — userspace UNREGISTER owns that. */
		return ScsiAdapterControlSuccess;

	case ScsiRestartAdapter:
		if (ext != NULL)
			ext->AdapterStopped = FALSE;
		VdSetAdapterExt(DeviceExtension);
		return ScsiAdapterControlSuccess;

	default:
		return ScsiAdapterControlUnsuccessful;
	}
}

VOID
HwStorFreeAdapterResources(_In_ PVOID DeviceExtension)
{
	PRAMSHARED_ADAPTER_EXT ext = (PRAMSHARED_ADAPTER_EXT)DeviceExtension;

	/*
	 * Virtual miniport required teardown: drop adapter pointer so
	 * completion paths cannot touch a dying extension.
	 */
	if (VdGetAdapterExt() == DeviceExtension)
		VdSetAdapterExt(NULL);
	if (ext != NULL) {
		ext->AdapterStopped = TRUE;
		ext->Disk = NULL;
		ext->Queue = NULL;
		ext->QueueRegistered = FALSE;
	}
}

NTSTATUS
DriverEntry(_In_ PDRIVER_OBJECT DriverObject, _In_ PUNICODE_STRING RegistryPath)
{
	HW_INITIALIZATION_DATA hw;
	NTSTATUS status;

	RtlZeroMemory(&hw, sizeof(hw));
	hw.HwInitializationDataSize = sizeof(HW_INITIALIZATION_DATA);
	hw.AdapterInterfaceType = Internal;
	hw.HwInitialize = HwStorInitialize;
	hw.HwStartIo = HwStorStartIo;
	hw.HwInterrupt = NULL; /* virtual: no hardware interrupt */
	/* Virtual FindAdapter signature (LowerDevice) — cast via PVOID field. */
	hw.HwFindAdapter = (PVOID)HwStorFindAdapter;
	hw.HwResetBus = HwStorResetBus;
	hw.HwDmaStarted = NULL;
	hw.HwAdapterState = NULL;
	hw.DeviceExtensionSize = sizeof(RAMSHARED_ADAPTER_EXT);
	hw.SpecificLuExtensionSize = 0;
	hw.SrbExtensionSize = 0;
	hw.NumberOfAccessRanges = 0;
	hw.MapBuffers = STOR_MAP_NON_READ_WRITE_BUFFERS;
	/* Storport expects TRUE even for virtual miniports (bus-width DMA model). */
	hw.NeedPhysicalAddresses = TRUE;
	hw.TaggedQueuing = TRUE;
	hw.AutoRequestSense = TRUE;
	hw.MultipleRequestPerLu = TRUE;
	hw.ReceiveEvent = FALSE;
	hw.HwAdapterControl = HwStorAdapterControl;
	hw.HwBuildIo = NULL; /* physical-only; virtual must leave NULL */
	hw.HwFreeAdapterResources = HwStorFreeAdapterResources;
	hw.HwProcessServiceRequest = NULL;
	hw.HwCompleteServiceIrp = NULL;
	hw.HwInitializeTracing = NULL;
	hw.HwCleanupTracing = NULL;
	hw.HwTracingEnabled = NULL;
	/* Critical: without this flag storport treats us as physical HBA. */
	hw.FeatureSupport = STOR_FEATURE_VIRTUAL_MINIPORT;
	hw.SrbTypeFlags = SRB_TYPE_FLAG_SCSI_REQUEST_BLOCK;
	hw.AddressTypeFlags = ADDRESS_TYPE_FLAG_BTL8;
	hw.Reserved1 = 0;
	hw.HwUnitControl = NULL;

	status = StorPortInitialize(DriverObject, RegistryPath, &hw, NULL);
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
