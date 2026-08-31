// SPDX-License-Identifier: MIT
/*
 * RamShared — Hardware Direct DMA & PCIe Latency Benchmark
 *
 * Measures raw Host-to-Device (H2D) and Device-to-Host (D2H) DMA throughput
 * across the PCIe bus using page-locked pinned memory and CUDA driver FFI.
 */

#define _POSIX_C_SOURCE 200112L
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <time.h>
#include <dlfcn.h>
#include <pthread.h>
#include <errno.h>

#define DEFAULT_CHUNK_MIB 256
#define MIB_TO_BYTES(mib) ((size_t)(mib) * 1024 * 1024)
#define DEFAULT_NUM_THREADS 4

typedef int (*cuInit_t)(unsigned int);
typedef int (*cuDeviceGet_t)(int*, int);
typedef int (*cuDeviceGetName_t)(char*, int, int);
typedef int (*cuCtxCreate_t)(void**, unsigned int, int);
typedef int (*cuCtxDestroy_t)(void*);
typedef int (*cuCtxSetCurrent_t)(void*);
typedef int (*cuMemGetInfo_t)(size_t*, size_t*);
typedef int (*cuMemAlloc_t)(uint64_t*, size_t);
typedef int (*cuMemFree_t)(uint64_t);
typedef int (*cuMemHostAlloc_t)(void**, size_t, unsigned int);
typedef int (*cuMemFreeHost_t)(void*);
typedef int (*cuMemcpyHtoD_t)(uint64_t, const void*, size_t);
typedef int (*cuMemcpyDtoH_t)(void*, uint64_t, size_t);

static cuCtxSetCurrent_t cuCtxSetCurrent;
static cuMemcpyHtoD_t cuMemcpyHtoD;
static cuMemcpyDtoH_t cuMemcpyDtoH;

struct worker_args {
	void *ctx;
	uint64_t dev_ptr;
	void *host_pinned;
	void *read_pinned;
	size_t chunk_bytes;
	pthread_barrier_t *start_barrier;
	pthread_barrier_t *end_barrier;
	int is_h2d;
};

