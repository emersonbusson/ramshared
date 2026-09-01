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

#define DEFAULT_CHUNK_MIB 256
#define MIB_TO_BYTES(mib) ((size_t)(mib) * 1024 * 1024)

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

#include <pthread.h>
#include <errno.h>

static double get_time_sec(void) {
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return (double)ts.tv_sec + (double)ts.tv_nsec * 1e-9;
}

struct bench_ctx {
	void *cuda_ctx;
	cuCtxSetCurrent_t cuCtxSetCurrent;
	cuMemcpyHtoD_t cuMemcpyHtoD;
	cuMemcpyDtoH_t cuMemcpyDtoH;
	uint64_t dev_ptr;
	void *host_pinned;
	size_t chunk_bytes;
	int is_d2h;
	pthread_barrier_t *barrier_start;
	pthread_barrier_t *barrier_end;
	int status;
};

static void *benchmark_worker(void *arg)
{
	struct bench_ctx *bctx = arg;
	int ret;

	if (!bctx)
		return NULL;

	if (bctx->cuCtxSetCurrent) {
		ret = bctx->cuCtxSetCurrent(bctx->cuda_ctx);
		if (ret != 0) {
			bctx->status = -EINVAL;
			goto out_sync;
		}
	}

	pthread_barrier_wait(bctx->barrier_start);

	if (bctx->is_d2h)
		bctx->status = bctx->cuMemcpyDtoH(bctx->host_pinned, bctx->dev_ptr, bctx->chunk_bytes);
	else
		bctx->status = bctx->cuMemcpyHtoD(bctx->dev_ptr, bctx->host_pinned, bctx->chunk_bytes);

	pthread_barrier_wait(bctx->barrier_end);

	return NULL;

out_sync:
	pthread_barrier_wait(bctx->barrier_start);
	pthread_barrier_wait(bctx->barrier_end);
	return NULL;
}

