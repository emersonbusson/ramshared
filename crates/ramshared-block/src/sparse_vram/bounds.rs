use crate::IoError;

/// Validates that a logical I/O access falls entirely within the advertised capacity.
pub fn check_logical_bounds(off: u64, len: usize, capacity: u64, is_write: bool) -> Result<(), IoError> {
    let end = off.checked_add(len as u64).ok_or_else(|| {
        IoError(format!(
            "sparse {} overflow off={} len={}",
            if is_write { "write" } else { "read" },
            off, len
        ))
    })?;
    if end > capacity {
        return Err(IoError(format!(
            "sparse {} oob off={} len={} cap={}",
            if is_write { "write" } else { "read" },
            off, len, capacity
        )));
    }
    Ok(())
}

/// Validates that a physical DMA access falls entirely within the allocated GPU chunk memory.
pub fn check_physical_bounds(rel: usize, n: usize, physical_len: usize, is_write: bool) -> Result<(), IoError> {
    let end = rel.checked_add(n).ok_or_else(|| {
        IoError(format!(
            "sparse physical {} overflow rel={} n={}",
            if is_write { "write" } else { "read" },
            rel, n
        ))
    })?;
    if end > physical_len {
        return Err(IoError(format!(
            "sparse physical {} oob rel={} n={} physical_len={}",
            if is_write { "write" } else { "read" },
            rel, n, physical_len
        )));
    }
    Ok(())
}
