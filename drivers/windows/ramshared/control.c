/* SPDX-License-Identifier: MIT */
/*
 * Control device IOCTL dispatch + security (SPEC ITEM-5 / RNF-4 / DT-1 / DT-25).
 */
#include "control.h"
#include "queue.h"
#include "virtdisk.h"
#include <wdmsec.h>

#ifndef FILE_DEVICE_MASS_STORAGE
#define FILE_DEVICE_MASS_STORAGE 0x0000002d
#endif

#define IOCTL_RAMSHARED_REGISTER_QUEUE \
	CTL_CODE(FILE_DEVICE_MASS_STORAGE, 0x800 | RAMSHARED_IOCTL_FN_REGISTER_QUEUE, \
		 METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)
#define IOCTL_RAMSHARED_UNREGISTER_QUEUE \
	CTL_CODE(FILE_DEVICE_MASS_STORAGE, 0x800 | RAMSHARED_IOCTL_FN_UNREGISTER_QUEUE, \
		 METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)
#define IOCTL_RAMSHARED_COMMIT_AND_FETCH \
	CTL_CODE(FILE_DEVICE_MASS_STORAGE, 0x800 | RAMSHARED_IOCTL_FN_COMMIT_AND_FETCH, \
		 METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)
#define IOCTL_RAMSHARED_CREATE_DISK \
	CTL_CODE(FILE_DEVICE_MASS_STORAGE, 0x800 | RAMSHARED_IOCTL_FN_CREATE_DISK, \
		 METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)
#define IOCTL_RAMSHARED_DESTROY_DISK \
	CTL_CODE(FILE_DEVICE_MASS_STORAGE, 0x800 | RAMSHARED_IOCTL_FN_DESTROY_DISK, \
		 METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS)

static PDEVICE_OBJECT g_ControlDevice = NULL;
static UNICODE_STRING g_ControlName;
static UNICODE_STRING g_ControlLink;

/* StorPort originals — never drop (DT-25). */
static PDRIVER_DISPATCH g_OrigCreate;
static PDRIVER_DISPATCH g_OrigClose;
static PDRIVER_DISPATCH g_OrigCleanup;
static PDRIVER_DISPATCH g_OrigDeviceControl;

PDEVICE_OBJECT
CtlGetControlDevice(VOID)
{
	return g_ControlDevice;
}

static NTSTATUS
CtlForward(
	_In_ PDRIVER_DISPATCH Orig,
	_In_ PDEVICE_OBJECT DeviceObject,
	_Inout_ PIRP Irp)
{
	if (Orig != NULL) {
		return Orig(DeviceObject, Irp);
	}
	Irp->IoStatus.Status = STATUS_INVALID_DEVICE_REQUEST;
	Irp->IoStatus.Information = 0;
	IoCompleteRequest(Irp, IO_NO_INCREMENT);
	return STATUS_INVALID_DEVICE_REQUEST;
}

static NTSTATUS
CtlDispatchCreateClose(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	if (DeviceObject != g_ControlDevice) {
		return CtlForward(g_OrigCreate, DeviceObject, Irp);
	}
	Irp->IoStatus.Status = STATUS_SUCCESS;
	Irp->IoStatus.Information = 0;
	IoCompleteRequest(Irp, IO_NO_INCREMENT);
	return STATUS_SUCCESS;
}

static NTSTATUS
CtlDispatchCleanup(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	if (DeviceObject != g_ControlDevice) {
		return CtlForward(g_OrigCleanup, DeviceObject, Irp);
	}
	/* Service handle closed → deterministic crash containment (DT-10). */
	if (VdIsActive()) {
		QTeardownOnCrash(&VdGetActive()->queue);
	}
	Irp->IoStatus.Status = STATUS_SUCCESS;
	Irp->IoStatus.Information = 0;
	IoCompleteRequest(Irp, IO_NO_INCREMENT);
	return STATUS_SUCCESS;
}

