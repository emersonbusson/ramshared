import sys

def apply_patch():
    with open('crates/ramshared-broker/src/protocol.rs', 'r') as f:
        content = f.read()

    old_code = '''    let had_newline = buf.last() == Some(&b'\\n');
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
                };'''

    safer_code = '''    let had_newline = buf.last() == Some(&b'\\n');
    if !had_newline {
        if buf.len() > MAX_LINE_BYTES {
            // Discard the remainder of the oversized line to resynchronize the parser on the next message
            // without unbounded allocations to prevent memory exhaustion (DoS).
            loop {
                let available = match r.fill_buf() {
                    Ok([]) => break,
                    Ok(b) => b,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };'''

    content = content.replace(old_code, safer_code)

    with open('crates/ramshared-broker/src/protocol.rs', 'w') as f:
        f.write(content)

apply_patch()
