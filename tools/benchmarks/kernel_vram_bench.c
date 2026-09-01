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
#include <signal.h>
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

static volatile sig_atomic_t g_interrupted = 0;

static void handle_signal(int sig) {
	(void)sig;
	g_interrupted = 1;
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
	int ret = 0;

	if (argc > 1) {
		int val = atoi(argv[1]);
		if (val <= 0 || val > 4096) {
			fprintf(stderr, "[-] Error: Invalid chunk size. Must be 1-4096 MiB.\n");
			return -EINVAL;
		}
		chunk_mib = val;
	}

	size_t chunk_bytes = MIB_TO_BYTES(chunk_mib);
	if (chunk_bytes == 0 || chunk_bytes > MIB_TO_BYTES(4096)) {
		return -ERANGE;
	}

	struct sigaction sa;
	memset(&sa, 0, sizeof(sa));
	sa.sa_handler = handle_signal;
	sigaction(SIGINT, &sa, NULL);
	sigaction(SIGTERM, &sa, NULL);

	printf("=================================================================\n");
	printf("   RamShared Hardware DMA & PCIe Bandwidth Benchmark Tool         \n");
	printf("=================================================================\n");

	void *lib = load_cuda_driver();
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
	cuDeviceGet(&dev, 0);
	char dev_name[256] = {0};
	cuDeviceGetName(dev_name, sizeof(dev_name), dev);
	printf("[+] Hardware: %s (PCIe Direct DMA Channel)\n", dev_name);

	void *ctx = NULL;
	if (cuCtxCreate(&ctx, 0, dev) != 0) {
		fprintf(stderr, "[-] Error: cuCtxCreate failed.\n");
		ret = -ENODEV;
		goto out_lib;
	}

	size_t free_b = 0, total_b = 0;
	cuMemGetInfo(&free_b, &total_b);
	printf("[+] GPU Memory: %zu MiB free / %zu MiB total\n",
	       free_b / (1024 * 1024), total_b / (1024 * 1024));

	if (g_interrupted) {
		ret = -EINTR;
		goto out_ctx;
	}

	// Allocate Pinned Host Memory
	void *host_pinned = NULL;
	if (cuMemHostAlloc(&host_pinned, chunk_bytes, 0) != 0) {
		fprintf(stderr, "[-] Error: cuMemHostAlloc failed (%d MiB).\n", chunk_mib);
		ret = -ENOMEM;
		goto out_ctx;
	}
	memset(host_pinned, 0xA5, chunk_bytes);

	if (g_interrupted) {
		ret = -EINTR;
		goto out_host;
	}

	// Allocate Device VRAM Buffer
	uint64_t dev_ptr = 0;
	if (cuMemAlloc(&dev_ptr, chunk_bytes) != 0) {
		fprintf(stderr, "[-] Error: cuMemAlloc failed (%d MiB).\n", chunk_mib);
		ret = -ENOMEM;
		goto out_host;
	}

	if (g_interrupted) {
		ret = -EINTR;
		goto out_dev;
	}

	// 1. Direct DMA Host -> VRAM (Push)
	printf("[+] Benchmarking Host -> VRAM DMA (%d MiB)...\n", chunk_mib);
	double t0 = get_time_sec();
	cuMemcpyHtoD(dev_ptr, host_pinned, chunk_bytes);
	double t1 = get_time_sec();
	double h2d_speed = (double)chunk_mib / (t1 - t0);
	printf("[+] H2D PCIe DMA Write: %.2f MiB/s (%.2f GiB/s) in %.4f s\n",
	       h2d_speed, h2d_speed / 1024.0, t1 - t0);

	if (g_interrupted) {
		ret = -EINTR;
		goto out_dev;
	}

	// 2. Direct DMA VRAM -> Host (Pull)
	void *read_pinned = NULL;
	if (cuMemHostAlloc(&read_pinned, chunk_bytes, 0) != 0) {
		fprintf(stderr, "[-] Error: cuMemHostAlloc for read failed.\n");
		ret = -ENOMEM;
		goto out_dev;
	}

	if (g_interrupted) {
		ret = -EINTR;
		goto out_read;
	}

	printf("[+] Benchmarking VRAM -> Host DMA (%d MiB)...\n", chunk_mib);
	t0 = get_time_sec();
	cuMemcpyDtoH(read_pinned, dev_ptr, chunk_bytes);
	t1 = get_time_sec();
	double d2h_speed = (double)chunk_mib / (t1 - t0);
	printf("[+] D2H PCIe DMA Read : %.2f MiB/s (%.2f GiB/s) in %.4f s\n",
	       d2h_speed, d2h_speed / 1024.0, t1 - t0);

	if (g_interrupted) {
		ret = -EINTR;
		goto out_read;
	}

	// 3. Bit-by-bit Verification
	int match = (memcmp(host_pinned, read_pinned, chunk_bytes) == 0);
	printf("\n[+] Data Integrity Proof: %s\n",
	       match ? "PASS (100% Bit-Exact Match, Zero Corruption)" : "FAIL");

	ret = match ? 0 : -EFAULT;

	// Cleanup
out_read:
	if (read_pinned) cuMemFreeHost(read_pinned);
out_dev:
	if (dev_ptr) cuMemFree(dev_ptr);
out_host:
	if (host_pinned) cuMemFreeHost(host_pinned);
out_ctx:
	if (ctx) cuCtxDestroy(ctx);
out_lib:
	if (lib) dlclose(lib);

	printf("=================================================================\n");
	return ret;
}