static NTSTATUS
CtlDispatchDeviceControl(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	PIO_STACK_LOCATION irpSp;
	ULONG code;
	ULONG inLen;
	PVOID buf;
	NTSTATUS status = STATUS_INVALID_DEVICE_REQUEST;
	ULONG_PTR info = 0;

	if (DeviceObject != g_ControlDevice) {
		return CtlForward(g_OrigDeviceControl, DeviceObject, Irp);
	}

	irpSp = IoGetCurrentIrpStackLocation(Irp);
	code = irpSp->Parameters.DeviceIoControl.IoControlCode;
	inLen = irpSp->Parameters.DeviceIoControl.InputBufferLength;
	buf = Irp->AssociatedIrp.SystemBuffer;

	switch (code) {
	case IOCTL_RAMSHARED_REGISTER_QUEUE:
		if (inLen != sizeof(RAMSHARED_REGISTER) || buf == NULL) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
		if (!VdIsActive()) {
			status = STATUS_DEVICE_NOT_READY;
			break;
		}
		status = QRegister(&VdGetActive()->queue,
				   (const RAMSHARED_REGISTER *)buf,
				   Irp->RequestorMode);
		break;

	case IOCTL_RAMSHARED_UNREGISTER_QUEUE:
		if (VdIsActive()) {
			QUnregister(&VdGetActive()->queue);
		}
		status = STATUS_SUCCESS;
		break;

	case IOCTL_RAMSHARED_COMMIT_AND_FETCH:
		if (!VdIsActive()) {
			status = STATUS_DEVICE_NOT_READY;
			break;
		}
		status = QCommitAndFetch(&VdGetActive()->queue, Irp);
		if (status == STATUS_PENDING) {
			return STATUS_PENDING;
		}
		info = Irp->IoStatus.Information;
		break;

	case IOCTL_RAMSHARED_CREATE_DISK:
		if (inLen != sizeof(RAMSHARED_DISK_PARAMS) || buf == NULL) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
		status = VdActivate((const RAMSHARED_DISK_PARAMS *)buf);
		break;

	case IOCTL_RAMSHARED_DESTROY_DISK:
		VdDeactivate();
		status = STATUS_SUCCESS;
		break;

	default:
		status = STATUS_INVALID_DEVICE_REQUEST;
		break;
	}

	Irp->IoStatus.Status = status;
	Irp->IoStatus.Information = info;
	IoCompleteRequest(Irp, IO_NO_INCREMENT);
	return status;
}

NTSTATUS
CtlInstallDispatchHooks(_In_ PDRIVER_OBJECT DriverObject)
{
	g_OrigCreate = DriverObject->MajorFunction[IRP_MJ_CREATE];
	g_OrigClose = DriverObject->MajorFunction[IRP_MJ_CLOSE];
	g_OrigCleanup = DriverObject->MajorFunction[IRP_MJ_CLEANUP];
	g_OrigDeviceControl = DriverObject->MajorFunction[IRP_MJ_DEVICE_CONTROL];

	DriverObject->MajorFunction[IRP_MJ_CREATE] = CtlDispatchCreateClose;
	DriverObject->MajorFunction[IRP_MJ_CLOSE] = CtlDispatchCreateClose;
	DriverObject->MajorFunction[IRP_MJ_CLEANUP] = CtlDispatchCleanup;
	DriverObject->MajorFunction[IRP_MJ_DEVICE_CONTROL] = CtlDispatchDeviceControl;
	return STATUS_SUCCESS;
}

NTSTATUS
CtlCreateControlDevice(
	_In_ PDRIVER_OBJECT DriverObject,
	_In_ PCWSTR Sddl,
	_In_ const GUID *InterfaceGuid)
{
	NTSTATUS status;
	UNICODE_STRING sddl;

	UNREFERENCED_PARAMETER(InterfaceGuid);

	RtlInitUnicodeString(&g_ControlName, L"\\Device\\RamSharedCtl");
	RtlInitUnicodeString(&g_ControlLink, L"\\DosDevices\\RamSharedCtl");
	RtlInitUnicodeString(&sddl, (PWSTR)Sddl);

	status = IoCreateDeviceSecure(
		DriverObject,
		0,
		&g_ControlName,
		FILE_DEVICE_UNKNOWN,
		FILE_DEVICE_SECURE_OPEN,
		FALSE,
		&sddl,
		NULL,
		&g_ControlDevice);
	if (!NT_SUCCESS(status)) {
		return status;
	}

	status = IoCreateSymbolicLink(&g_ControlLink, &g_ControlName);
	if (!NT_SUCCESS(status)) {
		IoDeleteDevice(g_ControlDevice);
		g_ControlDevice = NULL;
		return status;
	}

	g_ControlDevice->Flags |= DO_BUFFERED_IO;
	g_ControlDevice->Flags &= ~DO_DEVICE_INITIALIZING;
	return STATUS_SUCCESS;
}

VOID
CtlDeleteControlDevice(VOID)
{
	if (g_ControlDevice) {
		IoDeleteSymbolicLink(&g_ControlLink);
		IoDeleteDevice(g_ControlDevice);
		g_ControlDevice = NULL;
	}
}

/* Legacy names for any external refs — map to wrappers. */
NTSTATUS
CtlCreateClose(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	return CtlDispatchCreateClose(DeviceObject, Irp);
}

NTSTATUS
CtlCleanup(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	return CtlDispatchCleanup(DeviceObject, Irp);
}

NTSTATUS
CtlDeviceControl(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	return CtlDispatchDeviceControl(DeviceObject, Irp);
}
