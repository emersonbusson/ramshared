/* SPDX-License-Identifier: MIT */
/*
 * Rings / doorbell / inflight / crash containment.
 * SPEC ITEM-5 / DT-2 / DT-4 / DT-10 / DT-18 / DT-22.
 */
#include "queue.h"

static BOOLEAN
IsPowerOfTwo(UINT32 v)
{
	return v != 0 && (v & (v - 1)) == 0;
}

static VOID
QUnlockAll(_Inout_ PRAMSHARED_QUEUE Q)
{
	if (Q->SqMdl) {
		MmUnlockPages(Q->SqMdl);
		IoFreeMdl(Q->SqMdl);
		Q->SqMdl = NULL;
		Q->Sq = NULL;
	}
	if (Q->CqMdl) {
		MmUnlockPages(Q->CqMdl);
		IoFreeMdl(Q->CqMdl);
		Q->CqMdl = NULL;
		Q->Cq = NULL;
	}
	if (Q->DataMdl) {
		MmUnlockPages(Q->DataMdl);
		IoFreeMdl(Q->DataMdl);
		Q->DataMdl = NULL;
		Q->Data = NULL;
	}
	if (Q->SqEvent) {
		ObDereferenceObject(Q->SqEvent);
		Q->SqEvent = NULL;
	}
	if (Q->CqEvent) {
		ObDereferenceObject(Q->CqEvent);
		Q->CqEvent = NULL;
	}
	Q->Registered = FALSE;
}

NTSTATUS
QRegister(
	_Inout_ PRAMSHARED_QUEUE Q,
	_In_ const RAMSHARED_REGISTER *Reg,
	_In_ KPROCESSOR_MODE AccessMode)
{
	NTSTATUS status;
	SIZE_T sq_bytes, cq_bytes;

	/* DT-18: validate everything BEFORE MmProbeAndLockPages. */
	if (Reg == NULL) {
		return STATUS_INVALID_PARAMETER;
	}
	if (Reg->abi_version != RAMSHARED_ABI_VERSION) {
		return STATUS_REVISION_MISMATCH;
	}
	if (!IsPowerOfTwo(Reg->queue_depth) || Reg->queue_depth > RAMSHARED_MAX_QD) {
		return STATUS_INVALID_PARAMETER;
	}
	if (Reg->block_size != 512 && Reg->block_size != 4096) {
		return STATUS_INVALID_PARAMETER;
	}
	if (Reg->max_io_bytes == 0 || Reg->max_io_bytes > RAMSHARED_MAX_IO) {
		return STATUS_INVALID_PARAMETER;
	}
	if (Reg->sq_ring_va == 0 || Reg->cq_ring_va == 0 || Reg->data_area_va == 0) {
		return STATUS_INVALID_PARAMETER;
	}
	if (Reg->data_area_len < (UINT64)Reg->queue_depth * Reg->max_io_bytes) {
		return STATUS_INVALID_PARAMETER;
	}
	if (Q->Registered) {
		return STATUS_DEVICE_BUSY;
	}

	RtlZeroMemory(Q->Inflight, sizeof(Q->Inflight));
	KeInitializeSpinLock(&Q->Lock);
	Q->QueueDepth = Reg->queue_depth;
	Q->MaxIoBytes = Reg->max_io_bytes;
	Q->BlockSize = Reg->block_size;
	Q->PendedFetch = NULL;

	sq_bytes = sizeof(RAMSHARED_RING_HDR) +
		   (SIZE_T)Reg->queue_depth * sizeof(RAMSHARED_SQE);
	cq_bytes = sizeof(RAMSHARED_RING_HDR) +
		   (SIZE_T)Reg->queue_depth * sizeof(RAMSHARED_CQE);

	/* Map SQ */
	Q->SqMdl = IoAllocateMdl((PVOID)(ULONG_PTR)Reg->sq_ring_va, (ULONG)sq_bytes, FALSE, FALSE, NULL);
	if (!Q->SqMdl) {
		return STATUS_INSUFFICIENT_RESOURCES;
	}
	__try {
		MmProbeAndLockPages(Q->SqMdl, AccessMode, IoModifyAccess);
	} __except (EXCEPTION_EXECUTE_HANDLER) {
		IoFreeMdl(Q->SqMdl);
		Q->SqMdl = NULL;
		return STATUS_INVALID_PARAMETER;
	}
	Q->Sq = (PRAMSHARED_RING_HDR)MmGetSystemAddressForMdlSafe(Q->SqMdl, NormalPagePriority);
	if (!Q->Sq) {
		status = STATUS_INSUFFICIENT_RESOURCES;
		goto out_err;
	}

	/* Map CQ */
	Q->CqMdl = IoAllocateMdl((PVOID)(ULONG_PTR)Reg->cq_ring_va, (ULONG)cq_bytes, FALSE, FALSE, NULL);
	if (!Q->CqMdl) {
		status = STATUS_INSUFFICIENT_RESOURCES;
		goto out_err;
	}
	__try {
		MmProbeAndLockPages(Q->CqMdl, AccessMode, IoModifyAccess);
	} __except (EXCEPTION_EXECUTE_HANDLER) {
		status = STATUS_INVALID_PARAMETER;
		goto out_err;
	}
	Q->Cq = (PRAMSHARED_RING_HDR)MmGetSystemAddressForMdlSafe(Q->CqMdl, NormalPagePriority);
	if (!Q->Cq) {
		status = STATUS_INSUFFICIENT_RESOURCES;
		goto out_err;
	}

	/* Map data area */
	Q->DataMdl = IoAllocateMdl((PVOID)(ULONG_PTR)Reg->data_area_va,
				   (ULONG)Reg->data_area_len, FALSE, FALSE, NULL);
	if (!Q->DataMdl) {
		status = STATUS_INSUFFICIENT_RESOURCES;
		goto out_err;
	}
	__try {
		MmProbeAndLockPages(Q->DataMdl, AccessMode, IoModifyAccess);
	} __except (EXCEPTION_EXECUTE_HANDLER) {
		status = STATUS_INVALID_PARAMETER;
		goto out_err;
	}
	Q->Data = (PUCHAR)MmGetSystemAddressForMdlSafe(Q->DataMdl, NormalPagePriority);
	if (!Q->Data) {
		status = STATUS_INSUFFICIENT_RESOURCES;
		goto out_err;
	}

	/* Auxiliary events (DT-22) — optional handles. */
	if (Reg->sq_event_handle) {
		status = ObReferenceObjectByHandle(
			(HANDLE)(ULONG_PTR)Reg->sq_event_handle,
			EVENT_MODIFY_STATE, *ExEventObjectType, AccessMode,
			(PVOID *)&Q->SqEvent, NULL);
		if (!NT_SUCCESS(status)) {
			goto out_err;
		}
	}
	if (Reg->cq_event_handle) {
		status = ObReferenceObjectByHandle(
			(HANDLE)(ULONG_PTR)Reg->cq_event_handle,
			EVENT_MODIFY_STATE, *ExEventObjectType, AccessMode,
			(PVOID *)&Q->CqEvent, NULL);
		if (!NT_SUCCESS(status)) {
			goto out_err;
		}
	}

	Q->Registered = TRUE;
	return STATUS_SUCCESS;

out_err:
	QUnlockAll(Q);
	return status;
}

