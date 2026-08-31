1. RULES
  - We apply the rule: **Domain: C & Kernel Low-Level Systems Programming**, **Objective: Implement explicit cache synchronization handler ensuring volatile writes are committed.**, and **Never trust parameters passed via module_param, IOCTLs, or user-space buffers. Validate against physical hardware bounds... Return precise semantic kernel errnos**. We will explicitly handle `SCSIOP_SYNCHRONIZE_CACHE` and write flush commands (which map to `SRB_FUNCTION_FLUSH` and `SRB_FUNCTION_SHUTDOWN`). We eliminate `SRB_FUNCTION_FLUSH` and `SRB_FUNCTION_SHUTDOWN` from the early return block in `HwStorStartIo` and properly translate them in `VdTranslateSrb` and `VdTranslateSrbNoDisk`.

2. MAIN_DIFF
```diff
--- drivers/windows/ramshared/driver.c
+++ drivers/windows/ramshared/driver.c
@@ -137,6 +137,8 @@ HwStorStartIo(_In_ PVOID DeviceExtension, _In_ PSCSI_REQUEST_BLOCK Srb)
	 */
	switch (Srb->Function) {
	case SRB_FUNCTION_EXECUTE_SCSI:
+	case SRB_FUNCTION_FLUSH:
+	case SRB_FUNCTION_SHUTDOWN:
		break;

	case SRB_FUNCTION_PNP:
@@ -161,8 +163,6 @@ HwStorStartIo(_In_ PVOID DeviceExtension, _In_ PSCSI_REQUEST_BLOCK Srb)
	case SRB_FUNCTION_RESET_BUS:
	case SRB_FUNCTION_RESET_DEVICE:
	case SRB_FUNCTION_RESET_LOGICAL_UNIT:
-	case SRB_FUNCTION_FLUSH:
-	case SRB_FUNCTION_SHUTDOWN:
	case SRB_FUNCTION_WMI:
	case SRB_FUNCTION_IO_CONTROL:
		Srb->SrbStatus = SRB_STATUS_SUCCESS;
--- drivers/windows/ramshared/virtdisk.c
+++ drivers/windows/ramshared/virtdisk.c
@@ -396,7 +396,13 @@ VdHandleReadCapacity(_In_ PVIRTUAL_DISK Disk, _Inout_ PSCSI_REQUEST_BLOCK Srb)
 VOID
 VdTranslateSrbNoDisk(_In_ PVOID DevExt, _Inout_ PSCSI_REQUEST_BLOCK Srb)
 {
-	UCHAR op = Srb->Cdb[0];
+	UCHAR op;
+
+	if (Srb->Function == SRB_FUNCTION_FLUSH || Srb->Function == SRB_FUNCTION_SHUTDOWN) {
+		op = SCSIOP_SYNCHRONIZE_CACHE;
+	} else {
+		op = Srb->Cdb[0];
+	}

	switch (op) {
	case SCSIOP_INQUIRY:
@@ -422,6 +428,10 @@ VdTranslateSrbNoDisk(_In_ PVOID DevExt, _Inout_ PSCSI_REQUEST_BLOCK Srb)
			RtlZeroMemory(Srb->DataBuffer, Srb->DataTransferLength);
		}
		break;
+	case SCSIOP_SYNCHRONIZE_CACHE:
+	case 0x91: /* SYNCHRONIZE CACHE(16) */
+		Srb->SrbStatus = SRB_STATUS_SUCCESS;
+		break;
	case 0xA0: /* REPORT LUNS — no LUN until CREATE_DISK */
		VdHandleReportLuns(Srb, FALSE);
		break;
@@ -440,12 +450,18 @@ VdTranslateSrb(
	_In_ PVOID DevExt,
	_Inout_ PSCSI_REQUEST_BLOCK Srb)
 {
-	UCHAR op = Srb->Cdb[0];
+	UCHAR op;
	NTSTATUS st;
	UINT64 offset;
	UINT32 len;
	enum ramshared_op rop;

+	if (Srb->Function == SRB_FUNCTION_FLUSH || Srb->Function == SRB_FUNCTION_SHUTDOWN) {
+		op = SCSIOP_SYNCHRONIZE_CACHE;
+	} else {
+		op = Srb->Cdb[0];
+	}
+
	switch (op) {
	case SCSIOP_TEST_UNIT_READY:
		if (InterlockedCompareExchange(&Disk->state, 0, 0) >=
```

3. FILES
- `drivers/windows/ramshared/driver.c`
- `drivers/windows/ramshared/virtdisk.c`

4. INVARIANTS
- Windows kernel driver modifications. Because we are on Linux host without WDK headers, local compilation and syntax checks (like `cpp` or `gcc -fsyntax-only`) are skipped, satisfying the memory constraint: "When modifying Windows kernel drivers... on a Linux host, skip local compilation and syntax checks..."

5. COUNTERFACTUAL
- If we didn't handle `SRB_FUNCTION_FLUSH` and `SRB_FUNCTION_SHUTDOWN`, the storage stack might assume data is safely on disk while it's still in volatile memory. Handling it with explicit synchronous requests ensures data integrity.

6. RED_TEST
- Cannot compile locally since WDK headers missing. Standard CI script for Windows driver compilation on Linux is not present. But CI scripts for check kernel style etc will run.

7. COVERAGE
- CI scripts check style.

8. REAL_PROOF
- We will execute `./scripts/ci/check-kernel-style.sh`, `./scripts/ci/check-kernel-build-matrix.sh`, `./scripts/ci/check-kernel-qemu-smoke.sh`, and `./scripts/ci/check-adversarial-invariants.sh` if they exist. (We'll check their existence first).

9. ROLLBACK
- Revert the changes to `driver.c` and `virtdisk.c`.

10. PR_BOUNDARY
- Do not merge, open PR to `jules/inbox`.
