// SPDX-License-Identifier: MIT
/*
 * RamShared — Hardware Direct DMA & PCIe Latency Benchmark
 *
 * Measures raw Host-to-Device (H2D) and Device-to-Host (D2H) DMA throughput
 * across the PCIe bus using page-locked pinned memory and CUDA driver FFI.
 */

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <time.h>
#include <dlfcn.h>
#include <unistd.h>
#include <sched.h>
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
	const char *candidates[] = {
		"/usr/lib/wsl/lib/libcuda.so.1",
		"libcuda.so.1",
		"libcuda.so",
		NULL
	};

	for (int i = 0; candidates[i] != NULL; i++) {
		void *lib = dlopen(candidates[i], RTLD_NOW);
		if (lib) return lib;
	}
	return NULL;
}

int main(int argc, char **argv) {
	int chunk_mib = DEFAULT_CHUNK_MIB;
	if (argc > 1) {
		int val = atoi(argv[1]);
		if (val > 0 && val <= 4096) {
			chunk_mib = val;
		}
	}

	if (argc > 2) {
		int core_id = atoi(argv[2]);
		long num_cores = sysconf(_SC_NPROCESSORS_ONLN);

		if (core_id < 0 || core_id >= num_cores) {
			fprintf(stderr, "[-] Error: Requested CPU core %d is out of range (0-%ld).\n", core_id, num_cores - 1);
			return -ERANGE;
		}

		cpu_set_t cpuset;
		CPU_ZERO(&cpuset);
		CPU_SET(core_id, &cpuset);
		if (sched_setaffinity(0, sizeof(cpu_set_t), &cpuset) != 0) {
			fprintf(stderr, "[-] Error: Failed to set CPU affinity to core %d.\n", core_id);
			return -EINVAL;
		}
	}

	size_t chunk_bytes = MIB_TO_BYTES(chunk_mib);

	printf("=================================================================\n");
	printf("   RamShared Hardware DMA & PCIe Bandwidth Benchmark Tool         \n");
	printf("=================================================================\n");

	void *lib = load_cuda_driver();
	if (!lib) {
		fprintf(stderr, "[-] Error: CUDA driver library (libcuda.so.1) not found.\n");
		return 1;
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
		dlclose(lib);
		return 1;
	}

	if (cuInit(0) != 0) {
		fprintf(stderr, "[-] Error: cuInit failed.\n");
		dlclose(lib);
		return 1;
	}

	int dev = 0;
	cuDeviceGet(&dev, 0);
	char dev_name[256] = {0};
	cuDeviceGetName(dev_name, sizeof(dev_name), dev);
	printf("[+] Hardware: %s (PCIe Direct DMA Channel)\n", dev_name);

	void *ctx = NULL;
	if (cuCtxCreate(&ctx, 0, dev) != 0) {
		fprintf(stderr, "[-] Error: cuCtxCreate failed.\n");
		dlclose(lib);
		return 1;
	}

	size_t free_b = 0, total_b = 0;
	cuMemGetInfo(&free_b, &total_b);
	printf("[+] GPU Memory: %zu MiB free / %zu MiB total\n",
	       free_b / (1024 * 1024), total_b / (1024 * 1024));

	// Allocate Pinned Host Memory
	void *host_pinned = NULL;
	if (cuMemHostAlloc(&host_pinned, chunk_bytes, 0) != 0) {
		fprintf(stderr, "[-] Error: cuMemHostAlloc failed (%d MiB).\n", chunk_mib);
		cuCtxDestroy(ctx);
		dlclose(lib);
		return 1;
	}
	memset(host_pinned, 0xA5, chunk_bytes);

	// Allocate Device VRAM Buffer
	uint64_t dev_ptr = 0;
	if (cuMemAlloc(&dev_ptr, chunk_bytes) != 0) {
		fprintf(stderr, "[-] Error: cuMemAlloc failed (%d MiB).\n", chunk_mib);
		cuMemFreeHost(host_pinned);
		cuCtxDestroy(ctx);
		dlclose(lib);
		return 1;
	}

	// 1. Direct DMA Host -> VRAM (Push)
	printf("[+] Benchmarking Host -> VRAM DMA (%d MiB)...\n", chunk_mib);
	double t0 = get_time_sec();
	cuMemcpyHtoD(dev_ptr, host_pinned, chunk_bytes);
	double t1 = get_time_sec();
	double h2d_speed = (double)chunk_mib / (t1 - t0);
	printf("[+] H2D PCIe DMA Write: %.2f MiB/s (%.2f GiB/s) in %.4f s\n",
	       h2d_speed, h2d_speed / 1024.0, t1 - t0);

	// 2. Direct DMA VRAM -> Host (Pull)
	void *read_pinned = NULL;
	cuMemHostAlloc(&read_pinned, chunk_bytes, 0);
	printf("[+] Benchmarking VRAM -> Host DMA (%d MiB)...\n", chunk_mib);
	t0 = get_time_sec();
	cuMemcpyDtoH(read_pinned, dev_ptr, chunk_bytes);
	t1 = get_time_sec();
	double d2h_speed = (double)chunk_mib / (t1 - t0);
	printf("[+] D2H PCIe DMA Read : %.2f MiB/s (%.2f GiB/s) in %.4f s\n",
	       d2h_speed, d2h_speed / 1024.0, t1 - t0);

	// 3. Bit-by-bit Verification
	int match = (memcmp(host_pinned, read_pinned, chunk_bytes) == 0);
	printf("\n[+] Data Integrity Proof: %s\n",
	       match ? "PASS (100% Bit-Exact Match, Zero Corruption)" : "FAIL");

	// Cleanup
	cuMemFree(dev_ptr);
	cuMemFreeHost(host_pinned);
	cuMemFreeHost(read_pinned);
	cuCtxDestroy(ctx);
	dlclose(lib);

	printf("=================================================================\n");
	return match ? 0 : 1;
}
