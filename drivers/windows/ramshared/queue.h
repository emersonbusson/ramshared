/* SPDX-License-Identifier: MIT */
/*
 * SPSC rings, inflight table, MDL lock/map, crash containment (DT-2/10/18).
 * SPEC windows-storport-cuda-vram DT-5 / DT-6: owner, rundown, slot states.
 *
 * Lock order: I/O cancel spin lock -> RAMSHARED_QUEUE.Lock
 * Never acquire cancel spin lock while holding RAMSHARED_QUEUE.Lock.
 * MmProbeAndLockPages / StorPortNotification / IoCompleteRequest: outside Lock.
 * PASSIVE-only for REGISTER/teardown; DISPATCH-safe bookkeeping uses nonpaged state.
 */
#pragma once

#include <ntddk.h>
#include <storport.h>
#include "protocol.h"

typedef enum _RAMSHARED_SLOT_STATE {
	RamSlotFree = 0,
	RamSlotReserved,
	RamSlotSubmitted,
	RamSlotCompleting,
} RAMSHARED_SLOT_STATE;

typedef enum _RAMSHARED_Q_STATE {
	RamQIdle = 0,
	RamQRegistered,
	RamQClosing,
	RamQFailed,
} RAMSHARED_Q_STATE;

typedef struct _RAMSHARED_INFLIGHT {
	PSCSI_REQUEST_BLOCK Srb;
	enum ramshared_op Op;
	UINT32 BufSlot;
	RAMSHARED_SLOT_STATE State;
} RAMSHARED_INFLIGHT, *PRAMSHARED_INFLIGHT;

typedef struct _RAMSHARED_QUEUE {
	PMDL SqMdl;
	PMDL CqMdl;
	PMDL DataMdl;
	PRAMSHARED_RING_HDR Sq;
	PRAMSHARED_RING_HDR Cq;
	PUCHAR Data;
	PKEVENT SqEvent;
	PKEVENT CqEvent;
	RAMSHARED_INFLIGHT Inflight[RAMSHARED_MAX_QD];
	KSPIN_LOCK Lock;
	PIRP PendedFetch;
	UINT32 QueueDepth;
	UINT32 MaxIoBytes;
	UINT32 BlockSize;
	BOOLEAN Registered;
	/* DT-5: process that registered the queue; balanced ObReference/Dereference. */
	PEPROCESS OwnerProcess;
	/* DT-6: rundown protects mapped ring/data access outside Lock. */
	EX_RUNDOWN_REF IoRundown;
	volatile LONG QState; /* RAMSHARED_Q_STATE */
} RAMSHARED_QUEUE, *PRAMSHARED_QUEUE;

NTSTATUS
QRegister(
	_Inout_ PRAMSHARED_QUEUE Q,
	_In_ const RAMSHARED_REGISTER *Reg,
	_In_ KPROCESSOR_MODE AccessMode,
	_In_ PEPROCESS ExpectedOwner);

VOID QUnregister(_Inout_ PRAMSHARED_QUEUE Q);

NTSTATUS
QSubmit(
	_Inout_ PRAMSHARED_QUEUE Q,
	_In_ PVOID DevExt,
	_In_ PSCSI_REQUEST_BLOCK Srb,
	_In_ enum ramshared_op Op,
	_In_ UINT64 Offset,
	_In_ UINT32 Len);

NTSTATUS
QCommitAndFetch(_Inout_ PRAMSHARED_QUEUE Q, _In_ PIRP Irp);

/* DT-10: complete all inflight SRBs with error; wait rundown; unlock MDLs. */
VOID QTeardownOnCrash(_Inout_ PRAMSHARED_QUEUE Q);

/* Owner check for IOCTL paths (DT-5). */
BOOLEAN QOwnerMatches(_In_ PRAMSHARED_QUEUE Q, _In_ PEPROCESS Process);
