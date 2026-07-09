/* SPDX-License-Identifier: MIT */
/*
 * Control device IOCTL dispatch + security (SPEC ITEM-5 / RNF-4 / DT-1).
 */
#include "control.h"
#include "queue.h"
#include "virtdisk.h"

/* CTL_CODE helpers — FILE_DEVICE_MASS_STORAGE = 0x0000002d */
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
static VIRTUAL_DISK g_Disk;
static RAMSHARED_QUEUE g_Queue;
static BOOLEAN g_DiskCreated = FALSE;

NTSTATUS
CtlCreateControlDevice(
	_In_ PDRIVER_OBJECT DriverObject,
	_In_ PCWSTR Sddl,
	_In_ const GUID *InterfaceGuid)
{
	NTSTATUS status;
	UNICODE_STRING sddl;
	PSECURITY_DESCRIPTOR sd = NULL;

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

	DriverObject->MajorFunction[IRP_MJ_CREATE] = CtlCreateClose;
	DriverObject->MajorFunction[IRP_MJ_CLOSE] = CtlCreateClose;
	DriverObject->MajorFunction[IRP_MJ_CLEANUP] = CtlCleanup;
	DriverObject->MajorFunction[IRP_MJ_DEVICE_CONTROL] = CtlDeviceControl;

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

NTSTATUS
CtlCreateClose(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	UNREFERENCED_PARAMETER(DeviceObject);
	Irp->IoStatus.Status = STATUS_SUCCESS;
	Irp->IoStatus.Information = 0;
	IoCompleteRequest(Irp, IO_NO_INCREMENT);
	return STATUS_SUCCESS;
}

NTSTATUS
CtlCleanup(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	UNREFERENCED_PARAMETER(DeviceObject);
	/* Service handle closed → deterministic crash containment (DT-10). */
	QTeardownOnCrash(&g_Queue);
	Irp->IoStatus.Status = STATUS_SUCCESS;
	Irp->IoStatus.Information = 0;
	IoCompleteRequest(Irp, IO_NO_INCREMENT);
	return STATUS_SUCCESS;
}

NTSTATUS
CtlDeviceControl(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	PIO_STACK_LOCATION irpSp;
	ULONG code;
	ULONG inLen;
	PVOID buf;
	NTSTATUS status = STATUS_INVALID_DEVICE_REQUEST;
	ULONG_PTR info = 0;

	UNREFERENCED_PARAMETER(DeviceObject);
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
		status = QRegister(&g_Queue, (const RAMSHARED_REGISTER *)buf, Irp->RequestorMode);
		break;

	case IOCTL_RAMSHARED_UNREGISTER_QUEUE:
		QUnregister(&g_Queue);
		status = STATUS_SUCCESS;
		break;

	case IOCTL_RAMSHARED_COMMIT_AND_FETCH:
		status = QCommitAndFetch(&g_Queue, Irp);
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
		if (g_DiskCreated) {
			status = STATUS_DEVICE_BUSY;
			break;
		}
		status = VdCreate(&g_Disk, (const RAMSHARED_DISK_PARAMS *)buf);
		if (NT_SUCCESS(status)) {
			g_Disk.queue = g_Queue;
			g_DiskCreated = TRUE;
		}
		break;

	case IOCTL_RAMSHARED_DESTROY_DISK:
		/* Must not destroy while pagefile active — enforced in service (DT-9). */
		QUnregister(&g_Queue);
		g_DiskCreated = FALSE;
		RtlZeroMemory(&g_Disk, sizeof(g_Disk));
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
