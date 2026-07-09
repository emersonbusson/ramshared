/* SPDX-License-Identifier: MIT */
/*
 * Virtual disk state + SCSI command translation (SPEC ITEM-5 / RF-1 / DT-25).
 */
#include "virtdisk.h"

static VIRTUAL_DISK g_ActiveDisk;
static BOOLEAN g_Active;
static PVOID g_AdapterExt;

VOID
VdSetAdapterExt(_In_opt_ PVOID DeviceExtension)
{
	g_AdapterExt = DeviceExtension;
}

PVOID
VdGetAdapterExt(VOID)
{
	return g_AdapterExt;
}

NTSTATUS
VdCreate(_Out_ PVIRTUAL_DISK Disk, _In_ const RAMSHARED_DISK_PARAMS *Params)
{
	if (Disk == NULL || Params == NULL) {
		return STATUS_INVALID_PARAMETER;
	}
	if (Params->block_size != 512 && Params->block_size != 4096) {
		return STATUS_INVALID_PARAMETER;
	}
	if (Params->size_bytes == 0 ||
	    (Params->size_bytes % Params->block_size) != 0) {
		return STATUS_INVALID_PARAMETER;
	}

	RtlZeroMemory(Disk, sizeof(*Disk));
	Disk->size_bytes = Params->size_bytes;
	Disk->block_size = Params->block_size;
	RtlCopyMemory(Disk->serial, Params->serial, sizeof(Disk->serial));
	InterlockedExchange(&Disk->state, VdStateCreated);
	return STATUS_SUCCESS;
}

VOID
VdDestroy(_Inout_ PVIRTUAL_DISK Disk)
{
	if (Disk == NULL) {
		return;
	}
	InterlockedExchange(&Disk->state, VdStateNone);
	RtlZeroMemory(Disk, sizeof(*Disk));
}

NTSTATUS
VdActivate(_In_ const RAMSHARED_DISK_PARAMS *Params)
{
	NTSTATUS st;

	if (g_Active) {
		return STATUS_DEVICE_BUSY;
	}
	st = VdCreate(&g_ActiveDisk, Params);
	if (!NT_SUCCESS(st)) {
		return st;
	}
	g_Active = TRUE;
	/* Re-enumerate so capacity/media-ready is visible (DT-25, INF path). */
	if (g_AdapterExt != NULL) {
		StorPortNotification(BusChangeDetected, g_AdapterExt, (UCHAR)0);
	}
	return STATUS_SUCCESS;
}

VOID
VdDeactivate(VOID)
{
	if (!g_Active) {
		return;
	}
	QUnregister(&g_ActiveDisk.queue);
	VdDestroy(&g_ActiveDisk);
	g_Active = FALSE;
	if (g_AdapterExt != NULL) {
		StorPortNotification(BusChangeDetected, g_AdapterExt, (UCHAR)0);
	}
}

PVIRTUAL_DISK
VdGetActive(VOID)
{
	return g_Active ? &g_ActiveDisk : NULL;
}

BOOLEAN
VdIsActive(VOID)
{
	return g_Active;
}

static VOID
VdComplete(_In_ PVOID DevExt, _Inout_ PSCSI_REQUEST_BLOCK Srb, UCHAR Status)
{
	Srb->SrbStatus = Status;
	if (DevExt != NULL) {
		StorPortNotification(RequestComplete, DevExt, Srb);
	}
}

static BOOLEAN
VdHandleInquiry(_Inout_ PSCSI_REQUEST_BLOCK Srb)
{
	UCHAR *buf;
	ULONG len;

	buf = (UCHAR *)Srb->DataBuffer;
	if (buf == NULL) {
		Srb->SrbStatus = SRB_STATUS_ERROR;
		return TRUE;
	}
	len = Srb->DataTransferLength;
	if (len < 36) {
		Srb->SrbStatus = SRB_STATUS_DATA_OVERRUN;
		return TRUE;
	}
	RtlZeroMemory(buf, len);
	buf[0] = 0x00; /* direct-access */
	buf[2] = 0x05; /* SPC-3 */
	buf[4] = 31;   /* additional length */
	RtlCopyMemory(&buf[8], "RAMSHARE", 8);
	RtlCopyMemory(&buf[16], "VRAMDISK        ", 16);
	RtlCopyMemory(&buf[32], "0001", 4);
	Srb->DataTransferLength = 36;
	Srb->SrbStatus = SRB_STATUS_SUCCESS;
	return TRUE;
}

static BOOLEAN
VdHandleReadCapacity(_In_ PVIRTUAL_DISK Disk, _Inout_ PSCSI_REQUEST_BLOCK Srb)
{
	UCHAR *buf;
	ULONG64 last_lba;
	ULONG bs;

	buf = (UCHAR *)Srb->DataBuffer;
	if (buf == NULL) {
		Srb->SrbStatus = SRB_STATUS_ERROR;
		return TRUE;
	}
	if (Disk == NULL || Disk->block_size == 0 || Disk->size_bytes == 0) {
		/* Not ready / no media: report 0 capacity. */
		if (Srb->DataTransferLength < 8) {
			Srb->SrbStatus = SRB_STATUS_DATA_OVERRUN;
			return TRUE;
		}
		RtlZeroMemory(buf, 8);
		Srb->DataTransferLength = 8;
		Srb->SrbStatus = SRB_STATUS_SUCCESS;
		return TRUE;
	}
	bs = Disk->block_size;
	last_lba = (Disk->size_bytes / bs) - 1;
	if (Srb->Cdb[0] == SCSIOP_READ_CAPACITY) {
		if (Srb->DataTransferLength < 8) {
			Srb->SrbStatus = SRB_STATUS_DATA_OVERRUN;
			return TRUE;
		}
		buf[0] = (UCHAR)((last_lba >> 24) & 0xFF);
		buf[1] = (UCHAR)((last_lba >> 16) & 0xFF);
		buf[2] = (UCHAR)((last_lba >> 8) & 0xFF);
		buf[3] = (UCHAR)(last_lba & 0xFF);
		buf[4] = (UCHAR)((bs >> 24) & 0xFF);
		buf[5] = (UCHAR)((bs >> 16) & 0xFF);
		buf[6] = (UCHAR)((bs >> 8) & 0xFF);
		buf[7] = (UCHAR)(bs & 0xFF);
		Srb->DataTransferLength = 8;
	}
	Srb->SrbStatus = SRB_STATUS_SUCCESS;
	return TRUE;
}