VOID
QUnregister(_Inout_ PRAMSHARED_QUEUE Q)
{
	QTeardownOnCrash(Q);
}

NTSTATUS
QSubmit(
	_Inout_ PRAMSHARED_QUEUE Q,
	_In_ PSCSI_REQUEST_BLOCK Srb,
	_In_ enum ramshared_op Op,
	_In_ UINT64 Offset,
	_In_ UINT32 Len)
{
	KIRQL old;
	UINT32 tag;
	UINT32 slot;
	PRAMSHARED_SQE sqe;
	UINT32 idx;
	PVOID sys_addr;

	if (!Q->Registered || Q->Sq == NULL) {
		return STATUS_DEVICE_NOT_CONNECTED;
	}

	KeAcquireSpinLock(&Q->Lock, &old);

	/* Find free inflight slot = tag. */
	tag = RAMSHARED_MAX_QD;
	for (slot = 0; slot < Q->QueueDepth; slot++) {
		if (!Q->Inflight[slot].InUse) {
			tag = slot;
			break;
		}
	}
	if (tag == RAMSHARED_MAX_QD) {
		KeReleaseSpinLock(&Q->Lock, old);
		return STATUS_INSUFFICIENT_RESOURCES;
	}

	/* Bounce WRITE into data slot (DT-4 / DT-23). */
	if (Op == RAMSHARED_OP_WRITE && Len > 0) {
		sys_addr = Srb->DataBuffer;
		if (sys_addr == NULL) {
			KeReleaseSpinLock(&Q->Lock, old);
			return STATUS_INVALID_PARAMETER;
		}
		if (Len > Q->MaxIoBytes) {
			KeReleaseSpinLock(&Q->Lock, old);
			return STATUS_INVALID_PARAMETER;
		}
		RtlCopyMemory(Q->Data + (SIZE_T)tag * Q->MaxIoBytes, sys_addr, Len);
	}

	Q->Inflight[tag].Srb = Srb;
	Q->Inflight[tag].Op = Op;
	Q->Inflight[tag].BufSlot = tag;
	Q->Inflight[tag].InUse = TRUE;

	idx = Q->Sq->tail & (Q->QueueDepth - 1);
	sqe = (PRAMSHARED_SQE)((PUCHAR)Q->Sq + sizeof(RAMSHARED_RING_HDR)) + idx;
	sqe->tag = tag;
	sqe->op = (UINT32)Op;
	sqe->flags = 0;
	sqe->offset = Offset;
	sqe->len = Len;
	sqe->buf_slot = tag;
	KeMemoryBarrier();
	Q->Sq->tail = Q->Sq->tail + 1;

	if (Q->SqEvent) {
		KeSetEvent(Q->SqEvent, IO_NO_INCREMENT, FALSE);
	}

	/* Wake primary: complete pended COMMIT_AND_FETCH IRP if any. */
	if (Q->PendedFetch) {
		PIRP irp = Q->PendedFetch;

		Q->PendedFetch = NULL;
		KeReleaseSpinLock(&Q->Lock, old);
		irp->IoStatus.Status = STATUS_SUCCESS;
		irp->IoStatus.Information = 0;
		IoCompleteRequest(irp, IO_NO_INCREMENT);
		return STATUS_PENDING;
	}

	KeReleaseSpinLock(&Q->Lock, old);
	return STATUS_PENDING;
}

