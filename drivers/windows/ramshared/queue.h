/* SPDX-License-Identifier: MIT */
/*
 * SPSC rings, inflight table, MDL lock/map, crash containment (DT-2/10/18).
 */
#pragma once

#include <ntddk.h>
#include <storport.h>
#include "protocol.h"

typedef struct _RAMSHARED_INFLIGHT {
	PSCSI_REQUEST_BLOCK Srb;
	enum ramshared_op Op;
	UINT32 BufSlot;
	BOOLEAN InUse;
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
} RAMSHARED_QUEUE, *PRAMSHARED_QUEUE;

NTSTATUS
QRegister(
	_Inout_ PRAMSHARED_QUEUE Q,
	_In_ const RAMSHARED_REGISTER *Reg,
	_In_ KPROCESSOR_MODE AccessMode);

VOID QUnregister(_Inout_ PRAMSHARED_QUEUE Q);

NTSTATUS
QSubmit(
	_Inout_ PRAMSHARED_QUEUE Q,
	_In_ PSCSI_REQUEST_BLOCK Srb,
	_In_ enum ramshared_op Op,
	_In_ UINT64 Offset,
	_In_ UINT32 Len);

NTSTATUS
QCommitAndFetch(_Inout_ PRAMSHARED_QUEUE Q, _In_ PIRP Irp);

/* DT-10: complete all inflight SRBs with error; unlock MDLs. */
VOID QTeardownOnCrash(_Inout_ PRAMSHARED_QUEUE Q);
