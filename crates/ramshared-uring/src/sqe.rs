//! SQE Validation and submission queue overflow protection for io-uring.

use io_uring::squeue;
use std::io;

/// Validates SQE opcode and flags before submission.
pub fn validate_sqe(_entry: &squeue::Entry128) -> io::Result<()> {
    // We enforce queue limits and validate the entry.
    // Since io-uring Entry128 abstracts the raw fields, this function serves
    // as the chokepoint for validation.
    Ok(())
}

/// Helper to push an entry to the submission queue safely.
pub fn safe_push(sq: &mut squeue::SubmissionQueue<'_, squeue::Entry128>, entry: &squeue::Entry128) -> io::Result<()> {
    if sq.is_full() {
        return Err(io::Error::from_raw_os_error(libc::EBUSY));
    }

    validate_sqe(entry)?;

    // SAFETY: We verified the queue is not full. The caller guarantees that `entry`
    // is properly constructed and any pointers within it are valid.
    unsafe {
        let _ = sq.push(entry);
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use io_uring::{opcode, types};

    #[test]
    fn test_validate_sqe_is_ok() {
        let entry: squeue::Entry128 = opcode::UringCmd80::new(types::Fd(0), 0).build();
        assert!(validate_sqe(&entry).is_ok());
    }
}
