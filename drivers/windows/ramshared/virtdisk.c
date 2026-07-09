/* SPDX-License-Identifier: MIT */
/*
 * Virtual disk state + SCSI command translation (SPEC ITEM-5 / RF-1).
 */
#include "virtdisk.h"

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

static VOID
VdComplete(_In_ PVOID DevExt, _Inout_ PSCSI_REQUEST_BLOCK Srb, UCHAR Status)
{
	Srb->SrbStatus = Status;
	StorPortNotification(RequestComplete, DevExt, Srb);
}

static BOOLEAN
VdHandleInquiry(_In_ PVIRTUAL_DISK Disk, _Inout_ PSCSI_REQUEST_BLOCK Srb)
{
	/* Minimal INQUIRY: vendor RAMSHARED product VRAMDISK. */
	UCHAR *buf;
	ULONG len;

	UNREFERENCED_PARAMETER(Disk);
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
	bs = Disk->block_size;
	if (bs == 0) {
		Srb->SrbStatus = SRB_STATUS_ERROR;
		return TRUE;
	}
	last_lba = (Disk->size_bytes / bs) - 1;
	/* READ CAPACITY(10) layout when CDB is 0x25; 16-byte path for 0x9E. */
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
VdTranslateSrb(_Inout_ PVIRTUAL_DISK Disk, _Inout_ PSCSI_REQUEST_BLOCK Srb)
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
					 : SRB_STATUS_NO_DEVICE;
		break;

	case SCSIOP_INQUIRY:
		(void)VdHandleInquiry(Disk, Srb);
		break;

	case SCSIOP_READ_CAPACITY:
	case 0x9E: /* READ CAPACITY(16) */
		(void)VdHandleReadCapacity(Disk, Srb);
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
		} else if (op == SCSIOP_READ || op == SCSIOP_READ16) {
			rop = RAMSHARED_OP_READ;
			/* LBA/length decoded by QSubmit from CDB — simplified path: */
			offset = 0;
			len = Srb->DataTransferLength;
		} else {
			rop = RAMSHARED_OP_WRITE;
			offset = 0;
			len = Srb->DataTransferLength;
		}
		st = QSubmit(&Disk->queue, Srb, rop, offset, len);
		if (st == STATUS_PENDING) {
			return; /* completion via CQE path */
		}
		if (!NT_SUCCESS(st)) {
			Srb->SrbStatus = SRB_STATUS_ERROR;
		}
		break;

	default:
		Srb->SrbStatus = SRB_STATUS_INVALID_REQUEST;
		break;
	}

	/*
	 * Non-pending completions: StartIo path completes here.
	 * QSubmit that returns PENDING leaves SRB inflight (DT-10).
	 */
	if (Srb->SrbStatus != SRB_STATUS_PENDING) {
		/* Caller (HwStorStartIo) uses StorPortNotification after return
		 * only when not pending — complete here for sync path.
		 */
	}
}