VOID
VdTranslateSrbNoDisk(_In_ PVOID DevExt, _Inout_ PSCSI_REQUEST_BLOCK Srb)
{
	UCHAR op = Srb->Cdb[0];

	switch (op) {
	case SCSIOP_INQUIRY:
		(void)VdHandleInquiry(Srb);
		break;
	case SCSIOP_TEST_UNIT_READY:
		/* Not ready until CREATE_DISK. */
		Srb->SrbStatus = SRB_STATUS_BUSY;
		break;
	case SCSIOP_READ_CAPACITY:
	case 0x9E:
		(void)VdHandleReadCapacity(NULL, Srb);
		break;
	case SCSIOP_MODE_SENSE:
	case SCSIOP_MODE_SENSE10:
		Srb->SrbStatus = SRB_STATUS_SUCCESS;
		if (Srb->DataBuffer && Srb->DataTransferLength >= 4) {
			RtlZeroMemory(Srb->DataBuffer, Srb->DataTransferLength);
		}
		break;
	default:
		Srb->SrbStatus = SRB_STATUS_NO_DEVICE;
		break;
	}
	if (Srb->SrbStatus != SRB_STATUS_PENDING) {
		VdComplete(DevExt, Srb, Srb->SrbStatus);
	}
}

VOID
VdTranslateSrb(
	_Inout_ PVIRTUAL_DISK Disk,
	_In_ PVOID DevExt,
	_Inout_ PSCSI_REQUEST_BLOCK Srb)
{
	UCHAR op = Srb->Cdb[0];
	NTSTATUS st;
	UINT64 offset;
	UINT32 len;
	enum ramshared_op rop;

	switch (op) {
	case SCSIOP_TEST_UNIT_READY:
		Srb->SrbStatus = (InterlockedCompareExchange(&Disk->state, 0, 0) >=
				  VdStateCreated)
					 ? SRB_STATUS_SUCCESS
					 : SRB_STATUS_BUSY;
		break;

	case SCSIOP_INQUIRY:
		(void)VdHandleInquiry(Srb);
		break;

	case SCSIOP_READ_CAPACITY:
	case 0x9E: /* READ CAPACITY(16) */
		(void)VdHandleReadCapacity(Disk, Srb);
		break;

	case SCSIOP_MODE_SENSE:
	case SCSIOP_MODE_SENSE10:
		Srb->SrbStatus = SRB_STATUS_SUCCESS;
		if (Srb->DataBuffer && Srb->DataTransferLength >= 4) {
			RtlZeroMemory(Srb->DataBuffer, Srb->DataTransferLength);
		}
		break;

	case SCSIOP_READ:
	case SCSIOP_READ16:
	case SCSIOP_WRITE:
	case SCSIOP_WRITE16:
	case SCSIOP_SYNCHRONIZE_CACHE:
	case 0x91: /* SYNCHRONIZE CACHE(16) */
		if (op == SCSIOP_SYNCHRONIZE_CACHE || op == 0x91) {
			rop = RAMSHARED_OP_FLUSH;
			offset = 0;
			len = 0;
		} else {
			/* Parse LBA from CDB (10-byte and 16-byte forms). */
			if (op == SCSIOP_READ16 || op == SCSIOP_WRITE16) {
				offset = ((UINT64)Srb->Cdb[2] << 56) |
					 ((UINT64)Srb->Cdb[3] << 48) |
					 ((UINT64)Srb->Cdb[4] << 40) |
					 ((UINT64)Srb->Cdb[5] << 32) |
					 ((UINT64)Srb->Cdb[6] << 24) |
					 ((UINT64)Srb->Cdb[7] << 16) |
					 ((UINT64)Srb->Cdb[8] << 8) |
					 ((UINT64)Srb->Cdb[9]);
			} else {
				offset = ((UINT64)Srb->Cdb[2] << 24) |
					 ((UINT64)Srb->Cdb[3] << 16) |
					 ((UINT64)Srb->Cdb[4] << 8) |
					 ((UINT64)Srb->Cdb[5]);
			}
			offset *= Disk->block_size;
			len = Srb->DataTransferLength;
			if (op == SCSIOP_READ || op == SCSIOP_READ16) {
				rop = RAMSHARED_OP_READ;
			} else {
				rop = RAMSHARED_OP_WRITE;
			}
		}
		st = QSubmit(&Disk->queue, DevExt, Srb, rop, offset, len);
		if (st == STATUS_PENDING) {
			Srb->SrbStatus = SRB_STATUS_PENDING;
			return;
		}
		if (!NT_SUCCESS(st)) {
			Srb->SrbStatus = SRB_STATUS_ERROR;
		} else {
			Srb->SrbStatus = SRB_STATUS_SUCCESS;
		}
		break;

	default:
		Srb->SrbStatus = SRB_STATUS_INVALID_REQUEST;
		break;
	}

	if (Srb->SrbStatus != SRB_STATUS_PENDING) {
		VdComplete(DevExt, Srb, Srb->SrbStatus);
	}
}
