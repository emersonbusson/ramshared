import sys

content = open("crates/ramshared-wsl2d/src/conn.rs", "r").read()

content = content.replace(
    'let Ok(idx) = server_handshake(&mut reader, &mut hs_writer, &exports, tx_flags) else {\n            eprintln!("[ramsharedd] conn: handshake failed");\n            let _ = jobs.send(WMsg::Closed);\n            return;\n        };',
    '''let idx = match server_handshake(&mut reader, &mut hs_writer, &exports, tx_flags) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("[ramsharedd] conn: handshake failed: {e}");
                let _ = jobs.send(WMsg::Closed);
                return;
            }
        };'''
)

content = content.replace(
    'let Ok(req) = parse_request(&hdr) else {\n                eprintln!("[ramsharedd] conn: malformed request; disconnecting");\n                break;\n            };',
    '''let req = match parse_request(&hdr) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[ramsharedd] conn: malformed request: {e}; disconnecting");
                    break;
                }
            };'''
)

content = content.replace(
    '''let Ok((stream, _)) = listener.accept() else {
                eprintln!("[ramsharedd] accept failed");
                break;
            };''',
    '''let stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(e) => {
                    eprintln!("[ramsharedd] accept failed: {e}");
                    break;
                }
            };'''
)

content = content.replace(
    '''let Ok((stream, _)) = listener.accept() else {
                eprintln!("[ramsharedd] TCP accept failed");
                break;
            };''',
    '''let stream = match listener.accept() {
                Ok((s, _)) => s,
                Err(e) => {
                    eprintln!("[ramsharedd] TCP accept failed: {e}");
                    break;
                }
            };'''
)

open("crates/ramshared-wsl2d/src/conn.rs", "w").write(content)
