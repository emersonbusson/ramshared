#![allow(clippy::expect_used, clippy::unwrap_used)]

use ramshared_cuda::{CU_MEMHOSTREGISTER_DEVICEMAP, Cuda, CudaError};

#[test]
#[ignore = "requires a working CUDA GPU (run with --ignored on a GPU host)"]
fn pinned_host_mapping_legitimate_lifecycle() {
    let cuda = Cuda::load().expect("libcuda must load");
    let dev = cuda.device(0).expect("device(0) must exist");
    let ctx = cuda.create_context(&dev).expect("context must be created");
    let layout = std::alloc::Layout::from_size_align(4096, 4096).unwrap();
    let page_ptr = unsafe { std::alloc::alloc(layout) };
    assert!(!page_ptr.is_null(), "aligned host allocation must succeed");
    unsafe { core::ptr::write_bytes(page_ptr, 0x42, 4096) };
    let mut mapping = unsafe {
        ctx.register_host(page_ptr.cast(), 4096, CU_MEMHOSTREGISTER_DEVICEMAP)
            .expect("page registration must succeed")
    };
    assert_eq!(mapping.len(), 4096);
    assert!(!mapping.is_empty());
    assert_ne!(mapping.dev_ptr(), 0);
    assert_eq!(mapping.as_slice()[0], 0x42);
    mapping.as_mut_slice()[0] = 0xAA;
    assert_eq!(mapping.as_slice()[0], 0xAA);
    drop(mapping);
    unsafe { std::alloc::dealloc(page_ptr, layout) };
}

#[test]
#[ignore = "requires a working CUDA GPU (run with --ignored on a GPU host)"]
fn gpu_roundtrip_256mib() {
    let cuda = Cuda::load().expect("libcuda must load");
    assert!(cuda.device_count().expect("device count") >= 1);
    let dev = cuda.device(0).expect("device(0) must exist");
    let ctx = cuda.create_context(&dev).expect("context must be created");
    let (free_before, total) = ctx.mem_info().expect("memory info must be available");
    assert!(total > 0 && free_before > 0);

    let size = 256 * 1024 * 1024;
    let mut mem = ctx.alloc(size).expect("allocation must succeed");
    mem.zero().expect("zero must succeed");
    let pattern: Vec<u8> = (0..4096).map(|index| (index % 251) as u8).collect();
    for offset in [0, size / 2, size - pattern.len()] {
        mem.write_at(offset, &pattern).expect("write must succeed");
        let mut output = vec![0; pattern.len()];
        mem.read_at(offset, &mut output).expect("read must succeed");
        assert_eq!(output, pattern, "roundtrip diverged at offset {offset}");
    }
    let mut tiny = [0; 16];
    assert!(matches!(
        mem.read_at(size - 8, &mut tiny),
        Err(CudaError::OutOfRange { .. })
    ));
}
