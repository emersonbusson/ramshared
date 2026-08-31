1. RULES
- Guard Clauses over nested if/else logic.
- Physical Limits & Sanity Checks: Strictly validate buffer lengths (both input and output) before payload processing.
- Specific & Semantic Kernel Error Returns: Use STATUS_INVALID_PARAMETER for buffer size mismatches.
- English only for all source code, comments, identifiers, commits, and PR descriptions.
- The PR description must be formatted precisely with the required Portuguese section headers (## Resumo, etc.).

2. MAIN_DIFF
```diff
<<<<<<< SEARCH
static NTSTATUS
CtlDispatchDeviceControl(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	PIO_STACK_LOCATION irpSp;
	ULONG code;
	ULONG inLen;
	PVOID buf;
	NTSTATUS status = STATUS_INVALID_DEVICE_REQUEST;
	ULONG_PTR info = 0;
=======
static NTSTATUS
CtlDispatchDeviceControl(_In_ PDEVICE_OBJECT DeviceObject, _Inout_ PIRP Irp)
{
	PIO_STACK_LOCATION irpSp;
	ULONG code;
	ULONG inLen;
	ULONG outLen;
	PVOID buf;
	NTSTATUS status = STATUS_INVALID_DEVICE_REQUEST;
	ULONG_PTR info = 0;
>>>>>>> REPLACE
<<<<<<< SEARCH
	irpSp = IoGetCurrentIrpStackLocation(Irp);
	code = irpSp->Parameters.DeviceIoControl.IoControlCode;
	inLen = irpSp->Parameters.DeviceIoControl.InputBufferLength;
	buf = Irp->AssociatedIrp.SystemBuffer;

	switch (code) {
	case IOCTL_RAMSHARED_REGISTER_QUEUE:
		if (inLen != sizeof(RAMSHARED_REGISTER) || buf == NULL) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
=======
	irpSp = IoGetCurrentIrpStackLocation(Irp);
	code = irpSp->Parameters.DeviceIoControl.IoControlCode;
	inLen = irpSp->Parameters.DeviceIoControl.InputBufferLength;
	outLen = irpSp->Parameters.DeviceIoControl.OutputBufferLength;
	buf = Irp->AssociatedIrp.SystemBuffer;

	switch (code) {
	case IOCTL_RAMSHARED_REGISTER_QUEUE:
		if (inLen != sizeof(RAMSHARED_REGISTER) || outLen != 0 || buf == NULL) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
>>>>>>> REPLACE
<<<<<<< SEARCH
	case IOCTL_RAMSHARED_UNREGISTER_QUEUE:
		/* Zero-input IOCTL: reject non-zero input length (DT-5). */
		if (inLen != 0) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
=======
	case IOCTL_RAMSHARED_UNREGISTER_QUEUE:
		/* Zero-input IOCTL: reject non-zero input length (DT-5). */
		if (inLen != 0 || outLen != 0) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
>>>>>>> REPLACE
<<<<<<< SEARCH
	case IOCTL_RAMSHARED_COMMIT_AND_FETCH:
		if (inLen != 0) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
=======
	case IOCTL_RAMSHARED_COMMIT_AND_FETCH:
		if (inLen != 0 || outLen != 0) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
>>>>>>> REPLACE
<<<<<<< SEARCH
	case IOCTL_RAMSHARED_CREATE_DISK:
		if (inLen != sizeof(RAMSHARED_DISK_PARAMS) || buf == NULL) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
=======
	case IOCTL_RAMSHARED_CREATE_DISK:
		if (inLen != sizeof(RAMSHARED_DISK_PARAMS) || outLen != 0 || buf == NULL) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
>>>>>>> REPLACE
<<<<<<< SEARCH
	case IOCTL_RAMSHARED_DESTROY_DISK:
		if (inLen != 0) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
=======
	case IOCTL_RAMSHARED_DESTROY_DISK:
		if (inLen != 0 || outLen != 0) {
			status = STATUS_INVALID_PARAMETER;
			break;
		}
>>>>>>> REPLACE
```

3. FILES
- `drivers/windows/ramshared/control.c`

4. INVARIANTS
- Input and output buffer lengths must be strictly checked against their expected expected struct sizes before payload processing, preventing information disclosure or uninitialized memory copies.

5. COUNTERFACTUAL
- If output buffer sizes are not validated, a userspace process could pass a large OutputBufferLength for `METHOD_BUFFERED` IOCTLs (like `COMMIT_AND_FETCH`). When the driver returns `Information` (e.g. `completed` count), the I/O manager could interpret `Information` as a byte count and erroneously copy uninitialized kernel memory from `SystemBuffer` to the user's output buffer, causing an information leak or a system bugcheck.

6. RED_TEST
- A userspace application issues `IOCTL_RAMSHARED_COMMIT_AND_FETCH` with a valid empty input but a non-zero `OutputBufferLength`, attempting to trigger a kernel memory disclosure when the returned `Information` count is interpreted as copied bytes.

7. COVERAGE
- All five Device Control IOCTLs in `CtlDispatchDeviceControl` are modified to validate `OutputBufferLength` (enforcing `outLen == 0` since none return struct payloads via the buffer).

8. REAL_PROOF
- We will execute compilation tests using `./scripts/ci/check-kernel-build-matrix.sh` and other CI scripts.

9. ROLLBACK
- Trigger: If legitimate userspace clients relied on passing non-zero, padded output buffers and now experience STATUS_INVALID_PARAMETER failures preventing valid device control interactions.

10. PR_BOUNDARY
- The pull request will be opened strictly against `jules/inbox` and remain unmerged.
