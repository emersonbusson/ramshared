pub fn redact(msg: &str) -> String {
    let mut result = String::with_capacity(msg.len());
    let mut i = 0;
    let chars: Vec<char> = msg.chars().collect();

    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '0' && (chars[i+1] == 'x' || chars[i+1] == 'X') {
            result.push_str("<REDACTED>");
            i += 2;
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            continue;
        }

        if chars[i] == '/' {
            let is_mid_word_slash = i > 0 && chars[i-1].is_alphanumeric();
            if !is_mid_word_slash {
                let mut j = i;
                while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '/' || chars[j] == '.' || chars[j] == '-' || chars[j] == '_') {
                    j += 1;
                }
                if j - i > 1 {
                    result.push_str("<REDACTED>");
                    i = j;
                    continue;
                }
            }
        }

        if chars[i].is_ascii_alphabetic() && i + 2 < chars.len() && chars[i+1] == ':' && (chars[i+2] == '\\' || chars[i+2] == '/') {
            let mut j = i + 2;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '\\' || chars[j] == '/' || chars[j] == '.' || chars[j] == '-' || chars[j] == '_') {
                j += 1;
            }
            result.push_str("<REDACTED>");
            i = j;
            continue;
        }

        let is_device = chars[i..].starts_with(&['n','v','m','e'])
            || chars[i..].starts_with(&['s','d'])
            || chars[i..].starts_with(&['v','d'])
            || chars[i..].starts_with(&['h','d'])
            || chars[i..].starts_with(&['x','v','d']);

        if is_device && (i == 0 || !chars[i-1].is_alphanumeric()) {
            let mut j = i;
            while j < chars.len() && chars[j].is_alphanumeric() {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let has_digit = word.chars().any(|c| c.is_ascii_digit());
            let is_short_dev = word.len() == 3 && (word.starts_with("sd") || word.starts_with("vd") || word.starts_with("hd"))
                && (word.ends_with('a') || word.ends_with('b') || word.ends_with('c') || word.ends_with('d') || word.ends_with('e') || word.ends_with('f'));
            let is_xvd = word.len() == 4 && word.starts_with("xvd")
                && (word.ends_with('a') || word.ends_with('b') || word.ends_with('c') || word.ends_with('d') || word.ends_with('e') || word.ends_with('f'));

            if has_digit || is_short_dev || is_xvd {
                result.push_str("<REDACTED>");
                i = j;
                continue;
            }
        }

        result.push(chars[i]);
        i += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_hex_addresses() {
        assert_eq!(redact("Failed at 0x7fffabcd1234!"), "Failed at <REDACTED>!");
        assert_eq!(redact("Address 0X1234ABCD is invalid"), "Address <REDACTED> is invalid");
    }

    #[test]
    fn test_redact_linux_paths() {
        assert_eq!(redact("Error opening /proc/meminfo: permission denied"), "Error opening <REDACTED>: permission denied");
        assert_eq!(redact("File /var/log/syslog not found"), "File <REDACTED> not found");
        assert_eq!(redact("Standalone / should not be redacted"), "Standalone / should not be redacted");
        assert_eq!(redact("n/a"), "n/a");
        assert_eq!(redact("and/or"), "and/or");
    }

    #[test]
    fn test_redact_windows_paths() {
        assert_eq!(redact("Could not read C:\\Users\\Administrator\\secret.txt file"), "Could not read <REDACTED> file");
        assert_eq!(redact("Path D:/data/test is invalid"), "Path <REDACTED> is invalid");
    }

    #[test]
    fn test_redact_devices() {
        assert_eq!(redact("Device nvme0n1 failed"), "Device <REDACTED> failed");
        assert_eq!(redact("Disk sda2 is full"), "Disk <REDACTED> is full");
        assert_eq!(redact("Volume vdb1 attached"), "Volume <REDACTED> attached");
        assert_eq!(redact("Checking hdc"), "Checking <REDACTED>");
        assert_eq!(redact("Drive xvda1 formatted"), "Drive <REDACTED> formatted");
        assert_eq!(redact("The standard is good"), "The standard is good");
        assert_eq!(redact("sdk is a kit"), "sdk is a kit");
        assert_eq!(redact("hdr display"), "hdr display");
    }
}
