/* SPDX-License-Identifier: MIT */
/*
 * Virtual disk state + SCSI command translation (SPEC ITEM-5 / RF-1 / DT-25).
 */
#include "virtdisk.h"

static VIRTUAL_DISK g_ActiveDisk;
static BOOLEAN g_Active;
static PVOID g_AdapterExt;

static BOOLEAN
VdIsAsciiHexSerial(_In_reads_(16) PCUCHAR serial)
{
	UCHAR i;

	if (serial == NULL)
		return FALSE;
	for (i = 0; i < 16; i++) {
		UCHAR c = serial[i];

		if (!((c >= '0' && c <= '9') ||
		      (c >= 'A' && c <= 'F'))) {
			return FALSE;
		}
	}
	return TRUE;
}

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
	if (Params->reserved != 0) {
		return STATUS_INVALID_PARAMETER; /* REFUSE_RESERVED_DISK_PARAMS */
	}
	if (Params->block_size != 512 && Params->block_size != 4096) {
		return STATUS_INVALID_PARAMETER;
	}
	if (Params->size_bytes == 0 ||
	    (Params->size_bytes % Params->block_size) != 0) {
		return STATUS_INVALID_PARAMETER;
	}
	if (!VdIsAsciiHexSerial(Params->serial))
		return STATUS_INVALID_PARAMETER;

	RtlZeroMemory(Disk, sizeof(*Disk));
	Disk->size_bytes = Params->size_bytes;
	Disk->block_size = Params->block_size;
	RtlCopyMemory(Disk->serial, Params->serial, sizeof(Disk->serial));
	ObReferenceObject(IoGetCurrentProcess());
	Disk->OwnerProcess = IoGetCurrentProcess();
	InterlockedExchange(&Disk->state, VdStateCreated);
	return STATUS_SUCCESS;
}

