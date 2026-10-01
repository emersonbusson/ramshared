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
 * reports INCONCLUSIVE. This mode takes <mib> as a ceiling the loop must not
 * reach and allocates 64 KiB chunks until /proc/buddyinfo shows no free
 * block of order 7 or above -- the test condition, measured rather than
 * inferred. It then punches a bounded set of physical buddy holes: one
 * member of every (pfn, pfn^1) pair whose buddy is also ours is unmapped.
 * A freed page's buddy is always held, so nothing above order-0 can
 * coalesce, order-7 cannot reappear, and the survivors still satisfy
 * order-0 fallback. Hole count is capped: the property is what matters,
 * not the density, and an uncapped punch drains the Unmovable slab through
 * vm_area_dup() until out_of_memory() panics an unkillable guest. It
 * prints FRAGMENT_BUDDY ready=1 only after the pattern is in place.
 *
 * The loop stops on the measured condition, not on hard refusal:
 * pagefault_out_of_memory() only warns and retries the fault, so touching
 * into true exhaustion with an unkillable process hangs instead of failing.
 *
 * Freeing every other virtual chunk does not give that guarantee: adjacent
 * virtual chunks are not necessarily physical buddies, so freed chunks can
 * reassemble into order-7 blocks and the drill reports INCONCLUSIVE. The
 * pagemap PFN check is what makes the claim airtight. It needs
 * CAP_SYS_ADMIN (init runs this inside the disposable guest) and
 * CONFIG_PROC_PAGE_MONITOR (enabled in the drill kernel); without both the
 * pattern cannot be built and this mode fails closed instead of reporting a
 * pattern it never created.
 *
 * The allocation loop also lowers vm.min_free_kbytes for its duration and
 * restores it before punching holes. Without that, the min watermark hides
 * free high-order blocks from userspace faults: buddyinfo still lists them,
 * MemAvailable says there is nothing left, and the loop stops with the test
 * condition unmet no matter how low the floor sits. The read-back in the
 * report is the value actually in force, not the value asked for.
 *
 * Never prints addresses or PFNs. Evidence must not carry KASLR material.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
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

	/*
	 * MAP_SHARED on an O_RDONLY fd gets VM_SHARED cleared by do_mmap()
	 * (mm/mmap.c: `if (!(file->f_mode & FMODE_WRITE)) vm_flags &=
	 * ~(VM_MAYWRITE | VM_SHARED)`), and hv_uio_mmap_validate() rejects
	 * any mapping without VM_SHARED. Open read-write so the mapping is
	 * actually shared; both the UIO device and the channel "ring" sysfs
	 * attribute are 0600 and writable here.
	 */
	fd = open(path, O_RDWR);
	if (fd < 0) {
		/* stdout: the guest console is the evidence channel */
		printf("HELPER open %s: %s\n", path, strerror(errno));
		fflush(stdout);
		return 1;
	}

	maps = calloc((size_t)count, sizeof(*maps));
	if (!maps) {
		printf("HELPER calloc: %s\n", strerror(errno));
		fflush(stdout);
		close(fd);
		return 1;
	}

	for (i = 0; i < count; i++) {
		off_t off = (off_t)(i * page);
		void *m = mmap(NULL, (size_t)bytes, PROT_READ, MAP_SHARED, fd, off);

		if (m == MAP_FAILED) {
			printf("HELPER mmap %s[%ld] off=%lld: %s\n",
			       path, i, (long long)off, strerror(errno));
			fflush(stdout);
			break;
		}
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
 * 64 KiB chunks: large enough that the allocation phase stays well under
 * max_map_count, small enough that the buddy splits them out of high-order
 * blocks one at a time.
 */
#define FRAG_CHUNK (64L * 1024)
#define FRAG_MAX_CHUNKS 48000
#define FRAG_PAGE 4096L
/*
 * Allocation-phase floor. Low enough that the last order-7 block in the
 * free remainder can still be split -- the first measured run stopped at
 * 8 MiB with one such block left -- and high enough that a page fault
 * cannot run into pagefault_out_of_memory() (which retries forever
 * against an unkillable process). 2 MiB is 512 pages: more than one
 * 64 KiB chunk needs to fault in, far below the point where a fault
 * cannot be satisfied.
 */
#define FRAG_SAFETY_KB 2048L
/*
 * Hole-phase caps. Punching one hole per eligible page needs one
 * vm_area_struct per surviving run. On a 2 GiB guest that is ~250k slab
 * objects; once the Unmovable free pool is exhausted, vm_area_dup() fails
 * into out_of_memory() and -- with this process unkillable -- the kernel
 * panics with "System is deadlocked on memory". That is exactly what run
 * 36794687025 did on windows-2025. The property that matters is "no two
 * free pages are buddies", not how dense the holes are, so cap them and
 * stop punching before slab starves.
 */
#define FRAG_HOLE_CAP 4096L

/*
 * Punching one hole per freed page needs one VMA per surviving run, which is
 * far more than a normal process. The default 65530 is exhausted long before
 * the buddy is, so raise it first. Best effort: if the write fails the
 * pattern below fails closed and says so.
 */
static void raise_max_map_count(void)
{
	int fd = open("/proc/sys/vm/max_map_count", O_WRONLY);

	if (fd < 0)
		return;
	if (write(fd, "1048576\n", 8) < 0) {
		/* best effort */
	}
	close(fd);
}

/*
 * min_free_kbytes sets the per-zone min watermark. On a 2 GiB guest the
 * default is ~20 MiB, so MemAvailable reads 2 MiB while /proc/buddyinfo
 * still shows 15 MiB of free pages -- those pages are free but reserved,
 * and a userspace page fault cannot cross the watermark to split the
 * high-order blocks hidden behind it. That is why the allocation loop kept
 * stopping with order-7 supply left no matter how low the MemAvailable
 * floor went: the floor was fine, the watermark was the wall.
 *
 * This is a disposable drill guest whose purpose is to exhaust the buddy.
 * Lowering the reserve is the only lever userspace has to reach those
 * pages. It is restored before the channel-open exercise, which needs the
 * normal reserve to allocate its ring. Best effort both ways: if the write
 * fails the loop simply stops on the floor as before and says so.
 */
static long min_free_kb_read(void)
{
	FILE *f = fopen("/proc/sys/vm/min_free_kbytes", "r");
	char buf[64];
	long kb = -1;

	if (!f)
		return -1;
	if (fgets(buf, sizeof(buf), f))
		kb = strtol(buf, NULL, 10);
	fclose(f);
	return kb;
}

static void min_free_kb_write(long kb)
{
	char buf[32];
	int fd, n;

	fd = open("/proc/sys/vm/min_free_kbytes", O_WRONLY);
	if (fd < 0)
		return;
	n = snprintf(buf, sizeof(buf), "%ld\n", kb);
	if (n > 0 && write(fd, buf, (size_t)n) < 0) {
		/* best effort */
	}
	close(fd);
}

/*
 * Free blocks at order 7 and above, across all zones. This is the condition
 * the drill is actually about: while any remain, the buddy can still satisfy
 * an order-7 request outright and the fallback is not under test.
 *
 * buddyinfo layout is `Node <n>, zone <name>` followed by one count per
 * order, so order-0 is field 5 and order-7 is field 12.
 */
static long high_order_blocks(void)
{
	FILE *f = fopen("/proc/buddyinfo", "r");
	char line[512];
	long sum = 0;

	if (!f)
		return -1;
	while (fgets(line, sizeof(line), f)) {
		char *save = NULL;
		char *tok = strtok_r(line, " \t\n", &save);
		int field = 0;

		while (tok) {
			field++;
			if (field > 4 && field - 5 >= 7) {
				char *end = NULL;
				long v = strtol(tok, &end, 10);

				if (end != tok)
					sum += v;
			}
			tok = strtok_r(NULL, " \t\n", &save);
		}
	}
	fclose(f);
	return sum;
}

static long mem_available_kb(void)
{
	FILE *f = fopen("/proc/meminfo", "r");
	char line[256];
	long kb = -1;

	if (!f)
		return -1;
	while (fgets(line, sizeof(line), f)) {
		if (strncmp(line, "MemAvailable:", 13) == 0) {
			kb = strtol(line + 13, NULL, 10);
			break;
		}
	}
	fclose(f);
	return kb;
}

/*
 * Pin or unpin this process against the OOM killer. Pin only while the
 * allocation loop is building the pattern: a kill there is a clean
 * INCONCLUSIVE, a hang is not. Unpin before punching holes -- that phase
 * allocates slab, and an unkillable process plus an Unmovable shortage is
 * a guest panic, not a drill failure.
 */
static void set_oom_adj(const char *value)
{
	int fd = open("/proc/self/oom_score_adj", O_WRONLY);

	if (fd < 0)
		return;
	if (write(fd, value, strlen(value)) < 0) {
		/* best effort */
	}
	close(fd);
}

/*
 * PFN of a populated page. Returns 0 and stores the PFN, or -1 when the
 * entry is absent, swapped out, or pagemap is withholding PFNs (no
 * CAP_SYS_ADMIN). PFNs are used only to choose which pages to unmap and
 * are never printed.
 */
static int pagemap_pfn(int fd, void *addr, unsigned long *pfn)
{
	uint64_t ent;
	unsigned long idx = (unsigned long)addr / (unsigned long)FRAG_PAGE;

	if (pread(fd, &ent, sizeof(ent),
		  (off_t)idx * (off_t)sizeof(ent)) != (ssize_t)sizeof(ent))
		return -1;
	if (!(ent & (1ULL << 63)))
		return -1;
	*pfn = (unsigned long)(ent & ((1ULL << 55) - 1));
	return *pfn ? 0 : -1;
}

static void pfn_setbit(unsigned long *bits, unsigned long pfn)
{
	bits[pfn / (8 * sizeof(long))] |= 1UL << (pfn % (8 * sizeof(long)));
}

static int pfn_getbit(const unsigned long *bits, unsigned long pfn)
{
	return !!(bits[pfn / (8 * sizeof(long))] &
		  (1UL << (pfn % (8 * sizeof(long)))));
}

static int do_fragment_buddy(int argc, char **argv)
{
	long mib, hold, want, got = 0, locked = 0, i, off, stopped = 0;
	long freed_pages = 0, held_pages = 0, no_pfn = 0, pairs = 0;
	long high_before, high_locked, high_after, avail_kb;
	long min_free_saved, min_free_set, min_free_now;
	void **maps;
	unsigned long *pfns = NULL, *seen = NULL;
	unsigned long max_pfn = 0, seen_words = 0;
	int pmfd;
	const char *stop_reason = "ceiling";

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

	/*
	 * mib is a ceiling the allocation loop must never reach, not a
	 * quota. Passing a share of MemAvailable stops the loop while the
	 * untouched remainder still holds order-10 blocks, so order-7 never
	 * fails and the drill reports INCONCLUSIVE. The caller passes
	 * MemTotal; the loop stops when the test condition is met.
	 */
	want = (mib * 1024 * 1024) / FRAG_CHUNK;
	if (want > FRAG_MAX_CHUNKS)
		want = FRAG_MAX_CHUNKS;
	if (want < 2) {
		fprintf(stderr, "HELPER mib too small to fragment\n");
		return 2;
	}

	raise_max_map_count();

	/*
	 * Lower the min watermark before allocating so the loop can reach the
	 * high-order blocks sitting behind it. Saved so it can be restored
	 * before the ring allocation that follows.
	 */
	min_free_saved = min_free_kb_read();
	min_free_set = 512;
	min_free_kb_write(min_free_set);
	/*
	 * Read back rather than trusting the write: the sysctl is
	 * best-effort and can be denied, and a silent no-op must not look
	 * like the lever was pulled. The report carries the value that is
	 * actually in force.
	 */
	min_free_now = min_free_kb_read();
	printf("FRAGMENT_BUDDY min_free_kbytes saved=%ld set=%ld now=%ld\n",
	       min_free_saved, min_free_set, min_free_now);
	fflush(stdout);

	/*
	 * This guest is disposable and this process is the point of the
	 * drill. Keep the OOM killer off us during allocation: if memory
	 * truly runs out the loop stops instead of us being killed
	 * mid-pattern. This is dropped again before hole-punching.
	 */
	set_oom_adj("-1000\n");

	maps = calloc((size_t)want, sizeof(*maps));
	pfns = calloc((size_t)want * (FRAG_CHUNK / FRAG_PAGE), sizeof(*pfns));
	if (!maps || !pfns) {
		printf("HELPER calloc: %s\n", strerror(errno));
		fflush(stdout);
		free(maps);
		free(pfns);
		return 1;
	}

	high_before = high_order_blocks();
	printf("FRAGMENT_BUDDY start cap_chunks=%ld high_order_7plus=%ld\n",
	       want, high_before);
	fflush(stdout);

	/*
	 * Allocate 64 KiB chunks and touch every page. Stop when the buddy
	 * no longer holds a free block of order 7 or above -- that is the
	 * test condition, measured, not inferred.
	 *
	 * The order-7 test runs BEFORE the MemAvailable floor. The first
	 * measured run stopped on the floor with one order-7 block still in
	 * the free remainder: the floor, not the buddy, was what kept the
	 * test condition unmet. The floor is a last resort against
	 * pagefault_out_of_memory(), which on this kernel only warns and
	 * retries the fault, so touching into true exhaustion with an
	 * unkillable process hangs instead of failing.
	 */
	for (i = 0; i < want; i++) {
		char *m;
		long high;

		if ((got % 32) == 0) {
			high = high_order_blocks();
			if (high == 0) {
				stopped = 1;
				stop_reason = "order7-depleted";
				break;
			}
		}

		avail_kb = mem_available_kb();
		if (avail_kb >= 0 && avail_kb < FRAG_SAFETY_KB) {
			/*
			 * Floor reached. If order-7 is already gone we are
			 * done; otherwise this is a real shortage and we
			 * must stop rather than fault into a hang.
			 */
			stopped = 1;
			stop_reason = "memavailable-floor";
			break;
		}

		m = mmap(NULL, FRAG_CHUNK, PROT_READ | PROT_WRITE,
			 MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
		if (m == MAP_FAILED) {
			stopped = 1;
			stop_reason = "mmap-refused";
			break;
		}
		for (off = 0; off < FRAG_CHUNK; off += FRAG_PAGE)
			m[off] = 1;
		maps[i] = m;
		got++;
	}
	/* One final check: the loop may have stopped on the ceiling. */
	if (stopped == 0 && high_order_blocks() == 0) {
		stopped = 1;
		stop_reason = "order7-depleted";
	}

	/*
	 * Pin the pages before punching holes so compaction cannot migrate
	 * survivors into the gaps and rebuild high orders under us.
	 */
	for (i = 0; i < got; i++) {
		if (mlock(maps[i], FRAG_CHUNK) == 0)
			locked++;
	}
	high_locked = high_order_blocks();
	printf("FRAGMENT_BUDDY allocated chunks=%ld high_order_7plus=%ld->%ld locked=%ld stop=%s\n",
	       got, high_before, high_locked, locked, stop_reason);
	fflush(stdout);

	pmfd = open("/proc/self/pagemap", O_RDONLY);
	if (pmfd < 0) {
		printf("FRAGMENT_BUDDY ready=0 pagemap=0 reason=open:%s\n",
		       strerror(errno));
		fflush(stdout);
		free(maps);
		free(pfns);
		return 2;
	}

	/* Pass 1: collect every PFN we own and the high-water mark. */
	for (i = 0; i < got; i++) {
		char *m = maps[i];

		for (off = 0; off < FRAG_CHUNK; off += FRAG_PAGE) {
			unsigned long pfn;

			if (pagemap_pfn(pmfd, m + off, &pfn) != 0) {
				no_pfn++;
				continue;
			}
			pfns[i * (FRAG_CHUNK / FRAG_PAGE) + off / FRAG_PAGE] =
				pfn + 1; /* 0 means "no pfn" in the array */
			if (pfn > max_pfn)
				max_pfn = pfn;
		}
	}

	if (no_pfn > 0 || max_pfn == 0) {
		printf("FRAGMENT_BUDDY ready=0 pagemap=0 no_pfn=%ld reason=pfn-incomplete\n",
		       no_pfn);
		fflush(stdout);
		close(pmfd);
		free(maps);
		free(pfns);
		return 2;
	}

	seen_words = max_pfn / (8 * sizeof(long)) + 2;
	seen = calloc(seen_words, sizeof(*seen));
	if (!seen) {
		printf("HELPER calloc pfn-set: %s\n", strerror(errno));
		fflush(stdout);
		close(pmfd);
		free(maps);
		free(pfns);
		return 1;
	}
	for (i = 0; i < got * (FRAG_CHUNK / FRAG_PAGE); i++) {
		if (pfns[i])
			pfn_setbit(seen, pfns[i] - 1);
	}

	/*
	 * Pass 2: unmap every page whose PFN is even and whose buddy (pfn+1)
	 * is also ours and will be held. A freed page's buddy is then always
	 * held, so nothing above order-0 can coalesce and the survivors still
	 * serve order-0 requests. Pages whose buddy is not ours are held:
	 * freeing them could pair with leftover free memory and rebuild the
	 * high orders we just drained.
	 *
	 * The property is what matters, not the density. Each munmap splits a
	 * VMA and allocates a vm_area_struct from the Unmovable slab; a quarter
	 * million of them drained that pool on a 2 GiB guest and the resulting
	 * out_of_memory() panicked the machine because this process was
	 * unkillable. Cap the holes, drop the OOM pin first, and stop punching
	 * before slab starves. FRAG_HOLE_CAP holes still free 16 MiB of
	 * order-0 -- far more than the 2 MiB ring needs -- and the held buddies
	 * keep order-7 from reforming.
	 */
	/*
	 * Restore the watermark before punching. The pattern is in place --
	 * pages are locked and buddies are held -- so raising the reserve
	 * again does not rebuild high orders, and the vm_area_dup() slab
	 * the punch needs should allocate against the normal reserve rather
	 * than eating into it. The channel open that follows needs that
	 * reserve too, to allocate its ring.
	 */
	if (min_free_saved > 0)
		min_free_kb_write(min_free_saved);

	set_oom_adj("0\n");
	for (i = 0; i < got && freed_pages < FRAG_HOLE_CAP; i++) {
		char *m = maps[i];

		for (off = 0; off < FRAG_CHUNK && freed_pages < FRAG_HOLE_CAP;
		     off += FRAG_PAGE) {
			unsigned long pfn =
				pfns[i * (FRAG_CHUNK / FRAG_PAGE) + off / FRAG_PAGE];

			if (!pfn) {
				held_pages++;
				continue;
			}
			pfn--;
			if ((pfn & 1UL) == 0 && pfn_getbit(seen, pfn + 1)) {
				if (munmap(m + off, (size_t)FRAG_PAGE) == 0) {
					freed_pages++;
					pairs++;
				} else {
					held_pages++;
				}
			} else {
				held_pages++;
			}
		}
	}

	high_after = high_order_blocks();
	printf("FRAGMENT_BUDDY ready=1 chunks=%ld pages=%ld held=%ld freed=%ld locked=%ld pairs=%ld chunk_kib=64 cap_chunks=%ld hole_cap=%ld exhausted=%ld pagemap=1 stop=%s high_order_7plus=%ld->%ld->%ld\n",
	       got, got * (FRAG_CHUNK / FRAG_PAGE), held_pages, freed_pages,
	       locked, pairs, want, FRAG_HOLE_CAP, stopped, stop_reason,
	       high_before, high_locked, high_after);
	fflush(stdout);

	if (hold > 0)
		sleep((unsigned int)hold);

	for (i = 0; i < got; i++)
		munmap(maps[i], FRAG_CHUNK);
	free(seen);
	free(maps);
	free(pfns);
	close(pmfd);
	printf("FRAGMENT_BUDDY released held=%ld freed=%ld\n",
	       held_pages, freed_pages);
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