static void *benchmark_worker(void *arg) {
	struct worker_args *wa = (struct worker_args *)arg;
	if (cuCtxSetCurrent) {
		cuCtxSetCurrent(wa->ctx);
	}

	pthread_barrier_wait(wa->start_barrier);

	if (wa->is_h2d) {
		cuMemcpyHtoD(wa->dev_ptr, wa->host_pinned, wa->chunk_bytes);
	} else {
		cuMemcpyDtoH(wa->read_pinned, wa->dev_ptr, wa->chunk_bytes);
	}

	pthread_barrier_wait(wa->end_barrier);
	return NULL;
}

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
	int num_threads = DEFAULT_NUM_THREADS;
	if (argc > 1) {
		int val = atoi(argv[1]);
		if (val > 0 && val <= 4096) {
			chunk_mib = val;
		}
	}
	if (argc > 2) {
		int val = atoi(argv[2]);
		if (val > 0 && val <= 256) {
			num_threads = val;
		}
	}
	size_t chunk_bytes = MIB_TO_BYTES(chunk_mib);
	size_t chunk_per_thread = chunk_bytes / num_threads;

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
	cuCtxSetCurrent = (cuCtxSetCurrent_t)dlsym(lib, "cuCtxSetCurrent");
	cuMemFree_t cuMemFree = (cuMemFree_t)dlsym(lib, "cuMemFree_v2");
	cuMemHostAlloc_t cuMemHostAlloc = (cuMemHostAlloc_t)dlsym(lib, "cuMemHostAlloc");
	cuMemFreeHost_t cuMemFreeHost = (cuMemFreeHost_t)dlsym(lib, "cuMemFreeHost");
	cuMemcpyHtoD = (cuMemcpyHtoD_t)dlsym(lib, "cuMemcpyHtoD_v2");
	cuMemcpyDtoH = (cuMemcpyDtoH_t)dlsym(lib, "cuMemcpyDtoH_v2");

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

	pthread_barrier_t start_barrier, end_barrier;
	if (pthread_barrier_init(&start_barrier, NULL, num_threads + 1) != 0) {
		fprintf(stderr, "[-] Error: pthread_barrier_init failed.\n");
		cuMemFree(dev_ptr);
		cuMemFreeHost(host_pinned);
		cuCtxDestroy(ctx);
		dlclose(lib);
		return -EINVAL;
	}
	if (pthread_barrier_init(&end_barrier, NULL, num_threads + 1) != 0) {
		fprintf(stderr, "[-] Error: pthread_barrier_init failed.\n");
		pthread_barrier_destroy(&start_barrier);
		cuMemFree(dev_ptr);
		cuMemFreeHost(host_pinned);
		cuCtxDestroy(ctx);
		dlclose(lib);
		return -EINVAL;
	}

	pthread_t threads[256];
	struct worker_args args[256];

	// 1. Direct DMA Host -> VRAM (Push)
	printf("[+] Benchmarking Host -> VRAM DMA (%d MiB) with %d threads...\n", chunk_mib, num_threads);
	for (int i = 0; i < num_threads; i++) {
		args[i].ctx = ctx;
		args[i].dev_ptr = dev_ptr + (i * chunk_per_thread);
		args[i].host_pinned = (uint8_t*)host_pinned + (i * chunk_per_thread);
		args[i].read_pinned = NULL;
		args[i].chunk_bytes = chunk_per_thread;
		args[i].start_barrier = &start_barrier;
		args[i].end_barrier = &end_barrier;
		args[i].is_h2d = 1;
		if (pthread_create(&threads[i], NULL, benchmark_worker, &args[i]) != 0) {
			fprintf(stderr, "[-] Error: pthread_create failed.\n");
			return -ENOMEM;
		}
	}

	pthread_barrier_wait(&start_barrier);
	double t0 = get_time_sec();

	pthread_barrier_wait(&end_barrier);
	double t1 = get_time_sec();

	for (int i = 0; i < num_threads; i++) {
		pthread_join(threads[i], NULL);
	}

	double h2d_speed = (double)chunk_mib / (t1 - t0);
	printf("[+] H2D PCIe DMA Write: %.2f MiB/s (%.2f GiB/s) in %.4f s\n",
	       h2d_speed, h2d_speed / 1024.0, t1 - t0);

	// 2. Direct DMA VRAM -> Host (Pull)
	void *read_pinned = NULL;
	if (cuMemHostAlloc(&read_pinned, chunk_bytes, 0) != 0) {
		fprintf(stderr, "[-] Error: cuMemHostAlloc failed.\n");
		return -ENOMEM;
	}

	printf("[+] Benchmarking VRAM -> Host DMA (%d MiB) with %d threads...\n", chunk_mib, num_threads);
	for (int i = 0; i < num_threads; i++) {
		args[i].read_pinned = (uint8_t*)read_pinned + (i * chunk_per_thread);
		args[i].is_h2d = 0;
		if (pthread_create(&threads[i], NULL, benchmark_worker, &args[i]) != 0) {
			fprintf(stderr, "[-] Error: pthread_create failed.\n");
			return -ENOMEM;
		}
	}

	pthread_barrier_wait(&start_barrier);
	t0 = get_time_sec();

	pthread_barrier_wait(&end_barrier);
	t1 = get_time_sec();

	for (int i = 0; i < num_threads; i++) {
		pthread_join(threads[i], NULL);
	}

	double d2h_speed = (double)chunk_mib / (t1 - t0);
	printf("[+] D2H PCIe DMA Read : %.2f MiB/s (%.2f GiB/s) in %.4f s\n",
	       d2h_speed, d2h_speed / 1024.0, t1 - t0);

	// 3. Bit-by-bit Verification
	int match = (memcmp(host_pinned, read_pinned, chunk_bytes) == 0);
	printf("\n[+] Data Integrity Proof: %s\n",
	       match ? "PASS (100% Bit-Exact Match, Zero Corruption)" : "FAIL");

	// Cleanup
	pthread_barrier_destroy(&start_barrier);
	pthread_barrier_destroy(&end_barrier);
	cuMemFree(dev_ptr);
	cuMemFreeHost(host_pinned);
	cuMemFreeHost(read_pinned);
	cuCtxDestroy(ctx);
	dlclose(lib);

	printf("=================================================================\n");
	return match ? 0 : 1;
}
