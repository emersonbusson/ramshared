import sys

def apply_patch():
    with open('crates/ramshared-broker/src/protocol.rs', 'r') as f:
        content = f.read()

    old_code = '''    let had_newline = buf.last() == Some(&b'\\n');
    if !had_newline {
        if buf.len() > MAX_LINE_BYTES {
            // Discard the remainder of the oversized line to resynchronize the parser on the next message.
            let mut discard_buf = Vec::new();
            // We ignore errors on discard; if connection dropped, next read_msg will handle it.
            let _ = r.read_until(b'\\n', &mut discard_buf);
            return Err(ProtocolError::PayloadTooLarge);
        } else {
            // Line was truncated before hitting a newline and before hitting MAX_LINE_BYTES.
            return Err(ProtocolError::ConnectionClosed(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated message",
            )));
        }
    } else if buf.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::PayloadTooLarge);
    }'''

    new_code = '''    let had_newline = buf.last() == Some(&b'\\n');
    if !had_newline {
        if buf.len() > MAX_LINE_BYTES {
            // Discard the remainder of the oversized line to resynchronize the parser on the next message
            // without unbounded allocations to prevent memory exhaustion (DoS).
            let mut sink = [0u8; 1024];
            loop {
                match r.read(&mut sink) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        if sink[..n].contains(&b'\\n') {
                            // If we read a chunk containing a newline, we have to reset the reader to the exact
                            // position after the newline so we don't discard part of the next message.
                            // But `read` advances the cursor. Using `BufRead::fill_buf` and `consume` is safer.
                            break; // This logic needs adjustment below, let's use BufRead directly
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            return Err(ProtocolError::PayloadTooLarge);
        } else {
            // Line was truncated before hitting a newline and before hitting MAX_LINE_BYTES.
            return Err(ProtocolError::ConnectionClosed(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated message",
            )));
        }
    } else if buf.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::PayloadTooLarge);
    }'''

    # We will use BufRead::fill_buf instead of read
    safer_code = '''    let had_newline = buf.last() == Some(&b'\\n');
    if !had_newline {
        if buf.len() > MAX_LINE_BYTES {
            // Discard the remainder of the oversized line to resynchronize the parser on the next message
            // without unbounded allocations to prevent memory exhaustion (DoS).
            loop {
                let available = match r.fill_buf() {
                    Ok(b) if b.is_empty() => break,
                    Ok(b) => b,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };

                if let Some(pos) = available.iter().position(|&b| b == b'\\n') {
                    r.consume(pos + 1);
                    break;
                } else {
                    let len = available.len();
                    r.consume(len);
                }
            }
            return Err(ProtocolError::PayloadTooLarge);
        } else {
            // Line was truncated before hitting a newline and before hitting MAX_LINE_BYTES.
            return Err(ProtocolError::ConnectionClosed(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "truncated message",
            )));
        }
    } else if buf.len() > MAX_LINE_BYTES {
        return Err(ProtocolError::PayloadTooLarge);
    }'''

    content = content.replace(old_code, safer_code)

    with open('crates/ramshared-broker/src/protocol.rs', 'w') as f:
        f.write(content)

apply_patch()
