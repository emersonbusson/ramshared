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

#pragma pack(push, 8)
typedef struct _POOLSTRESS_ALLOC_IN {
	UINT64 Bytes;
	UINT32 PoolTag;
} POOLSTRESS_ALLOC_IN;
#pragma pack(pop)

static PDEVICE_OBJECT g_Device = NULL;
static PVOID g_Pool = NULL;
static SIZE_T g_PoolSize = 0;
static UINT32 g_PoolTag = 0;

static VOID
PoolstressFill(_Inout_updates_bytes_(bytes) PUCHAR pool, SIZE_T bytes)
{
	SIZE_T offset = 0;

	if (pool == NULL || bytes == 0)
		return;
	while (offset < bytes) {
		ULONG i;
		ULONG chunk = (ULONG)min(bytes - offset, (SIZE_T)(16 * 1024 * 1024));
		NTSTATUS status;

		if ((SIZE_T)chunk > bytes - offset)
			return;
#pragma warning(suppress:6386) /* offset + chunk is bounded above */
		status = BCryptGenRandom(NULL, pool + offset, chunk,
					BCRYPT_USE_SYSTEM_PREFERRED_RNG);
		if (!NT_SUCCESS(status)) {
			for (i = 0; i < chunk; i++) {
#pragma warning(suppress:6386) /* i < chunk <= bytes - offset */
				pool[offset + i] = (UCHAR)((offset + i) * 131u);
			}
		}
		offset += chunk;
	}
}

_Function_class_(DRIVER_DISPATCH)
static NTSTATUS
PoolstressDispatch(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	PIO_STACK_LOCATION irpSp;
	NTSTATUS status = STATUS_SUCCESS;
	ULONG_PTR info = 0;
	ULONG code;
	PVOID buf;
	ULONG inLen;
	ULONG outLen;

	if (Irp == NULL)
		return STATUS_INVALID_PARAMETER;

	UNREFERENCED_PARAMETER(DeviceObject);
	irpSp = IoGetCurrentIrpStackLocation(Irp);
	if (irpSp == NULL) {
		status = STATUS_INVALID_PARAMETER;
		goto out;
	}

	if (irpSp->MajorFunction == IRP_MJ_CREATE || irpSp->MajorFunction == IRP_MJ_CLOSE) {
		status = STATUS_SUCCESS;
		goto out;
	}

	if (irpSp->MajorFunction != IRP_MJ_DEVICE_CONTROL) {
		status = STATUS_INVALID_DEVICE_REQUEST;
		goto out;
	}

	code = irpSp->Parameters.DeviceIoControl.IoControlCode;
	buf = Irp->AssociatedIrp.SystemBuffer;
	inLen = irpSp->Parameters.DeviceIoControl.InputBufferLength;
	outLen = irpSp->Parameters.DeviceIoControl.OutputBufferLength;

	if (code == IOCTL_POOLSTRESS_ALLOC) {
		POOLSTRESS_ALLOC_IN *in;
		SIZE_T bytes;
		SIZE_T i;
		UINT8 c1, c2, c3, c4;

		if (outLen != 0) {
			status = STATUS_INVALID_PARAMETER;
			goto out;
		}
		if (inLen < sizeof(POOLSTRESS_ALLOC_IN) || buf == NULL) {
			status = STATUS_INVALID_PARAMETER;
			goto out;
		}
		if (g_Pool != NULL) {
			status = STATUS_DEVICE_BUSY;
			goto out;
		}

		in = (POOLSTRESS_ALLOC_IN *)buf;
		if (in->Bytes < 64 || in->Bytes > 32 * 1024 * 1024) {
			status = STATUS_INVALID_PARAMETER;
			goto out;
		}

		c1 = (UINT8)(in->PoolTag & 0xFF);
		c2 = (UINT8)((in->PoolTag >> 8) & 0xFF);
		c3 = (UINT8)((in->PoolTag >> 16) & 0xFF);
		c4 = (UINT8)((in->PoolTag >> 24) & 0xFF);

		if (c1 < 0x20 || c1 > 0x7E || c2 < 0x20 || c2 > 0x7E ||
		    c3 < 0x20 || c3 > 0x7E || c4 < 0x20 || c4 > 0x7E) {
			status = STATUS_INVALID_PARAMETER;
			goto out;
		}
		if (c1 == c2 || c1 == c3 || c1 == c4 ||
		    c2 == c3 || c2 == c4 || c3 == c4) {
			status = STATUS_INVALID_PARAMETER;
			goto out;
		}

		bytes = (SIZE_T)in->Bytes;
		g_Pool = ExAllocatePool2(POOL_FLAG_PAGED, bytes, in->PoolTag);
		if (!g_Pool) {
			status = STATUS_INSUFFICIENT_RESOURCES;
			goto out;
		}

		g_PoolSize = bytes;
		g_PoolTag = in->PoolTag;

		/* Fill every byte in bounded BCrypt calls (DT-21). */
		PoolstressFill((PUCHAR)g_Pool, bytes);
		/* Touch every page so pages are resident then pageable. */
		for (i = 0; i < bytes / PAGE_SIZE; i++) {
			volatile UCHAR *p = (PUCHAR)g_Pool + i * PAGE_SIZE;
			*p = *p;
		}

		status = STATUS_SUCCESS;
		goto out;
	}

	if (code == IOCTL_POOLSTRESS_READBACK) {
		SIZE_T i;
		volatile UCHAR sum = 0;

		if (outLen != 0) {
			status = STATUS_INVALID_PARAMETER;
			goto out;
		}
		if (!g_Pool) {
			status = STATUS_INVALID_DEVICE_STATE;
			goto out;
		}
		for (i = 0; i < g_PoolSize; i += PAGE_SIZE) {
			sum ^= *((PUCHAR)g_Pool + i);
		}
		(VOID)sum;
		info = 0;
		status = STATUS_SUCCESS;
		goto out;
	}

	if (code == IOCTL_POOLSTRESS_FREE) {
		if (outLen != 0) {
			status = STATUS_INVALID_PARAMETER;
			goto out;
		}
		if (g_Pool) {
			ExFreePoolWithTag(g_Pool, g_PoolTag);
			g_Pool = NULL;
			g_PoolSize = 0;
			g_PoolTag = 0;
		}
		status = STATUS_SUCCESS;
		goto out;
	}

	status = STATUS_INVALID_DEVICE_REQUEST;

out:
	Irp->IoStatus.Status = status;
	Irp->IoStatus.Information = info;
	IoCompleteRequest(Irp, IO_NO_INCREMENT);
	return status;
}

_Function_class_(DRIVER_UNLOAD)
static VOID
PoolstressUnload(_In_ PDRIVER_OBJECT DriverObject)
{
	UNICODE_STRING link;

	UNREFERENCED_PARAMETER(DriverObject);
	if (g_Pool) {
		ExFreePoolWithTag(g_Pool, g_PoolTag);
		g_Pool = NULL;
		g_PoolTag = 0;
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
