// SPDX-License-Identifier: MIT
/*
 * vram_free_probe - one-shot cuMemGetInfo reader for the GPU budget probe.
 *
 * Usage: vram-free-probe [interval_sec] [count]
 *
 * Prints free/total VRAM as reported by the CUDA driver's cuMemGetInfo, so a
 * sampler can compare it side by side with nvidia-smi and with RamShared's
 * cache-status.json budget while an external consumer (vram-ramp) holds device
 * memory. This isolates whether the allocator view tracks other processes:
 * if free memory does not drop while the consumer holds, the driver budget is
 * not a usable pressure signal for the containment chain.
 *
 * This probe only reads. It never allocates, never touches swap, and never
 * mutates RamShared state.
 */
#define _GNU_SOURCE

#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>

/* CUDA driver ABI types. Declared locally: the WSL image ships libcuda.so
 * without the development headers. cuMemGetInfo requires an active context
 * (CUDA_ERROR_INVALID_CONTEXT otherwise), so the probe owns one.
 */
struct CUctx_st;

#define CUDA_SUCCESS		0

static int (*p_cuInit)(unsigned int flags);
static int (*p_cuDeviceGet)(int *device, int ordinal);
static int (*p_cuCtxCreate)(struct CUctx_st **ctx, unsigned int flags,
			    int device);
static int (*p_cuCtxDestroy)(struct CUctx_st *ctx);
static int (*p_cuMemGetInfo)(unsigned long long *free_bytes,
			     unsigned long long *total_bytes);
static int (*p_cuGetErrorString)(int err, const char **str);

static int load_driver(void)
{
	void *h;
	const char *why = "unknown";
	int err;

	h = dlopen("libcuda.so.1", RTLD_NOW | RTLD_GLOBAL);
	if (!h)
		h = dlopen("libcuda.so", RTLD_NOW | RTLD_GLOBAL);
	if (!h) {
		fprintf(stderr, "[free] dlopen libcuda: %s\n", dlerror());
		return -1;
	}

#define LOAD(sym)								\
	do {									\
		*(void **)&p_##sym = dlsym(h, #sym);				\
		if (!p_##sym) {							\
			fprintf(stderr, "[free] missing symbol %s\n", #sym);	\
			return -1;						\
		}								\
	} while (0)

	LOAD(cuInit);
	LOAD(cuDeviceGet);
	LOAD(cuCtxCreate);
	LOAD(cuCtxDestroy);
	LOAD(cuMemGetInfo);
	LOAD(cuGetErrorString);
#undef LOAD

	err = p_cuInit(0);
	if (err != CUDA_SUCCESS) {
		p_cuGetErrorString(err, &why);
		fprintf(stderr, "[free] cuInit failed: %s (%d)\n", why, err);
		return -1;
	}
	return 0;
}

int main(int argc, char **argv)
{
	struct CUctx_st *ctx = NULL;
	unsigned int interval = 2, count = 8, i;
	unsigned long long free_bytes, total_bytes;
	const char *why = "unknown";
	struct timespec ts;
	int dev, err;

	if (argc > 3) {
		fprintf(stderr, "usage: %s [interval_sec] [count]\n", argv[0]);
		return 2;
	}
	if (argc >= 2)
		interval = (unsigned int)strtoul(argv[1], NULL, 10);
	if (argc >= 3)
		count = (unsigned int)strtoul(argv[2], NULL, 10);
	if (interval == 0 || count == 0) {
		fprintf(stderr, "[free] invalid interval/count\n");
		return 2;
	}

	if (load_driver() != 0)
		return 1;

	err = p_cuDeviceGet(&dev, 0);
	if (err != CUDA_SUCCESS) {
		p_cuGetErrorString(err, &why);
		fprintf(stderr, "[free] cuDeviceGet(0) failed: %s (%d)\n", why,
			err);
		return 1;
	}
	err = p_cuCtxCreate(&ctx, 0, dev);
	if (err != CUDA_SUCCESS) {
		p_cuGetErrorString(err, &why);
		fprintf(stderr, "[free] cuCtxCreate failed: %s (%d)\n", why, err);
		return 1;
	}

	for (i = 0; i < count; i++) {
		free_bytes = 0;
		total_bytes = 0;
		err = p_cuMemGetInfo(&free_bytes, &total_bytes);
		if (err != CUDA_SUCCESS) {
			p_cuGetErrorString(err, &why);
			fprintf(stderr, "[free] cuMemGetInfo failed: %s (%d)\n",
				why, err);
			p_cuCtxDestroy(ctx);
			return 1;
		}
		printf("[free] n=%u free_mib=%llu used_mib=%llu total_mib=%llu\n",
		       i, free_bytes / (1024ULL * 1024ULL),
		       (total_bytes - free_bytes) / (1024ULL * 1024ULL),
		       total_bytes / (1024ULL * 1024ULL));
		fflush(stdout);
		if (i + 1 < count) {
			ts.tv_sec = (time_t)interval;
			ts.tv_nsec = 0;
			while (nanosleep(&ts, &ts) == -1 && errno == EINTR)
				continue;
		}
	}
	p_cuCtxDestroy(ctx);
	return 0;
}
