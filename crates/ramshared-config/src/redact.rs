pub(crate) fn redact(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut current_word = String::new();

    for c in text.chars() {
        if c.is_whitespace() {
            if !current_word.is_empty() {
                result.push_str(&process_word(&current_word));
                current_word.clear();
            }
            result.push(c);
        } else {
            current_word.push(c);
        }
    }
    if !current_word.is_empty() {
        result.push_str(&process_word(&current_word));
    }
    result
}

fn process_word(word: &str) -> String {
    let mut start = 0;
    let mut end = word.len();
    let bytes = word.as_bytes();

    while start < end && is_punctuation_byte(bytes[start]) {
        start += 1;
    }
    while end > start && is_punctuation_byte(bytes[end - 1]) {
        end -= 1;
    }

    if start >= end {
        return word.to_string();
    }

    let core = &word[start..end];
    if should_redact(core) {
        let mut redacted = String::new();
        redacted.push_str(&word[..start]);
        redacted.push_str("[REDACTED]");
        redacted.push_str(&word[end..]);
        redacted
    } else {
        word.to_string()
    }
}

fn is_punctuation_byte(b: u8) -> bool {
    matches!(b, b'\'' | b'"' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';' | b'`' | b'.')
}

fn should_redact(core: &str) -> bool {
    if core.starts_with("0x") && core.len() > 2 && core[2..].chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }

    if core.starts_with('/') && core.len() > 1 {
        return true;
    }

    if core.len() >= 3 {
        let b = core.as_bytes();
        if b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/') {
            return true;
        }
    }

    let parts: Vec<&str> = core.split('-').collect();
    if parts.len() == 5 && parts[0].len() == 8 && parts[1].len() == 4 && parts[2].len() == 4 && parts[3].len() == 4 && parts[4].len() == 12
        && core.chars().filter(|c| *c != '-').all(|c| c.is_ascii_hexdigit()) {
            return true;
        }

    let pci_parts: Vec<&str> = core.split(&[':', '.'][..]).collect();
    if pci_parts.len() == 4 && pci_parts[0].len() == 4 && pci_parts[1].len() == 2 && pci_parts[2].len() == 2 && pci_parts[3].len() == 1
        && core.chars().filter(|c| *c != ':' && *c != '.').all(|c| c.is_ascii_hexdigit()) {
            return true;
        }

    false
}
