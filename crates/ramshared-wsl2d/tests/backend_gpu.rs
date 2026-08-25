//! Environment-bound GPU checks kept outside the backend business-logic slice.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use ramshared_block::{Command, Request, serve};
use ramshared_cuda::Cuda;
use ramshared_wsl2d::VramBackend;

#[test]
#[ignore = "requires a functional CUDA GPU (WSL2/GPU-PV)"]
fn vram_backend_serves_nbd_write_then_read() {
    let cuda = Cuda::load().expect("libcuda");
    let dev = cuda.device(0).unwrap();
    let ctx = cuda.create_context(&dev).unwrap();
    let mut mem = ctx.alloc(1 << 20).unwrap();
    mem.zero().unwrap();
    let mut be = VramBackend::new(mem, 4096);

    let payload = vec![0x5Au8; 4096];
    let w = serve(
        &Request {
            flags: 0,
            cmd: Command::Write,
            handle: 1,
            offset: 4096,
            len: 4096,
        },
        &payload,
        &mut be,
    );
    assert_eq!(
        u32::from_be_bytes([w.reply[4], w.reply[5], w.reply[6], w.reply[7]]),
        0,
        "WRITE must return NBD_OK"
    );

    let r = serve(
        &Request {
            flags: 0,
            cmd: Command::Read,
            handle: 2,
            offset: 4096,
            len: 4096,
        },
        &[],
        &mut be,
    );
    assert_eq!(r.read_data, payload, "READ must return the written payload");
}

#[test]
#[ignore = "requires a working CUDA GPU (WSL2/GPU-PV)"]
fn vram_gauge_outros_captures_real_graphics_usage() {
    use ramshared_wsl2d::telemetry::{VramGauge, vram_outros};
    use std::sync::atomic::Ordering;

    let cuda = Cuda::load().expect("libcuda");
    let dev = cuda.device(0).unwrap();
    let ctx = cuda.create_context(&dev).unwrap();
    let chunk = 64 * 1024 * 1024usize;
    let _mem = ctx.alloc(chunk).unwrap();
    let (free, total) = ctx.mem_info().unwrap();
    let gauge = VramGauge::default();
    gauge.free.store(free as u64, Ordering::Relaxed);
    gauge.total.store(total as u64, Ordering::Relaxed);
    assert!(total > 0 && free <= total, "mem_info is consistent");
    let used = (total - free) as u64;
    let alloc_daemon = chunk as u64;
    let outros = vram_outros(used, alloc_daemon);
    assert!(
        used > alloc_daemon,
        "total usage ({used}) > daemon allocation ({alloc_daemon})"
    );
    assert!(
        outros > 0,
        "vram_outros captures graphics/Windows usage: {outros} bytes"
    );
    eprintln!(
        "Real VRAM (MiB): total={} free={} used={} daemon={} other={}",
        total >> 20,
        free >> 20,
        used >> 20,
        alloc_daemon >> 20,
        outros >> 20
    );
}