VOID
VdDestroy(_Inout_ PVIRTUAL_DISK Disk)
{
	if (Disk == NULL) {
		return;
	}
	if (Disk->OwnerProcess) {
		ObDereferenceObject(Disk->OwnerProcess);
		Disk->OwnerProcess = NULL;
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

BOOLEAN
VdOwnerMatches(_In_ PEPROCESS Process)
{
	if (!g_Active || Process == NULL) {
		return FALSE;
	}
	return g_ActiveDisk.OwnerProcess == Process;
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

/*
 * Task Manager / class driver poll TEST UNIT READY aggressively. Returning
 * SRB_STATUS_BUSY makes StorPort requeue forever -> "% Disk Time" stuck at
 * 100% with 0 B/s and 0 ms (no real transfer counters). Use CHECK CONDITION
 * NOT READY with autosense so the stack backs off cleanly.
 *
 * Sense: SK=NOT_READY (0x02), ASC=LOGICAL UNIT NOT READY (0x04),
 * ASCQ=INITIALIZING COMMAND REQUIRED (0x02).
 */
static VOID
VdSetSenseNotReady(_Inout_ PSCSI_REQUEST_BLOCK Srb)
{
	UCHAR *sense;
	ULONG senseLen;

	Srb->ScsiStatus = SCSISTAT_CHECK_CONDITION;
	sense = (UCHAR *)Srb->SenseInfoBuffer;
	senseLen = Srb->SenseInfoBufferLength;
	if (sense != NULL && senseLen >= 18) {
		RtlZeroMemory(sense, senseLen);
		/* Fixed format sense data (response code 0x70). */
		sense[0] = 0x70;
		sense[2] = 0x02 /* NOT READY */; /* 0x02 */
		sense[7] = 10; /* additional sense length */
		sense[12] = 0x04 /* LUN NOT READY */; /* 0x04 */
		sense[13] = 0x02; /* INITIALIZING COMMAND REQUIRED */
		Srb->SrbStatus = (UCHAR)(SRB_STATUS_ERROR | SRB_STATUS_AUTOSENSE_VALID);
	} else {
		/* No sense buffer: still fail closed without BUSY thrash. */
		Srb->SrbStatus = SRB_STATUS_ERROR;
	}
}

/*
 * Standard INQUIRY + VPD 0x00 / 0x80 (unit serial from 16-byte disk serial).
 * CDB[1] EVPD bit, CDB[2] page code (DT-5 / RF-4 / VPD_SERIAL_MATCH).
 */
static BOOLEAN
VdHandleInquiry(_In_opt_ PVIRTUAL_DISK Disk, _Inout_ PSCSI_REQUEST_BLOCK Srb)
{
	UCHAR response[36];
	ULONG responseLen;
	ULONG transferLen;
	ULONG allocationLen;
	UCHAR evpd;
	UCHAR page;

	evpd = Srb->Cdb[1] & 0x01;
	page = Srb->Cdb[2];
	allocationLen = Srb->Cdb[4];
	RtlZeroMemory(response, sizeof(response));

	if (evpd == 0) {
		response[0] = 0x00; /* direct-access */
		response[2] = 0x05; /* SPC-3 */
		response[4] = 31;   /* additional length */
		RtlCopyMemory(&response[8], "RAMSHARE", 8);
		RtlCopyMemory(&response[16], "VRAMDISK        ", 16);
		RtlCopyMemory(&response[32], "0001", 4);
		responseLen = 36;
		goto copy_response;
	}

	/* VPD pages */
	if (page == 0x00) {
		/* Supported VPD pages: 0x00, 0x80 */
		response[0] = 0x00;
		response[1] = 0x00;
		response[3] = 2;
		response[4] = 0x00;
		response[5] = 0x80;
		responseLen = 6;
		goto copy_response;
	}
	if (page == 0x80) {
		/* Unit serial number: 16 ASCII hex digits from disk serial. */
		if (Disk == NULL) {
			Srb->SrbStatus = SRB_STATUS_INVALID_REQUEST;
			return TRUE;
		}
		response[0] = 0x00;
		response[1] = 0x80;
		response[3] = 16;
		RtlCopyMemory(&response[4], Disk->serial, 16);
		responseLen = 20;
		goto copy_response;
	}

	Srb->SrbStatus = SRB_STATUS_INVALID_REQUEST;
	return TRUE;

copy_response:
	transferLen = min(Srb->DataTransferLength, allocationLen);
	transferLen = min(transferLen, responseLen);
	if (transferLen != 0 && Srb->DataBuffer == NULL) {
		Srb->SrbStatus = SRB_STATUS_ERROR;
		return TRUE;
	}
	if (transferLen != 0)
		RtlCopyMemory(Srb->DataBuffer, response, transferLen);
	Srb->DataTransferLength = transferLen;
	Srb->SrbStatus = SRB_STATUS_SUCCESS;
	return TRUE;
}

static VOID
VdHandleReportLuns(_Inout_ PSCSI_REQUEST_BLOCK Srb, _In_ BOOLEAN Present)
{
	ULONG required = Present ? 16 : 8;
	UCHAR *buf;

	if (Srb->DataBuffer == NULL || Srb->DataTransferLength < required) {
		Srb->SrbStatus = SRB_STATUS_DATA_OVERRUN;
		return;
	}
	buf = (UCHAR *)Srb->DataBuffer;
	RtlZeroMemory(buf, Srb->DataTransferLength);
	if (Present) {
		/* LUN list length = 8 (one 8-byte LUN entry for LUN 0). */
		buf[3] = 8;
	}
	Srb->DataTransferLength = required;
	Srb->SrbStatus = SRB_STATUS_SUCCESS;
}

static BOOLEAN
VdHandleReadCapacity(_In_ PVIRTUAL_DISK Disk, _Inout_ PSCSI_REQUEST_BLOCK Srb)
{
	UCHAR response[32];
	ULONG64 last_lba;
	ULONG allocationLen;
	ULONG transferLen;
	ULONG bs;
	UCHAR i;

	if (Disk == NULL || Disk->block_size == 0 || Disk->size_bytes == 0) {
		Srb->SrbStatus = SRB_STATUS_NO_DEVICE;
		return TRUE;
	}
	bs = Disk->block_size;
	last_lba = (Disk->size_bytes / bs) - 1;
	RtlZeroMemory(response, sizeof(response));
	if (Srb->Cdb[0] == SCSIOP_READ_CAPACITY) {
		if (Srb->DataBuffer == NULL || Srb->DataTransferLength < 8) {
			Srb->SrbStatus = SRB_STATUS_DATA_OVERRUN;
			return TRUE;
		}
		if (last_lba > MAXULONG)
			last_lba = MAXULONG;
		response[0] = (UCHAR)((last_lba >> 24) & 0xFF);
		response[1] = (UCHAR)((last_lba >> 16) & 0xFF);
		response[2] = (UCHAR)((last_lba >> 8) & 0xFF);
		response[3] = (UCHAR)(last_lba & 0xFF);
		response[4] = (UCHAR)((bs >> 24) & 0xFF);
		response[5] = (UCHAR)((bs >> 16) & 0xFF);
		response[6] = (UCHAR)((bs >> 8) & 0xFF);
		response[7] = (UCHAR)(bs & 0xFF);
		RtlCopyMemory(Srb->DataBuffer, response, 8);
		Srb->DataTransferLength = 8;
		Srb->SrbStatus = SRB_STATUS_SUCCESS;
		return TRUE;
	}
	if ((Srb->Cdb[1] & 0x1F) != 0x10) {
		Srb->SrbStatus = SRB_STATUS_INVALID_REQUEST;
		return TRUE;
	}
	for (i = 0; i < 8; i++)
		response[i] = (UCHAR)((last_lba >> ((7 - i) * 8)) & 0xFF);
	response[8] = (UCHAR)((bs >> 24) & 0xFF);
	response[9] = (UCHAR)((bs >> 16) & 0xFF);
	response[10] = (UCHAR)((bs >> 8) & 0xFF);
	response[11] = (UCHAR)(bs & 0xFF);
	allocationLen = ((ULONG)Srb->Cdb[10] << 24) |
			((ULONG)Srb->Cdb[11] << 16) |
			((ULONG)Srb->Cdb[12] << 8) |
			(ULONG)Srb->Cdb[13];
	transferLen = min(Srb->DataTransferLength, allocationLen);
	transferLen = min(transferLen, (ULONG)sizeof(response));
	if (transferLen != 0 && Srb->DataBuffer == NULL) {
		Srb->SrbStatus = SRB_STATUS_ERROR;
		return TRUE;
	}
	if (transferLen != 0)
		RtlCopyMemory(Srb->DataBuffer, response, transferLen);
	Srb->DataTransferLength = transferLen;
	Srb->SrbStatus = SRB_STATUS_SUCCESS;
	return TRUE;
}

VOID
VdTranslateSrbNoDisk(_In_ PVOID DevExt, _Inout_ PSCSI_REQUEST_BLOCK Srb)
{
	UCHAR op = Srb->Cdb[0];

	switch (op) {
	case SCSIOP_INQUIRY:
		/* No child PDO before CREATE: avoid caching placeholder VPD identity. */
		Srb->SrbStatus = SRB_STATUS_NO_DEVICE;
		break;
	case SCSIOP_TEST_UNIT_READY:
		/*
		 * Not ready until CREATE_DISK — never SRB_STATUS_BUSY (TM 100%).
		 * Sense NOT_READY lets the stack back off without thrashing.
		 */
		VdSetSenseNotReady(Srb);
		break;
	case SCSIOP_READ_CAPACITY:
	case 0x9E:
		/* Stale no-media request: remain not-present, never synthesize size. */
		Srb->SrbStatus = SRB_STATUS_NO_DEVICE;
		break;
	case SCSIOP_MODE_SENSE:
	case SCSIOP_MODE_SENSE10:
		Srb->SrbStatus = SRB_STATUS_SUCCESS;
		if (Srb->DataBuffer && Srb->DataTransferLength >= 4) {
			RtlZeroMemory(Srb->DataBuffer, Srb->DataTransferLength);
		}
		break;
	case 0xA0: /* REPORT LUNS — no LUN until CREATE_DISK */
		VdHandleReportLuns(Srb, FALSE);
		break;
	default:
		Srb->SrbStatus = SRB_STATUS_INVALID_REQUEST;
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
		if (InterlockedCompareExchange(&Disk->state, 0, 0) >=
		    VdStateCreated) {
			Srb->SrbStatus = SRB_STATUS_SUCCESS;
		} else {
			VdSetSenseNotReady(Srb);
		}
		break;

	case SCSIOP_INQUIRY:
		(void)VdHandleInquiry(Disk, Srb);
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
		/* B2: backend gone / queue torn down - fail fast, do not hang. */
		if (InterlockedCompareExchange(&Disk->state, 0, 0) ==
		    (LONG)VdStateFailed) {
			Srb->SrbStatus = SRB_STATUS_ERROR;
			break;
		}
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

	case 0xA0: /* REPORT LUNS */
		VdHandleReportLuns(Srb, TRUE);
		break;

	default:
		Srb->SrbStatus = SRB_STATUS_INVALID_REQUEST;
		break;
	}

	if (Srb->SrbStatus != SRB_STATUS_PENDING) {
		VdComplete(DevExt, Srb, Srb->SrbStatus);
	}
}
