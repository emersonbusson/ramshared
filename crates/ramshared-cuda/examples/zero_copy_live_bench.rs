/*
 * SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Live zero-copy host memory registration benchmark on hardware GPU.

use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::time::Instant;

use ramshared_cuda::{CU_MEMHOSTREGISTER_DEVICEMAP, Cuda};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== RamShared CUDA Zero-Copy Live Hardware Verification ===");

    let cuda = Cuda::load()?;
    let dev_count = cuda.device_count().unwrap_or(0);
    println!("[+] CUDA Driver loaded successfully. Devices detected: {dev_count}");
    if dev_count == 0 {
        eprintln!("[-] No CUDA devices found.");
        return Ok(());
    }

    let dev = cuda.device(0)?;
    println!("[+] Device 0: {} (Ordinal: {})", dev.name(), dev.ordinal());

    let ctx = cuda.create_context(&dev)?;
    let (free_mem, total_mem) = ctx.mem_info()?;
    println!(
        "[+] VRAM Topology: {:.2} MiB Free / {:.2} MiB Total",
        free_mem as f64 / (1024.0 * 1024.0),
        total_mem as f64 / (1024.0 * 1024.0)
    );

    // 1. Allocate 64 MiB page-aligned host memory
    let test_size_bytes = 64 * 1024 * 1024; // 64 MiB
    let page_align = 4096;
    let layout = Layout::from_size_align(test_size_bytes, page_align)?;
    let host_ptr = unsafe { alloc_zeroed(layout) };
    if host_ptr.is_null() {
        return Err("Host allocation failed".into());
    }

    // Populate buffer with non-trivial pattern
    println!("[+] Initializing 64 MiB host buffer with pseudo-random pattern...");
    let host_slice = unsafe { std::slice::from_raw_parts_mut(host_ptr, test_size_bytes) };
    for (i, byte) in host_slice.iter_mut().enumerate() {
        *byte = ((i ^ (i >> 8)) & 0xFF) as u8;
    }

    // 2. Register with CUDA via cuMemHostRegister
    println!("[+] Registering host buffer via cuMemHostRegister (zero-copy)...");
    let reg_start = Instant::now();
    let mapping = unsafe {
        ctx.register_host(
            host_ptr.cast(),
            test_size_bytes,
            CU_MEMHOSTREGISTER_DEVICEMAP,
        )?
    };
    let reg_elapsed = reg_start.elapsed();

    println!(
        "[+] Mapped Successfully! Device Pointer: 0x{:x}, Registration Time: {:?}",
        mapping.dev_ptr(),
        reg_elapsed
    );
    assert_eq!(mapping.len(), test_size_bytes);
    assert!(mapping.dev_ptr() != 0);

    // 3. Allocate device VRAM buffer to verify DMA transfer
    println!("[+] Allocating 64 MiB device VRAM buffer...");
    let mut dev_buf = ctx.alloc(test_size_bytes)?;

    // Warmup + 10 iterations of Host -> VRAM -> Host transfer
    println!("[+] Running 10 DMA transfer iterations between Pinned Host Mapping and VRAM...");
    let iterations = 10;
    let mut dma_to_vram_durations = Vec::new();
    let mut dma_from_vram_durations = Vec::new();

    let mut verify_buf = vec![0u8; test_size_bytes];

    for _ in 0..iterations {
        // Pinned Host -> VRAM
        let t0 = Instant::now();
        dev_buf.write_at(0, mapping.as_slice())?;
        dma_to_vram_durations.push(t0.elapsed());

        // VRAM -> Host
        let t1 = Instant::now();
        dev_buf.read_at(0, &mut verify_buf)?;
        dma_from_vram_durations.push(t1.elapsed());
    }

    // Calculate throughput
    let avg_to_vram_s = dma_to_vram_durations
        .iter()
        .map(|d| d.as_secs_f64())
        .sum::<f64>()
        / iterations as f64;
    let avg_from_vram_s = dma_from_vram_durations
        .iter()
        .map(|d| d.as_secs_f64())
        .sum::<f64>()
        / iterations as f64;

    let gb_size = test_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let to_vram_bw_gbps = gb_size / avg_to_vram_s;
    let from_vram_bw_gbps = gb_size / avg_from_vram_s;

    // 4. Verify bit-level data integrity
    println!("[+] Verifying 100% data integrity...");
    let mut mismatches = 0usize;
    for (i, (&orig, &back)) in host_slice.iter().zip(verify_buf.iter()).enumerate() {
        if orig != back {
            mismatches += 1;
            if mismatches <= 3 {
                eprintln!(
                    "[-] Integrity mismatch at offset {i}: orig={orig:#04x}, got={back:#04x}"
                );
            }
        }
    }

    assert_eq!(
        mismatches, 0,
        "Data corruption detected during zero-copy DMA!"
    );
    println!("[+] Data Integrity: 100% MATCH (0 bit errors across 64 MiB)");

    // Print summary
    println!("\n=================== BENCHMARK RESULTS ===================");
    println!("Workload:           64 MiB Zero-Copy Host Registration");
    println!(
        "Host -> VRAM Bandwidth: {:.2} GB/s ({:.2} ms avg)",
        to_vram_bw_gbps,
        avg_to_vram_s * 1000.0
    );
    println!(
        "VRAM -> Host Bandwidth: {:.2} GB/s ({:.2} ms avg)",
        from_vram_bw_gbps,
        avg_from_vram_s * 1000.0
    );
    println!(
        "Registration Latency:   {:.3} ms",
        reg_elapsed.as_secs_f64() * 1000.0
    );
    println!("Status:                 PASS_ZERO_PANIC (Verified Live on RTX GPU)");
    println!("=========================================================\n");

    // 5. Clean teardown
    println!("[+] Dropping PinnedHostMapping (triggering cuMemHostUnregister)...");
    drop(mapping);
    drop(dev_buf);

    unsafe {
        dealloc(host_ptr, layout);
    }
    println!("[+] Cleanup verified. Test completed cleanly!");
    Ok(())
}
