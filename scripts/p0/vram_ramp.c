// SPDX-License-Identifier: MIT
/*
 * vram_ramp - staged VRAM consumer for the GPU budget containment probe.
 *
 * Usage: vram-ramp <step_mib> <max_mib> <step_hold_sec> [final_hold_sec]
 *
 * Allocates <step_mib> of device memory at a time up to <max_mib>, commits
 * every byte with cuMemsetD8 so the driver really accounts it, and pauses
 * <step_hold_sec> after each step so an outside sampler can record how the
 * RamShared cache and the WDDM budget react as the ramp climbs. On reaching
 * the cap (or a driver out-of-memory) it holds <final_hold_sec> and then frees
 * everything, so the release direction can be observed too.
 *
 * This is a probe of the containment chain (driver budget -> safe cache target
 * -> demote), not a stress tool: it stops at its own cap and always frees on
 * exit. It never touches swap, never opens the cascade, and never mutates
 * RamShared state directly.
 */
#define _GNU_SOURCE

#include <dlfcn.h>
#include <errno.h>
#include <inttypes.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

/* CUDA driver API. Declared locally: the WSL image ships libcuda.so without
 * the development headers, and only these entry points are needed.
 */
/* CUDA driver ABI types. Used inline rather than typedef'd: the ABI is
 * `int` device, `struct CUctx_st *` context, `unsigned long long` pointer.
 */
struct CUctx_st;

#define CUDA_SUCCESS		0

static int (*p_cuInit)(unsigned int flags);
static int (*p_cuDeviceGet)(int *device, int ordinal);
static int (*p_cuCtxCreate)(struct CUctx_st **ctx, unsigned int flags,
			    int device);
static int (*p_cuCtxDestroy)(struct CUctx_st *ctx);
static int (*p_cuMemAlloc)(unsigned long long *dptr, size_t bytes);
static int (*p_cuMemFree)(unsigned long long dptr);
static int (*p_cuMemsetD8)(unsigned long long dst, unsigned char uc,
			   size_t n);
static int (*p_cuGetErrorString)(int err, const char **str);

static sig_atomic_t stop_requested;

static void on_stop(int sig)
{
	(void)sig;
	stop_requested = 1;
}

static void sleep_sec(unsigned int sec)
{
	struct timespec ts = {
		.tv_sec = (time_t)sec,
		.tv_nsec = 0,
	};

	while (!stop_requested && nanosleep(&ts, &ts) == -1 && errno == EINTR)
		continue;
}

static int load_driver(void)
{
	/*
	 * The driver library is already mapped by the WSL CUDA shim; dlopen of
	 * the soname is enough to resolve the entry points.
	 */
	void *h;
	const char *why = "unknown";
	int err;

	h = dlopen("libcuda.so.1", RTLD_NOW | RTLD_GLOBAL);
	if (!h)
		h = dlopen("libcuda.so", RTLD_NOW | RTLD_GLOBAL);
	if (!h) {
		fprintf(stderr, "[ramp] dlopen libcuda: %s\n", dlerror());
		return -1;
	}

#define LOAD(sym)								\
	do {									\
		*(void **)&p_##sym = dlsym(h, #sym);				\
		if (!p_##sym) {							\
			fprintf(stderr, "[ramp] missing symbol %s\n", #sym);	\
			return -1;						\
		}								\
	} while (0)

	LOAD(cuInit);
	LOAD(cuDeviceGet);
	LOAD(cuCtxCreate);
	LOAD(cuCtxDestroy);
	LOAD(cuMemAlloc);
	LOAD(cuMemFree);
	LOAD(cuMemsetD8);
	LOAD(cuGetErrorString);
#undef LOAD

	err = p_cuInit(0);
	if (err != CUDA_SUCCESS) {
		p_cuGetErrorString(err, &why);
		fprintf(stderr, "[ramp] cuInit failed: %s (%d)\n", why, err);
		return -1;
	}
	return 0;
}

int main(int argc, char **argv)
{
	struct CUctx_st *ctx = NULL;
	int dev;
	unsigned long long *slots;
	size_t step_bytes, total_bytes = 0;
	unsigned int step_mib, max_mib, step_hold, final_hold;
	unsigned long step, steps, allocated = 0;
	const char *why = "unknown";
	int err;

	if (argc < 4 || argc > 5) {
		fprintf(stderr,
			"usage: %s <step_mib> <max_mib> <step_hold_sec> [final_hold_sec]\n",
			argv[0]);
		return 2;
	}
	step_mib = (unsigned int)strtoul(argv[1], NULL, 10);
	max_mib = (unsigned int)strtoul(argv[2], NULL, 10);
	step_hold = (unsigned int)strtoul(argv[3], NULL, 10);
	final_hold = (argc == 5) ? (unsigned int)strtoul(argv[4], NULL, 10) : 10;

	if (step_mib == 0 || max_mib == 0 || max_mib < step_mib) {
		fprintf(stderr, "[ramp] invalid step/max\n");
		return 2;
	}
	steps = max_mib / step_mib;

	signal(SIGINT, on_stop);
	signal(SIGTERM, on_stop);

	if (load_driver() != 0)
		return 1;

	err = p_cuDeviceGet(&dev, 0);
	if (err != CUDA_SUCCESS) {
		fprintf(stderr, "[ramp] cuDeviceGet(0) failed: %d\n", err);
		return 1;
	}
	err = p_cuCtxCreate(&ctx, 0, dev);
	if (err != CUDA_SUCCESS) {
		fprintf(stderr, "[ramp] cuCtxCreate failed: %d\n", err);
		return 1;
	}

	slots = calloc(steps, sizeof(*slots));
	if (!slots) {
		p_cuCtxDestroy(ctx);
		return 1;
	}

	step_bytes = (size_t)step_mib * 1024UL * 1024UL;
	printf("[ramp] start step=%u MiB max=%u MiB hold=%us final=%us\n",
	       step_mib, max_mib, step_hold, final_hold);
	fflush(stdout);

	for (step = 0; step < steps && !stop_requested; step++) {
		err = p_cuMemAlloc(&slots[step], step_bytes);
		if (err != CUDA_SUCCESS) {
			if (p_cuGetErrorString)
				p_cuGetErrorString(err, &why);
			printf("[ramp] stop at step=%lu: cuMemAlloc %u MiB failed (%s, %d)\n",
			       step, step_mib, why, err);
			fflush(stdout);
			break;
		}
		/* Commit the pages so the driver accounts them as resident. */
		if (p_cuMemsetD8(slots[step], (unsigned char)(0xA5 + step),
				 step_bytes) != CUDA_SUCCESS) {
			printf("[ramp] warn: cuMemsetD8 failed at step=%lu\n", step);
			fflush(stdout);
		}
		allocated++;
		total_bytes += step_bytes;
		printf("[ramp] step=%lu allocated=%lu MiB total=%zu MiB\n",
		       step, (unsigned long)((step + 1) * step_mib),
		       total_bytes / (1024UL * 1024UL));
		fflush(stdout);
		sleep_sec(step_hold);
	}

	printf("[ramp] peak held: %zu MiB over %lu steps; holding %us before free\n",
	       total_bytes / (1024UL * 1024UL), allocated, final_hold);
	fflush(stdout);
	sleep_sec(final_hold);

	for (step = 0; step < allocated; step++)
		p_cuMemFree(slots[step]);

	printf("[ramp] freed %lu MiB; exit\n",
	       total_bytes / (1024UL * 1024UL));
	fflush(stdout);

	free(slots);
	p_cuCtxDestroy(ctx);
	return 0;
}
