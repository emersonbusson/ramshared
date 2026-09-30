// SPDX-License-Identifier: GPL-2.0
/*
 * vmbus_drill_helper.c - static helper for the VMBus runtime drills.
 *
 * The drills used to embed python3 heredocs for two primitives: holding a UIO
 * mapping alive while the caller tears the channel down, and locking a large
 * anonymous region to fragment the buddy allocator. A disposable drill guest
 * has no CPython and must not grow one, so both live here as one statically
 * linked binary. This is a Day-0 replacement, not a shim: the drills take a
 * single dependency and the initramfs stays free of an interpreter.
 *
 * Usage:
 *   vmbus_drill_helper mmap-hold <path> <bytes> <hold_seconds> [count]
 *   vmbus_drill_helper mlock-hog <mib> <hold_seconds>
 *   vmbus_drill_helper fragment-buddy <mib> <hold_seconds>
 *
 * mmap-hold maps <count> regions of <bytes> at sequential page-aligned
 * offsets and keeps them alive for <hold_seconds>. UIO map N lives at offset
 * N * pagesize, so this is exactly the UIO ABI. Count defaults to 1 and is
 * how the sysfs ring file is mapped whole.
 *
 * mlock-hog allocates <mib> MiB anonymously, populates every page, locks it
 * with mlock(2) and holds for <hold_seconds>. Locked pages resist compaction
 * and drain high-order free blocks. It prints MLOCK_HOG ready=1 only after
 * the lock succeeds.
 *
 * fragment-buddy is the one the order-zero drill actually needs. A single
 * large hog splits the buddy only as far as it must and leaves the untouched
 * remainder as high-order blocks, so order-7 still succeeds and the drill
 * reports INCONCLUSIVE. This mode instead takes <mib> as many independent
 * 64 KiB chunks, then frees every other chunk and mlocks the rest. No two
 * neighbouring chunks are free at once, so nothing above order-4 can coalesce,
 * order-7 must fail, and the freed chunks still satisfy order-0 fallback.
 * It prints FRAGMENT_BUDDY ready=1 only after the pattern is in place.
 *
 * Never prints addresses. Evidence must not carry KASLR material.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

static long parse_long(const char *s, const char *what)
{
	char *end = NULL;
	long v;

	errno = 0;
	v = strtol(s, &end, 10);
	if (errno || end == s || *end != '\0' || v < 0) {
		fprintf(stderr, "HELPER bad %s: '%s'\n", what, s);
		exit(2);
	}
	return v;
}

static int do_mmap_hold(int argc, char **argv)
{
	const char *path;
	long bytes, hold, count, i, mapped = 0;
	long page;
	int fd;
	void **maps;

	if (argc < 4) {
		fprintf(stderr,
			"usage: mmap-hold <path> <bytes> <hold_seconds> [count]\n");
		return 2;
	}
	path = argv[1];
	bytes = parse_long(argv[2], "bytes");
	hold = parse_long(argv[3], "hold_seconds");
	count = (argc > 4) ? parse_long(argv[4], "count") : 1;

	page = sysconf(_SC_PAGESIZE);
	if (page <= 0)
		page = 4096;
	if (bytes <= 0 || bytes > (64L * 1024 * 1024)) {
		fprintf(stderr, "HELPER bytes out of range\n");
		return 2;
	}
	if (count <= 0 || count > 64) {
		fprintf(stderr, "HELPER count out of range\n");
		return 2;
	}

	fd = open(path, O_RDONLY);
	if (fd < 0) {
		fprintf(stderr, "HELPER open %s: %s\n", path, strerror(errno));
		return 1;
	}

	maps = calloc((size_t)count, sizeof(*maps));
	if (!maps) {
		fprintf(stderr, "HELPER calloc: %s\n", strerror(errno));
		close(fd);
		return 1;
	}

	for (i = 0; i < count; i++) {
		off_t off = (off_t)(i * page);
		void *m = mmap(NULL, (size_t)bytes, PROT_READ, MAP_SHARED, fd, off);

		if (m == MAP_FAILED)
			break;
		maps[i] = m;
		mapped++;
	}

	printf("MMAP_HOLD path=%s bytes=%ld maps=%ld hold=%ld\n",
	       path, bytes, mapped, hold);
	fflush(stdout);

	/*
	 * Hold the mappings alive across the caller's teardown. This is the
	 * BUG-3 window: the ring is freed while a userspace mapping still
	 * references it. The unbind races this sleep on purpose.
	 */
	if (hold > 0)
		sleep((unsigned int)hold);

	for (i = 0; i < mapped; i++)
		munmap(maps[i], (size_t)bytes);
	free(maps);
	close(fd);
	printf("MMAP_HOLD released maps=%ld\n", mapped);
	fflush(stdout);
	return mapped > 0 ? 0 : 1;
}

