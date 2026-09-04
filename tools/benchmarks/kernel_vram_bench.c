// SPDX-License-Identifier: MIT
/*
 * RamShared — Hardware Direct DMA & PCIe Latency Benchmark
 *
 * Measures raw Host-to-Device (H2D) and Device-to-Host (D2H) DMA throughput
 * across the PCIe bus using page-locked pinned memory and CUDA driver FFI.
 */

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <time.h>
#include <dlfcn.h>
#include <errno.h>

#define DEFAULT_CHUNK_MIB 256
#define MIB_TO_BYTES(mib) ((size_t)(mib) * 1024 * 1024)

typedef int (*cuInit_t)(unsigned int);
typedef int (*cuDeviceGet_t)(int*, int);
typedef int (*cuDeviceGetName_t)(char*, int, int);
typedef int (*cuCtxCreate_t)(void**, unsigned int, int);
typedef int (*cuCtxDestroy_t)(void*);
typedef int (*cuMemGetInfo_t)(size_t*, size_t*);
typedef int (*cuMemAlloc_t)(uint64_t*, size_t);
typedef int (*cuMemFree_t)(uint64_t);
typedef int (*cuMemHostAlloc_t)(void**, size_t, unsigned int);
typedef int (*cuMemFreeHost_t)(void*);
typedef int (*cuMemcpyHtoD_t)(uint64_t, const void*, size_t);
typedef int (*cuMemcpyDtoH_t)(void*, uint64_t, size_t);

static double get_time_sec(void) {
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return (double)ts.tv_sec + (double)ts.tv_nsec * 1e-9;
}

static void *load_cuda_driver(void) {
	const char *paths[] = {
		"libcuda.so.1",
		"libcuda.so",
		"/usr/lib/x86_64-linux-gnu/libcuda.so.1",
		"/usr/lib/wsl/lib/libcuda.so.1",
		"/usr/local/cuda/lib64/libcuda.so"
	};
	for (size_t i = 0; i < sizeof(paths)/sizeof(paths[0]); i++) {
		void *handle = dlopen(paths[i], RTLD_NOW);
		if (handle) return handle;
	}
	return NULL;
}

