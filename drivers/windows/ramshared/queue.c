/* SPDX-License-Identifier: MIT */
/*
 * Rings / doorbell / inflight / crash containment.
 * SPEC ITEM-5 / DT-2 / DT-4 / DT-10 / DT-18 / DT-22.
 */
#include "queue.h"
#include "virtdisk.h"

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

/* Cap single MDL map: 4 MiB avoids system-PTE pressure on small lab VMs. */
#define RAMSHARED_MAX_DATA_MDL (4u * 1024u * 1024u)

static BOOLEAN
QIsUserVa(_In_ ULONG_PTR Va, _In_ SIZE_T Len)
{
	ULONG_PTR end;

	if (Va == 0 || Len == 0) {
		return FALSE;
	}
	if (Va > (ULONG_PTR)MmHighestUserAddress) {
		return FALSE;
	}
	end = Va + Len - 1;
	if (end < Va) {
		return FALSE; /* overflow */
	}
	if (end > (ULONG_PTR)MmHighestUserAddress) {
		return FALSE;
	}
	return TRUE;
}

static NTSTATUS
QMapUserRegion(
	_Out_ PMDL *OutMdl,
	_Out_ PVOID *OutVa,
	_In_ ULONG_PTR UserVa,
	_In_ SIZE_T Len,
	_In_ KPROCESSOR_MODE AccessMode)
{
	PMDL mdl;
	PVOID mapped;

	*OutMdl = NULL;
	*OutVa = NULL;

	if (Len == 0 || Len > (SIZE_T)MAXULONG) {
		return STATUS_INVALID_PARAMETER;
	}
	if (!QIsUserVa(UserVa, Len)) {
		return STATUS_INVALID_PARAMETER;
	}

	mdl = IoAllocateMdl((PVOID)UserVa, (ULONG)Len, FALSE, FALSE, NULL);
	if (!mdl) {
		return STATUS_INSUFFICIENT_RESOURCES;
	}

	__try {
		/* Always probe as UserMode: ring/data VAs are process user addresses. */
		MmProbeAndLockPages(mdl,
				    AccessMode == KernelMode ? UserMode : AccessMode,
				    IoModifyAccess);
	} __except (EXCEPTION_EXECUTE_HANDLER) {
		IoFreeMdl(mdl);
		return STATUS_INVALID_PARAMETER;
	}

	mapped = MmGetSystemAddressForMdlSafe(
		mdl, (NormalPagePriority | MdlMappingNoExecute));
	if (!mapped) {
		MmUnlockPages(mdl);
		IoFreeMdl(mdl);
		return STATUS_INSUFFICIENT_RESOURCES;
	}

	*OutMdl = mdl;
	*OutVa = mapped;
	return STATUS_SUCCESS;
}

