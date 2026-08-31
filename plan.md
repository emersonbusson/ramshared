1. **RULES**:
   - Guard clauses over nested if/else.
   - Physical limits & sanity checks (4096-byte page/sector alignment).
   - Specific & Semantic Kernel Error returns (NTSTATUS codes for Windows driver, `STATUS_INVALID_PARAMETER`).
   - The memory limits compiling on windows drivers on linux host (missing ntddk.h, etc).

2. **MAIN_DIFF**:
```diff
<<<<<<< SEARCH
			offset *= Disk->block_size;
			len = Srb->DataTransferLength;
			if (op == SCSIOP_READ || op == SCSIOP_READ16) {
				rop = RAMSHARED_OP_READ;
			} else {
				rop = RAMSHARED_OP_WRITE;
			}
		}
		st = QSubmit(&Disk->queue, DevExt, Srb, rop, offset, len);
=======
			offset *= Disk->block_size;
			len = Srb->DataTransferLength;

			if ((offset % Disk->block_size) != 0 || (len % Disk->block_size) != 0) {
				Srb->SrbStatus = SRB_STATUS_INVALID_REQUEST;
				break;
			}

			if (op == SCSIOP_READ || op == SCSIOP_READ16) {
				rop = RAMSHARED_OP_READ;
			} else {
				rop = RAMSHARED_OP_WRITE;
			}
		}
		st = QSubmit(&Disk->queue, DevExt, Srb, rop, offset, len);
>>>>>>> REPLACE
```

3. **FILES**:
   - `drivers/windows/ramshared/virtdisk.c`

4. **INVARIANTS**:
   - Virtual disk read/write offsets and lengths are exact multiples of disk sector size (512 or 4096 bytes).

5. **COUNTERFACTUAL**:
   - If not aligned, offset or len might cause out of bounds reading/writing.

6. **RED_TEST**:
   - Unaligned R/W request fails immediately with `SRB_STATUS_INVALID_REQUEST`.

7. **COVERAGE**:
   - Check alignment on read/write SCSI commands inside `VdTranslateSrb`.

8. **REAL_PROOF**:
   - `git diff drivers/windows/ramshared/virtdisk.c` will show the specific check. Cannot compile windows driver on linux host, will rely on standard CI check-kernel scripts excluding it properly.

9. **ROLLBACK**:
   - Revert if valid read/write requests start failing (especially due to alignment).

10. **PR_BOUNDARY**:
    - "do not merge"
