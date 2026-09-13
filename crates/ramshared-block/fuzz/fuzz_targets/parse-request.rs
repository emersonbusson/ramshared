#![no_main]

use libfuzzer_sys::fuzz_target;
use ramshared_block::protocol::parse_request;

fuzz_target!(|data: &[u8]| {
    let _ = parse_request(data);
});
