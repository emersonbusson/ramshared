pub fn redact(msg: &str) -> String {
    let mut result = String::with_capacity(msg.len());
    let mut i = 0;
    let chars: Vec<char> = msg.chars().collect();

    while i < chars.len() {
        // Redact Hex Addresses (0x...)
        if i + 1 < chars.len() && chars[i] == '0' && (chars[i+1] == 'x' || chars[i+1] == 'X') {
            result.push_str("<REDACTED>");
            i += 2;
            while i < chars.len() && chars[i].is_ascii_hexdigit() {
                i += 1;
            }
            continue;
        }

        // Paths starting with '/' (Linux absolute paths, devices, etc.)
        if chars[i] == '/' {
            let mut j = i;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '/' || chars[j] == '.' || chars[j] == '-' || chars[j] == '_') {
                j += 1;
            }
            // Require at least 2 chars length to be considered a path to avoid redacting standalone slashes
            if j - i > 1 {
                result.push_str("<REDACTED>");
                i = j;
                continue;
            }
        }

        // Paths starting with 'C:\' or similar
        if chars[i].is_ascii_alphabetic() && i + 2 < chars.len() && chars[i+1] == ':' && (chars[i+2] == '\\' || chars[i+2] == '/') {
            let mut j = i + 2;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '\\' || chars[j] == '/' || chars[j] == '.' || chars[j] == '-' || chars[j] == '_') {
                j += 1;
            }
            result.push_str("<REDACTED>");
            i = j;
            continue;
        }

        // Device identifiers: nvme, sd, vd, hd, xvd followed by alphanumeric characters
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
            // check: starts with device prefix and contains a letter following the prefix, or a number
            // Wait, hdc is h+d+c. So just length > prefix_len?
            // "sd" is length 2. "sda" is length 3.
            if word.len() >= 3 || (word.starts_with("hd") && word.len() >= 3) {
                // Actually, "sd", "hd", "vd", "xvd", "nvme"
                // Let's just redact it.
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
        // Should not redact normal words
        assert_eq!(redact("The standard is good"), "The standard is good");
    }
}