static int run_multithreaded_bench(
	void *cuda_ctx,
	cuCtxSetCurrent_t cuCtxSetCurrent,
	cuMemcpyHtoD_t cuMemcpyHtoD,
	cuMemcpyDtoH_t cuMemcpyDtoH,
	uint64_t dev_ptr,
	void *host_pinned,
	size_t total_bytes,
	int num_threads,
	int is_d2h,
	double *out_speed_mib,
	double *out_time_sec)
{
	pthread_t *threads = NULL;
	struct bench_ctx *bctx = NULL;
	pthread_barrier_t barrier_start;
	pthread_barrier_t barrier_end;
	size_t thread_chunk;
	int ret = 0;
	double t0, t1;
	int i, j;

	if (!cuda_ctx || !dev_ptr || !host_pinned || total_bytes == 0 || num_threads <= 0)
		return -EINVAL;

	if (total_bytes & 0xFFF)
		return -EINVAL;

	thread_chunk = total_bytes / num_threads;
	if (thread_chunk & 0xFFF)
		return -EINVAL;

	threads = calloc(num_threads, sizeof(pthread_t));
	if (!threads)
		return -ENOMEM;

	bctx = calloc(num_threads, sizeof(struct bench_ctx));
	if (!bctx) {
		ret = -ENOMEM;
		goto err_free_threads;
	}

	if (pthread_barrier_init(&barrier_start, NULL, num_threads + 1) != 0) {
		ret = -EINVAL;
		goto err_free_bctx;
	}

	if (pthread_barrier_init(&barrier_end, NULL, num_threads + 1) != 0) {
		ret = -EINVAL;
		goto err_destroy_start;
	}

	for (i = 0; i < num_threads; i++) {
		bctx[i].cuda_ctx = cuda_ctx;
		bctx[i].cuCtxSetCurrent = cuCtxSetCurrent;
		bctx[i].cuMemcpyHtoD = cuMemcpyHtoD;
		bctx[i].cuMemcpyDtoH = cuMemcpyDtoH;
		bctx[i].dev_ptr = dev_ptr + (i * thread_chunk);
		bctx[i].host_pinned = (char *)host_pinned + (i * thread_chunk);
		bctx[i].chunk_bytes = thread_chunk;
		bctx[i].is_d2h = is_d2h;
		bctx[i].barrier_start = &barrier_start;
		bctx[i].barrier_end = &barrier_end;
		bctx[i].status = 0;

		if (pthread_create(&threads[i], NULL, benchmark_worker, &bctx[i]) != 0) {
			ret = -EBUSY;
			goto err_cancel_threads;
		}
	}

	pthread_barrier_wait(&barrier_start);
	t0 = get_time_sec();

	pthread_barrier_wait(&barrier_end);
	t1 = get_time_sec();

	for (i = 0; i < num_threads; i++) {
		pthread_join(threads[i], NULL);
		if (bctx[i].status != 0)
			ret = -EFAULT;
	}

	if (ret == 0 && out_speed_mib && out_time_sec) {
		double elapsed = t1 - t0;
		*out_time_sec = elapsed;
		if (elapsed > 0)
			*out_speed_mib = ((double)total_bytes / (1024.0 * 1024.0)) / elapsed;
		else
			*out_speed_mib = 0;
	}

	pthread_barrier_destroy(&barrier_end);
	pthread_barrier_destroy(&barrier_start);
	free(bctx);
	free(threads);

	return ret;

err_cancel_threads:
	for (j = 0; j < i; j++) {
		pthread_cancel(threads[j]);
		pthread_join(threads[j], NULL);
	}
	pthread_barrier_destroy(&barrier_end);
err_destroy_start:
	pthread_barrier_destroy(&barrier_start);
err_free_bctx:
	free(bctx);
err_free_threads:
	free(threads);

	return ret;
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
	int num_threads = 4;

	if (argc > 1)
		chunk_mib = atoi(argv[1]);
	if (argc > 2)
		num_threads = atoi(argv[2]);

	if (chunk_mib <= 0 || chunk_mib > 4096) {
		fprintf(stderr, "[-] Error: chunk_mib out of bounds (1-4096).\n");
		return -ERANGE;
	}
	if (num_threads <= 0 || num_threads > 1024) {
		fprintf(stderr, "[-] Error: num_threads out of bounds (1-1024).\n");
		return -ERANGE;
	}

	size_t chunk_bytes = MIB_TO_BYTES(chunk_mib);
	if (chunk_bytes & 0xFFF) {
		fprintf(stderr, "[-] Error: buffer not 4096-byte aligned.\n");
		return -EINVAL;
	}

	size_t thread_chunk = chunk_bytes / num_threads;
	if (thread_chunk & 0xFFF) {
		fprintf(stderr, "[-] Error: thread chunk not 4096-byte aligned.\n");
		return -EINVAL;
	}

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
	cuCtxSetCurrent_t cuCtxSetCurrent = (cuCtxSetCurrent_t)dlsym(lib, "cuCtxSetCurrent");
	cuMemGetInfo_t cuMemGetInfo = (cuMemGetInfo_t)dlsym(lib, "cuMemGetInfo_v2");
	cuMemAlloc_t cuMemAlloc = (cuMemAlloc_t)dlsym(lib, "cuMemAlloc_v2");
	cuMemFree_t cuMemFree = (cuMemFree_t)dlsym(lib, "cuMemFree_v2");
	cuMemHostAlloc_t cuMemHostAlloc = (cuMemHostAlloc_t)dlsym(lib, "cuMemHostAlloc");
	cuMemFreeHost_t cuMemFreeHost = (cuMemFreeHost_t)dlsym(lib, "cuMemFreeHost");
	cuMemcpyHtoD_t cuMemcpyHtoD = (cuMemcpyHtoD_t)dlsym(lib, "cuMemcpyHtoD_v2");
	cuMemcpyDtoH_t cuMemcpyDtoH = (cuMemcpyDtoH_t)dlsym(lib, "cuMemcpyDtoH_v2");

	if (!cuInit || !cuCtxCreate || !cuMemcpyHtoD || !cuMemcpyDtoH || !cuCtxSetCurrent) {
		fprintf(stderr, "[-] Error: Required CUDA symbols missing in library.\n");
		dlclose(lib);
		return -EINVAL;
	}

	if (cuInit(0) != 0) {
		fprintf(stderr, "[-] Error: cuInit failed.\n");
		dlclose(lib);
		return -EFAULT;
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
		return -EFAULT;
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
		return -ENOMEM;
	}
	memset(host_pinned, 0xA5, chunk_bytes);

	// Allocate Device VRAM Buffer
	uint64_t dev_ptr = 0;
	if (cuMemAlloc(&dev_ptr, chunk_bytes) != 0) {
		fprintf(stderr, "[-] Error: cuMemAlloc failed (%d MiB).\n", chunk_mib);
		cuMemFreeHost(host_pinned);
		cuCtxDestroy(ctx);
		dlclose(lib);
		return -ENOMEM;
	}

	double speed = 0, elapsed = 0;
	int ret = 0;

	// 1. Direct DMA Host -> VRAM (Push)
	printf("[+] Benchmarking Host -> VRAM DMA (%d MiB, %d threads)...\n", chunk_mib, num_threads);
	ret = run_multithreaded_bench(ctx, cuCtxSetCurrent, cuMemcpyHtoD, cuMemcpyDtoH,
				      dev_ptr, host_pinned, chunk_bytes, num_threads, 0,
				      &speed, &elapsed);
	if (ret != 0) {
		fprintf(stderr, "[-] Error: H2D benchmark failed (code: %d)\n", ret);
		goto out_cleanup;
	}
	printf("[+] H2D PCIe DMA Write: %.2f MiB/s (%.2f GiB/s) in %.4f s\n",
	       speed, speed / 1024.0, elapsed);

	// 2. Direct DMA VRAM -> Host (Pull)
	void *read_pinned = NULL;
	if (cuMemHostAlloc(&read_pinned, chunk_bytes, 0) != 0) {
		fprintf(stderr, "[-] Error: cuMemHostAlloc for read failed.\n");
		ret = -ENOMEM;
		goto out_cleanup;
	}

	printf("[+] Benchmarking VRAM -> Host DMA (%d MiB, %d threads)...\n", chunk_mib, num_threads);
	ret = run_multithreaded_bench(ctx, cuCtxSetCurrent, cuMemcpyHtoD, cuMemcpyDtoH,
				      dev_ptr, read_pinned, chunk_bytes, num_threads, 1,
				      &speed, &elapsed);
	if (ret != 0) {
		fprintf(stderr, "[-] Error: D2H benchmark failed (code: %d)\n", ret);
		cuMemFreeHost(read_pinned);
		goto out_cleanup;
	}
	printf("[+] D2H PCIe DMA Read : %.2f MiB/s (%.2f GiB/s) in %.4f s\n",
	       speed, speed / 1024.0, elapsed);

	// 3. Bit-by-bit Verification
	int match = (memcmp(host_pinned, read_pinned, chunk_bytes) == 0);
	printf("\n[+] Data Integrity Proof: %s\n",
	       match ? "PASS (100% Bit-Exact Match, Zero Corruption)" : "FAIL");

	cuMemFreeHost(read_pinned);
	ret = match ? 0 : -EFAULT;

out_cleanup:
	// Cleanup
	cuMemFree(dev_ptr);
	cuMemFreeHost(host_pinned);
	cuCtxDestroy(ctx);
	dlclose(lib);

	printf("=================================================================\n");
	return ret;
}