NTSTATUS
QRegister(
	_Inout_ PRAMSHARED_QUEUE Q,
	_In_ const RAMSHARED_REGISTER *Reg,
	_In_ KPROCESSOR_MODE AccessMode)
{
	NTSTATUS status;
	SIZE_T sq_bytes, cq_bytes, data_len;
	PVOID mapped;

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
	/* Hard cap for lab safety (BSOD 0x3B AV_ramshared!QRegister on 32MiB map). */
	if (Reg->data_area_len > RAMSHARED_MAX_DATA_MDL) {
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
	data_len = (SIZE_T)Reg->data_area_len;

	status = QMapUserRegion(&Q->SqMdl, &mapped,
				(ULONG_PTR)Reg->sq_ring_va, sq_bytes, AccessMode);
	if (!NT_SUCCESS(status)) {
		goto out_err;
	}
	Q->Sq = (PRAMSHARED_RING_HDR)mapped;

	status = QMapUserRegion(&Q->CqMdl, &mapped,
				(ULONG_PTR)Reg->cq_ring_va, cq_bytes, AccessMode);
	if (!NT_SUCCESS(status)) {
		goto out_err;
	}
	Q->Cq = (PRAMSHARED_RING_HDR)mapped;

	status = QMapUserRegion(&Q->DataMdl, &mapped,
				(ULONG_PTR)Reg->data_area_va, data_len, AccessMode);
	if (!NT_SUCCESS(status)) {
		goto out_err;
	}
	Q->Data = (PUCHAR)mapped;

	/* Auxiliary events (DT-22) — optional handles. */
	if (Reg->sq_event_handle) {
		status = ObReferenceObjectByHandle(
			(HANDLE)(ULONG_PTR)Reg->sq_event_handle,
			EVENT_MODIFY_STATE, *ExEventObjectType, UserMode,
			(PVOID *)&Q->SqEvent, NULL);
		if (!NT_SUCCESS(status)) {
			goto out_err;
		}
	}
	if (Reg->cq_event_handle) {
		status = ObReferenceObjectByHandle(
			(HANDLE)(ULONG_PTR)Reg->cq_event_handle,
			EVENT_MODIFY_STATE, *ExEventObjectType, UserMode,
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
	_In_ PVOID DevExt,
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
	ULONG st;

	if (!Q->Registered || Q->Sq == NULL) {
		return STATUS_DEVICE_NOT_CONNECTED;
	}
	if (Len > Q->MaxIoBytes) {
		return STATUS_INVALID_PARAMETER;
	}

	/*
	 * DT-23: MapBuffers is NON_READ_WRITE — DataBuffer is invalid for R/W.
	 * StorPortGetSystemAddress before spinlock (may be heavy).
	 */
	sys_addr = NULL;
	if ((Op == RAMSHARED_OP_WRITE || Op == RAMSHARED_OP_READ) && Len > 0) {
		st = StorPortGetSystemAddress(DevExt, Srb, &sys_addr);
		if (st != STOR_STATUS_SUCCESS || sys_addr == NULL) {
			return STATUS_INVALID_PARAMETER;
		}
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

	/* Bounce WRITE into data slot (DT-4). */
	if (Op == RAMSHARED_OP_WRITE && Len > 0 && sys_addr != NULL) {
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
		PDRIVER_CANCEL oldc;

		Q->PendedFetch = NULL;
		KeReleaseSpinLock(&Q->Lock, old);
		oldc = IoSetCancelRoutine(irp, NULL);
		if (oldc != NULL) {
			irp->IoStatus.Status = STATUS_SUCCESS;
			irp->IoStatus.Information = 0;
			IoCompleteRequest(irp, IO_NO_INCREMENT);
		}
		return STATUS_PENDING;
	}

	KeReleaseSpinLock(&Q->Lock, old);
	return STATUS_PENDING;
}

/*
 * Cancel pended COMMIT_AND_FETCH so userspace CancelIo / handle close
 * does not leave the next IOCTL stuck (lab hang after empty-SQ poll).
 */
static VOID
QCommitCancel(_Inout_ PDEVICE_OBJECT DeviceObject, _Inout_ _IRQL_uses_cancel_ PIRP Irp)
{
	PRAMSHARED_QUEUE q;
	KIRQL old;
	BOOLEAN ours = FALSE;

	UNREFERENCED_PARAMETER(DeviceObject);

	/* Irp->Tail.Overlay.DriverContext[0] holds queue ptr (set on pend). */
	q = (PRAMSHARED_QUEUE)Irp->Tail.Overlay.DriverContext[0];
	if (q != NULL) {
		KeAcquireSpinLock(&q->Lock, &old);
		if (q->PendedFetch == Irp) {
			q->PendedFetch = NULL;
			ours = TRUE;
		}
		KeReleaseSpinLock(&q->Lock, old);
	}

	IoReleaseCancelSpinLock(Irp->CancelIrql);

	if (ours) {
		Irp->IoStatus.Status = STATUS_CANCELLED;
		Irp->IoStatus.Information = 0;
		IoCompleteRequest(Irp, IO_NO_INCREMENT);
	}
}

NTSTATUS
QCommitAndFetch(_Inout_ PRAMSHARED_QUEUE Q, _In_ PIRP Irp)
{
	KIRQL old;
	UINT32 completed = 0;
	PDRIVER_CANCEL oldCancel;

	if (!Q->Registered || Q->Cq == NULL) {
		return STATUS_DEVICE_NOT_CONNECTED;
	}

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
			PVOID adext = VdGetAdapterExt();
			ULONG gst;

			sys_addr = NULL;
			if (adext != NULL) {
				gst = StorPortGetSystemAddress(adext, srb, &sys_addr);
				if (gst == STOR_STATUS_SUCCESS && sys_addr != NULL) {
					RtlCopyMemory(sys_addr,
						      Q->Data + (SIZE_T)Q->Inflight[tag].BufSlot *
								  Q->MaxIoBytes,
						      srb->DataTransferLength);
				}
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

		/* Adapter extension from HwStorInitialize (DT-25). */
		{
			PVOID adext = VdGetAdapterExt();

			if (adext != NULL) {
				StorPortNotification(RequestComplete, adext, srb);
			}
		}
	}

	if (completed == 0 && Q->Sq && Q->Sq->head == Q->Sq->tail) {
		/* No work and empty SQ — pend IRP (primary wake, DT-22). */
		if (Q->PendedFetch != NULL) {
			/* Only one pended fetch at a time. */
			KeReleaseSpinLock(&Q->Lock, old);
			return STATUS_DEVICE_BUSY;
		}
		IoMarkIrpPending(Irp);
		Irp->Tail.Overlay.DriverContext[0] = Q;
		Q->PendedFetch = Irp;
		KeReleaseSpinLock(&Q->Lock, old);

		oldCancel = IoSetCancelRoutine(Irp, QCommitCancel);
		if (Irp->Cancel) {
			oldCancel = IoSetCancelRoutine(Irp, NULL);
			if (oldCancel != NULL) {
				/* We still own completion. */
				KeAcquireSpinLock(&Q->Lock, &old);
				if (Q->PendedFetch == Irp) {
					Q->PendedFetch = NULL;
				}
				KeReleaseSpinLock(&Q->Lock, old);
				Irp->IoStatus.Status = STATUS_CANCELLED;
				Irp->IoStatus.Information = 0;
				IoCompleteRequest(Irp, IO_NO_INCREMENT);
			}
			return STATUS_PENDING;
		}
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
	UINT32 nfail;
	PIRP pending;
	PVOID adext;
	PSCSI_REQUEST_BLOCK failed[RAMSHARED_MAX_QD];

	/*
	 * DT-10 / B2: complete inflight SRBs with error so the storage stack does
	 * not hang waiting for a dead userspace backend.
	 *
	 * Discipline:
	 * - Registered=FALSE first so new QSubmit fails fast (STATUS_DEVICE_NOT_CONNECTED).
	 * - Snapshot SRB pointers under the lock; RequestComplete **outside** the lock
	 *   (StorPort may re-enter StartIo — deadlock if we hold Q->Lock).
	 * - Never pass NULL DeviceExtension (ignored → hang). Use VdGetAdapterExt().
	 * - Do not complete the same SRB twice (clear InUse before RequestComplete).
	 */
	adext = VdGetAdapterExt();
	nfail = 0;
	RtlZeroMemory(failed, sizeof(failed));

	KeAcquireSpinLock(&Q->Lock, &old);
	pending = Q->PendedFetch;
	Q->PendedFetch = NULL;
	Q->Registered = FALSE;

	for (i = 0; i < RAMSHARED_MAX_QD; i++) {
		if (Q->Inflight[i].InUse && Q->Inflight[i].Srb) {
			failed[nfail++] = Q->Inflight[i].Srb;
			Q->Inflight[i].InUse = FALSE;
			Q->Inflight[i].Srb = NULL;
		}
	}
	KeReleaseSpinLock(&Q->Lock, old);

	for (i = 0; i < nfail; i++) {
		if (failed[i] == NULL) {
			continue;
		}
		failed[i]->SrbStatus = SRB_STATUS_ERROR;
		if (adext != NULL) {
			StorPortNotification(RequestComplete, adext, failed[i]);
		}
	}

	if (pending) {
		if (IoSetCancelRoutine(pending, NULL) != NULL) {
			pending->IoStatus.Status = STATUS_DEVICE_NOT_CONNECTED;
			pending->IoStatus.Information = 0;
			IoCompleteRequest(pending, IO_NO_INCREMENT);
		}
	}

	QUnlockAll(Q);
}
