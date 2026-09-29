#![allow(clippy::expect_used, clippy::unwrap_used)]

use ramshared_cuda::Cuda;
use ramshared_vram::{VramMemory, VramProvider};

#[test]
#[ignore = "requires functional CUDA GPU"]
fn cuda_vram_traits_round_trip_on_hardware() {
    let cuda = Cuda::load().expect("libcuda must load");
    let device = cuda.device(0).expect("device(0) must exist");
    let context = cuda
        .create_context(&device)
        .expect("context must be created");
    let size = 1024;

    let mut memory = VramProvider::alloc(&context, size).expect("allocation must work");
    assert_eq!(VramMemory::len(&memory), size);
    assert!(!VramMemory::is_empty(&memory));
    VramMemory::zero(&mut memory).expect("zero must work");

    let source = b"hello";
    VramMemory::write_at(&mut memory, 0, source).expect("write must work");
    let mut output = vec![0; source.len()];
    VramMemory::read_at(&memory, 0, &mut output).expect("read must work");
    assert_eq!(output, source);

    let (free, total) = VramProvider::mem_info(&context).expect("memory info must work");
    assert!(total > 0);
    assert!(free > 0);
    assert!(free <= total);
}