int main(int argc, char **argv) {
	int chunk_mib = DEFAULT_CHUNK_MIB;
	int ret = 0;
	void *lib = NULL;
	void *ctx = NULL;
	void *host_pinned = NULL;
	void *read_pinned = NULL;
	uint64_t dev_ptr = 0;

	if (argc > 1)
		chunk_mib = atoi(argv[1]);

	if (chunk_mib <= 0 || chunk_mib > 4096) {
		fprintf(stderr, "[-] Error: Invalid chunk_mib size (must be 1-4096).\n");
		return -EINVAL;
	}
	size_t chunk_bytes = MIB_TO_BYTES(chunk_mib);

	if (chunk_bytes % 4096 != 0) {
		fprintf(stderr, "[-] Error: Buffer size not 4096-byte aligned.\n");
		return -EINVAL;
	}

	printf("=================================================================\n");
	printf("   RamShared Hardware DMA & PCIe Bandwidth Benchmark Tool         \n");
	printf("=================================================================\n");

	lib = load_cuda_driver();
	if (!lib) {
		fprintf(stderr, "[-] Error: CUDA driver library (libcuda.so.1) not found.\n");
		return -ENODEV;
	}

	cuInit_t cuInit = (cuInit_t)dlsym(lib, "cuInit");
	cuDeviceGet_t cuDeviceGet = (cuDeviceGet_t)dlsym(lib, "cuDeviceGet");
	cuDeviceGetName_t cuDeviceGetName = (cuDeviceGetName_t)dlsym(lib, "cuDeviceGetName");
	cuCtxCreate_t cuCtxCreate = (cuCtxCreate_t)dlsym(lib, "cuCtxCreate_v2");
	cuCtxDestroy_t cuCtxDestroy = (cuCtxDestroy_t)dlsym(lib, "cuCtxDestroy_v2");
	cuMemGetInfo_t cuMemGetInfo = (cuMemGetInfo_t)dlsym(lib, "cuMemGetInfo_v2");
	cuMemAlloc_t cuMemAlloc = (cuMemAlloc_t)dlsym(lib, "cuMemAlloc_v2");
	cuMemFree_t cuMemFree = (cuMemFree_t)dlsym(lib, "cuMemFree_v2");
	cuMemHostAlloc_t cuMemHostAlloc = (cuMemHostAlloc_t)dlsym(lib, "cuMemHostAlloc");
	cuMemFreeHost_t cuMemFreeHost = (cuMemFreeHost_t)dlsym(lib, "cuMemFreeHost");
	cuMemcpyHtoD_t cuMemcpyHtoD = (cuMemcpyHtoD_t)dlsym(lib, "cuMemcpyHtoD_v2");
	cuMemcpyDtoH_t cuMemcpyDtoH = (cuMemcpyDtoH_t)dlsym(lib, "cuMemcpyDtoH_v2");

	if (!cuInit || !cuCtxCreate || !cuMemcpyHtoD || !cuMemcpyDtoH) {
		fprintf(stderr, "[-] Error: Required CUDA symbols missing in library.\n");
		ret = -EINVAL;
		goto out_lib;
	}

	if (cuInit(0) != 0) {
		fprintf(stderr, "[-] Error: cuInit failed.\n");
		ret = -ENODEV;
		goto out_lib;
	}

	int dev = 0;
	if (cuDeviceGet(&dev, 0) != 0) {
		fprintf(stderr, "[-] Error: cuDeviceGet failed.\n");
		ret = -ENODEV;
		goto out_lib;
	}

	char dev_name[256] = {0};
	if (cuDeviceGetName(dev_name, sizeof(dev_name), dev) != 0) {
		fprintf(stderr, "[-] Error: cuDeviceGetName failed.\n");
		ret = -ENODEV;
		goto out_lib;
	}
	printf("[+] Hardware: %s (PCIe Direct DMA Channel)\n", dev_name);

	if (cuCtxCreate(&ctx, 0, dev) != 0) {
		fprintf(stderr, "[-] Error: cuCtxCreate failed.\n");
		ret = -ENODEV;
		goto out_lib;
	}

	size_t free_b = 0, total_b = 0;
	if (cuMemGetInfo(&free_b, &total_b) != 0) {
		fprintf(stderr, "[-] Error: cuMemGetInfo failed.\n");
		ret = -EINVAL;
		goto out_ctx;
	}
	printf("[+] GPU Memory: %zu MiB free / %zu MiB total\n",
	       free_b / (1024 * 1024), total_b / (1024 * 1024));

	// Allocate Pinned Host Memory
	if (cuMemHostAlloc(&host_pinned, chunk_bytes, 0) != 0) {
		fprintf(stderr, "[-] Error: cuMemHostAlloc failed (%d MiB).\n", chunk_mib);
		ret = -ENOMEM;
		goto out_ctx;
	}

	// Fill buffers with pseudorandom test patterns (Xorshift32)
	uint32_t seed = (uint32_t)time(NULL) | 1;
	uint32_t *hp_ptr32 = (uint32_t *)host_pinned;
	size_t num_words = chunk_bytes / sizeof(uint32_t);
	for (size_t i = 0; i < num_words; i++) {
		seed ^= seed << 13;
		seed ^= seed >> 17;
		seed ^= seed << 5;
		hp_ptr32[i] = seed;
	}

	// Allocate Device VRAM Buffer
	if (cuMemAlloc(&dev_ptr, chunk_bytes) != 0) {
		fprintf(stderr, "[-] Error: cuMemAlloc failed (%d MiB).\n", chunk_mib);
		ret = -ENOMEM;
		goto out_host_pinned;
	}

	// 1. Direct DMA Host -> VRAM (Push)
	printf("[+] Benchmarking Host -> VRAM DMA (%d MiB)...\n", chunk_mib);
	double t0 = get_time_sec();
	if (cuMemcpyHtoD(dev_ptr, host_pinned, chunk_bytes) != 0) {
		fprintf(stderr, "[-] Error: cuMemcpyHtoD failed.\n");
		ret = -EFAULT;
		goto out_dev_ptr;
	}
	double t1 = get_time_sec();
	double h2d_speed = (double)chunk_mib / (t1 - t0);
	printf("[+] H2D PCIe DMA Write: %.2f MiB/s (%.2f GiB/s) in %.4f s\n",
	       h2d_speed, h2d_speed / 1024.0, t1 - t0);

	// 2. Direct DMA VRAM -> Host (Pull)
	if (cuMemHostAlloc(&read_pinned, chunk_bytes, 0) != 0) {
		fprintf(stderr, "[-] Error: cuMemHostAlloc (read) failed.\n");
		ret = -ENOMEM;
		goto out_dev_ptr;
	}

	printf("[+] Benchmarking VRAM -> Host DMA (%d MiB)...\n", chunk_mib);
	t0 = get_time_sec();
	if (cuMemcpyDtoH(read_pinned, dev_ptr, chunk_bytes) != 0) {
		fprintf(stderr, "[-] Error: cuMemcpyDtoH failed.\n");
		ret = -EFAULT;
		goto out_read_pinned;
	}
	t1 = get_time_sec();
	double d2h_speed = (double)chunk_mib / (t1 - t0);
	printf("[+] D2H PCIe DMA Read : %.2f MiB/s (%.2f GiB/s) in %.4f s\n",
	       d2h_speed, d2h_speed / 1024.0, t1 - t0);

	// 3. Bit-by-bit Verification
	int match = (memcmp(host_pinned, read_pinned, chunk_bytes) == 0);
	printf("\n[+] Data Integrity Proof: %s\n",
	       match ? "PASS (100% Bit-Exact Match, Zero Corruption)" : "FAIL");

	if (!match) {
		ret = -EFAULT;
	}

out_read_pinned:
	if (read_pinned) cuMemFreeHost(read_pinned);
out_dev_ptr:
	if (dev_ptr) cuMemFree(dev_ptr);
out_host_pinned:
	if (host_pinned) cuMemFreeHost(host_pinned);
out_ctx:
	if (ctx) cuCtxDestroy(ctx);
out_lib:
	if (lib) dlclose(lib);

	printf("=================================================================\n");
	return ret;
}
