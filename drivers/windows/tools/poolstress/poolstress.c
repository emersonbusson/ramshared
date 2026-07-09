/* SPDX-License-Identifier: MIT */
/*
 * poolstress.sys — VM-only test driver for kernel-page residency drills.
 * SPEC ITEM-8 / DT-11 / DT-21. NEVER ship on host (RNF-6).
 *
 * IOCTLs:
 *   ALLOC(n_gb)  — ExAllocatePool2(POOL_FLAG_PAGED) + BCryptGenRandom + touch
 *   READBACK     — read all pages (force page-in)
 *   FREE         — free pool
 */
#include <ntddk.h>
#include <bcrypt.h>

#define POOLSTRESS_DEVICE_NAME L"\\Device\\RamSharedPoolStress"
#define POOLSTRESS_LINK_NAME   L"\\DosDevices\\RamSharedPoolStress"

#define IOCTL_POOLSTRESS_ALLOC \
	CTL_CODE(FILE_DEVICE_UNKNOWN, 0x900, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_POOLSTRESS_READBACK \
	CTL_CODE(FILE_DEVICE_UNKNOWN, 0x901, METHOD_BUFFERED, FILE_ANY_ACCESS)
#define IOCTL_POOLSTRESS_FREE \
	CTL_CODE(FILE_DEVICE_UNKNOWN, 0x902, METHOD_BUFFERED, FILE_ANY_ACCESS)

typedef struct _POOLSTRESS_ALLOC_IN {
	ULONG NGb;
} POOLSTRESS_ALLOC_IN;

static PDEVICE_OBJECT g_Device = NULL;
static PVOID g_Pool = NULL;
static SIZE_T g_PoolSize = 0;

static NTSTATUS
PoolstressDispatch(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	PIO_STACK_LOCATION irpSp;
	NTSTATUS status = STATUS_SUCCESS;
	ULONG_PTR info = 0;

	UNREFERENCED_PARAMETER(DeviceObject);
	irpSp = IoGetCurrentIrpStackLocation(Irp);

	switch (irpSp->MajorFunction) {
	case IRP_MJ_CREATE:
	case IRP_MJ_CLOSE:
		break;

	case IRP_MJ_DEVICE_CONTROL: {
		ULONG code = irpSp->Parameters.DeviceIoControl.IoControlCode;
		PVOID buf = Irp->AssociatedIrp.SystemBuffer;
		ULONG inLen = irpSp->Parameters.DeviceIoControl.InputBufferLength;

		if (code == IOCTL_POOLSTRESS_ALLOC) {
			POOLSTRESS_ALLOC_IN *in;
			SIZE_T bytes;
			ULONG i;

			if (inLen < sizeof(POOLSTRESS_ALLOC_IN) || buf == NULL) {
				status = STATUS_INVALID_PARAMETER;
				break;
			}
			if (g_Pool != NULL) {
				status = STATUS_DEVICE_BUSY;
				break;
			}
			in = (POOLSTRESS_ALLOC_IN *)buf;
			if (in->NGb == 0 || in->NGb > 16) {
				status = STATUS_INVALID_PARAMETER;
				break;
			}
			bytes = (SIZE_T)in->NGb << 30;
			g_Pool = ExAllocatePool2(POOL_FLAG_PAGED, bytes, 'ssPR');
			if (!g_Pool) {
				status = STATUS_INSUFFICIENT_RESOURCES;
				break;
			}
			g_PoolSize = bytes;
			/* Incompressible fill (DT-21) — prefer BCrypt; fallback pattern. */
			{
				NTSTATUS br = BCryptGenRandom(
					NULL, (PUCHAR)g_Pool,
					(ULONG)min(bytes, (SIZE_T)0x7fffffff),
					BCRYPT_USE_SYSTEM_PREFERRED_RNG);
				if (!NT_SUCCESS(br)) {
					for (i = 0; i < (ULONG)(bytes / sizeof(ULONG)); i++) {
						((PULONG)g_Pool)[i] = i * 2654435761u;
					}
				}
			}
			/* Touch every page so pages are resident then pageable. */
			for (i = 0; i < (ULONG)(bytes / PAGE_SIZE); i++) {
				volatile UCHAR *p = (PUCHAR)g_Pool + (SIZE_T)i * PAGE_SIZE;
				*p = *p;
			}
		} else if (code == IOCTL_POOLSTRESS_READBACK) {
			SIZE_T i;
			volatile UCHAR sum = 0;

			if (!g_Pool) {
				status = STATUS_INVALID_DEVICE_STATE;
				break;
			}
			for (i = 0; i < g_PoolSize; i += PAGE_SIZE) {
				sum ^= *((PUCHAR)g_Pool + i);
			}
			info = sum;
		} else if (code == IOCTL_POOLSTRESS_FREE) {
			if (g_Pool) {
				ExFreePoolWithTag(g_Pool, 'ssPR');
				g_Pool = NULL;
				g_PoolSize = 0;
			}
		} else {
			status = STATUS_INVALID_DEVICE_REQUEST;
		}
		break;
	}

	default:
		status = STATUS_INVALID_DEVICE_REQUEST;
		break;
	}

	Irp->IoStatus.Status = status;
	Irp->IoStatus.Information = info;
	IoCompleteRequest(Irp, IO_NO_INCREMENT);
	return status;
}

static VOID
PoolstressUnload(_In_ PDRIVER_OBJECT DriverObject)
{
	UNICODE_STRING link;

	UNREFERENCED_PARAMETER(DriverObject);
	if (g_Pool) {
		ExFreePoolWithTag(g_Pool, 'ssPR');
		g_Pool = NULL;
	}
	RtlInitUnicodeString(&link, POOLSTRESS_LINK_NAME);
	IoDeleteSymbolicLink(&link);
	if (g_Device) {
		IoDeleteDevice(g_Device);
		g_Device = NULL;
	}
}

NTSTATUS
DriverEntry(_In_ PDRIVER_OBJECT DriverObject, _In_ PUNICODE_STRING RegistryPath)
{
	UNICODE_STRING name, link;
	NTSTATUS status;

	UNREFERENCED_PARAMETER(RegistryPath);
	RtlInitUnicodeString(&name, POOLSTRESS_DEVICE_NAME);
	RtlInitUnicodeString(&link, POOLSTRESS_LINK_NAME);

	status = IoCreateDevice(DriverObject, 0, &name, FILE_DEVICE_UNKNOWN,
				FILE_DEVICE_SECURE_OPEN, FALSE, &g_Device);
	if (!NT_SUCCESS(status)) {
		return status;
	}
	status = IoCreateSymbolicLink(&link, &name);
	if (!NT_SUCCESS(status)) {
		IoDeleteDevice(g_Device);
		g_Device = NULL;
		return status;
	}

	DriverObject->MajorFunction[IRP_MJ_CREATE] = PoolstressDispatch;
	DriverObject->MajorFunction[IRP_MJ_CLOSE] = PoolstressDispatch;
	DriverObject->MajorFunction[IRP_MJ_DEVICE_CONTROL] = PoolstressDispatch;
	DriverObject->DriverUnload = PoolstressUnload;
	g_Device->Flags &= ~DO_DEVICE_INITIALIZING;
	return STATUS_SUCCESS;
}