static int do_mlock_hog(int argc, char **argv)
{
	long mib, hold;
	size_t len, off;
	char *buf;

	if (argc < 3) {
		fprintf(stderr, "usage: mlock-hog <mib> <hold_seconds>\n");
		return 2;
	}
	mib = parse_long(argv[1], "mib");
	hold = parse_long(argv[2], "hold_seconds");
	if (mib <= 0 || mib > (long)(16 * 1024)) {
		fprintf(stderr, "HELPER mib out of range\n");
		return 2;
	}

	len = (size_t)mib * 1024UL * 1024UL;
	buf = mmap(NULL, len, PROT_READ | PROT_WRITE,
		   MAP_PRIVATE | MAP_ANONYMOUS | MAP_POPULATE, -1, 0);
	if (buf == MAP_FAILED) {
		fprintf(stderr, "HELPER mmap %ld MiB: %s\n",
			mib, strerror(errno));
		return 1;
	}

	/* Touch every page so none of it stays unpopulated and lazily faulted. */
	for (off = 0; off < len; off += 4096)
		buf[off] = 1;

	if (mlock(buf, len) != 0) {
		fprintf(stderr, "HELPER mlock: %s\n", strerror(errno));
		munmap(buf, len);
		return 1;
	}

	printf("MLOCK_HOG ready=1 mib=%ld\n", mib);
	fflush(stdout);

	if (hold > 0)
		sleep((unsigned int)hold);

	munlock(buf, len);
	munmap(buf, len);
	printf("MLOCK_HOG released mib=%ld\n", mib);
	fflush(stdout);
	return 0;
}

/*
 * Take <mib> as independent 64 KiB chunks, then free every other one and
 * mlock the survivors. A free chunk's buddy is always held, so the free list
 * tops out at order-4 and order-7 cannot form. The freed chunks keep order-0
 * available, which is exactly the state the fallback has to survive.
 *
 * 64 KiB keeps the VMA count well under the default max_map_count even at
 * several GiB: 4 KiB chunks would need one VMA per page and hit the limit
 * after ~65k of them.
 */
#define FRAG_CHUNK (64L * 1024)
#define FRAG_MAX_CHUNKS 32768

static int do_fragment_buddy(int argc, char **argv)
{
	long mib, hold, want, got = 0, freed = 0, locked = 0, i, off;
	void **maps;

	if (argc < 3) {
		fprintf(stderr, "usage: fragment-buddy <mib> <hold_seconds>\n");
		return 2;
	}
	mib = parse_long(argv[1], "mib");
	hold = parse_long(argv[2], "hold_seconds");
	if (mib <= 0 || mib > (long)(16 * 1024)) {
		fprintf(stderr, "HELPER mib out of range\n");
		return 2;
	}

	want = (mib * 1024 * 1024) / FRAG_CHUNK;
	if (want > FRAG_MAX_CHUNKS)
		want = FRAG_MAX_CHUNKS;
	if (want < 2) {
		fprintf(stderr, "HELPER mib too small to fragment\n");
		return 2;
	}

	maps = calloc((size_t)want, sizeof(*maps));
	if (!maps) {
		fprintf(stderr, "HELPER calloc: %s\n", strerror(errno));
		return 1;
	}

	for (i = 0; i < want; i++) {
		char *m = mmap(NULL, FRAG_CHUNK, PROT_READ | PROT_WRITE,
			       MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);

		if (m == MAP_FAILED)
			break;
		for (off = 0; off < FRAG_CHUNK; off += 4096)
			m[off] = 1;
		maps[i] = m;
		got++;
	}

	/* Free every other chunk. Each freed chunk's buddy stays allocated. */
	for (i = 0; i < got; i += 2) {
		munmap(maps[i], FRAG_CHUNK);
		maps[i] = NULL;
		freed++;
	}

	/* Pin the survivors so compaction cannot reassemble high orders. */
	for (i = 1; i < got; i += 2) {
		if (maps[i] && mlock(maps[i], FRAG_CHUNK) == 0)
			locked++;
	}

	printf("FRAGMENT_BUDDY ready=1 chunks=%ld held=%ld freed=%ld locked=%ld chunk_kib=64\n",
	       got, got - freed, freed, locked);
	fflush(stdout);

	if (hold > 0)
		sleep((unsigned int)hold);

	for (i = 1; i < got; i += 2) {
		if (maps[i]) {
			munlock(maps[i], FRAG_CHUNK);
			munmap(maps[i], FRAG_CHUNK);
		}
	}
	free(maps);
	printf("FRAGMENT_BUDDY released held=%ld\n", got - freed);
	fflush(stdout);
	return got > 1 ? 0 : 1;
}

static void usage(void)
{
	fprintf(stderr,
		"usage: vmbus_drill_helper mmap-hold <path> <bytes> <hold_seconds> [count]\n"
		"       vmbus_drill_helper mlock-hog <mib> <hold_seconds>\n"
		"       vmbus_drill_helper fragment-buddy <mib> <hold_seconds>\n");
}

int main(int argc, char **argv)
{
	if (argc < 2) {
		usage();
		return 2;
	}
	if (strcmp(argv[1], "mmap-hold") == 0)
		return do_mmap_hold(argc - 1, argv + 1);
	if (strcmp(argv[1], "mlock-hog") == 0)
		return do_mlock_hog(argc - 1, argv + 1);
	if (strcmp(argv[1], "fragment-buddy") == 0)
		return do_fragment_buddy(argc - 1, argv + 1);
	usage();
	return 2;
}
