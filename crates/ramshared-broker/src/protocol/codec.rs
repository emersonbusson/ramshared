use std::io::{BufRead, Read, Write};
use super::msg::Msg;

/// Anti-DoS line cap (64 KiB) — `read_msg` never allocates beyond this.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Protocol errors from I/O or codec failure.
#[derive(Debug)]
pub enum ProtocolError {
    BadMagic(String),
    UnsupportedVersion(u32),
    PayloadTooLarge,
    ConnectionClosed(std::io::Error),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic(s) => write!(f, "bad magic: {s}"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported version: {v}"),
            Self::PayloadTooLarge => write!(f, "payload too large (exceeds MAX_LINE_BYTES)"),
            Self::ConnectionClosed(e) => write!(f, "connection closed: {e}"),
        }
    }
}

impl std::error::Error for ProtocolError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ConnectionClosed(e) => Some(e),
            _ => None,
        }
    }
}

/// Serializes `msg` + `'\n'` and flushes (one message per line).
pub fn write_msg<W: Write>(w: &mut W, msg: &Msg) -> Result<(), ProtocolError> {
    let mut line = serde_json::to_vec(msg).map_err(|e| ProtocolError::BadMagic(e.to_string()))?;
    line.push(b'\n');
    w.write_all(&line)
        .map_err(ProtocolError::ConnectionClosed)?;
    w.flush().map_err(ProtocolError::ConnectionClosed)
}

/// Reads a line (up to [`MAX_LINE_BYTES`]) and deserializes it.
///
/// `Ok(None)` on clean EOF; `Err` on giant line, invalid JSON or unknown shape.
/// `take(MAX_LINE_BYTES + 1)` ensures we never read/allocate beyond the cap (anti-DoS).
pub fn read_msg<R: BufRead>(r: &mut R) -> Result<Option<Msg>, ProtocolError> {
    let mut buf = Vec::new();
    let n = r
        .by_ref()
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', &mut buf)
        .map_err(ProtocolError::ConnectionClosed)?;
    if n == 0 {
        return Ok(None); // clean EOF
    }
    let had_newline = buf.last() == Some(&b'\n');
    if !had_newline && buf.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::PayloadTooLarge);
    }
    let line = buf.strip_suffix(b"\n").unwrap_or(&buf);
    let msg =
        serde_json::from_slice::<Msg>(line).map_err(|e| ProtocolError::BadMagic(e.to_string()))?;
    Ok(Some(msg))
}