NTSTATUS
QCommitAndFetch(_Inout_ PRAMSHARED_QUEUE Q, _In_ PIRP Irp)
{
	KIRQL old;
	UINT32 completed = 0;
	PIO_STACK_LOCATION irpSp;

	if (!Q->Registered || Q->Cq == NULL) {
		return STATUS_DEVICE_NOT_CONNECTED;
	}

	irpSp = IoGetCurrentIrpStackLocation(Irp);
	UNREFERENCED_PARAMETER(irpSp);

	KeAcquireSpinLock(&Q->Lock, &old);

	/* Drain CQ with bounds checks (DT-18). */
	while (Q->Cq->head != Q->Cq->tail) {
		UINT32 idx = Q->Cq->head & (Q->QueueDepth - 1);
		PRAMSHARED_CQE cqe =
			(PRAMSHARED_CQE)((PUCHAR)Q->Cq + sizeof(RAMSHARED_RING_HDR)) + idx;
		UINT64 tag = cqe->tag;
		PSCSI_REQUEST_BLOCK srb;
		PVOID sys_addr;

		if (tag >= Q->QueueDepth || !Q->Inflight[tag].InUse) {
			/* Unknown/duplicate tag — skip, never double-complete. */
			Q->Cq->head = Q->Cq->head + 1;
			continue;
		}

		srb = Q->Inflight[tag].Srb;
		if (Q->Inflight[tag].Op == RAMSHARED_OP_READ && cqe->status == RAMSHARED_ST_OK) {
			sys_addr = srb->DataBuffer;
			if (sys_addr) {
				RtlCopyMemory(sys_addr,
					      Q->Data + (SIZE_T)Q->Inflight[tag].BufSlot * Q->MaxIoBytes,
					      srb->DataTransferLength);
			}
		}

		if (cqe->status == RAMSHARED_ST_OK) {
			srb->SrbStatus = SRB_STATUS_SUCCESS;
		} else {
			srb->SrbStatus = SRB_STATUS_ERROR;
		}
		Q->Inflight[tag].InUse = FALSE;
		Q->Inflight[tag].Srb = NULL;
		Q->Cq->head = Q->Cq->head + 1;
		completed++;

		/* Complete SRB outside lock ideally; StorPort allows at DISPATCH. */
		StorPortNotification(RequestComplete, srb->OriginalRequest /* placeholder */, srb);
	}

	if (completed == 0 && Q->Sq && Q->Sq->head == Q->Sq->tail) {
		/* No work and empty SQ — pend IRP (primary wake, DT-22). */
		IoMarkIrpPending(Irp);
		Q->PendedFetch = Irp;
		KeReleaseSpinLock(&Q->Lock, old);
		return STATUS_PENDING;
	}

	KeReleaseSpinLock(&Q->Lock, old);
	Irp->IoStatus.Status = STATUS_SUCCESS;
	Irp->IoStatus.Information = completed;
	return STATUS_SUCCESS;
}

VOID
QTeardownOnCrash(_Inout_ PRAMSHARED_QUEUE Q)
{
	KIRQL old;
	UINT32 i;
	PIRP pending;

	KeAcquireSpinLock(&Q->Lock, &old);
	pending = Q->PendedFetch;
	Q->PendedFetch = NULL;

	for (i = 0; i < RAMSHARED_MAX_QD; i++) {
		if (Q->Inflight[i].InUse && Q->Inflight[i].Srb) {
			PSCSI_REQUEST_BLOCK srb = Q->Inflight[i].Srb;

			srb->SrbStatus = SRB_STATUS_ERROR;
			/* STATUS_DEVICE_NOT_CONNECTED analog for storage stack. */
			StorPortNotification(RequestComplete, NULL, srb);
			Q->Inflight[i].InUse = FALSE;
			Q->Inflight[i].Srb = NULL;
		}
	}
	KeReleaseSpinLock(&Q->Lock, old);

	if (pending) {
		pending->IoStatus.Status = STATUS_DEVICE_NOT_CONNECTED;
		pending->IoStatus.Information = 0;
		IoCompleteRequest(pending, IO_NO_INCREMENT);
	}

	QUnlockAll(Q);
}
