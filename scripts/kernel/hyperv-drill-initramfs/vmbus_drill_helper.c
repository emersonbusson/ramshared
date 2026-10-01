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
 * The loop stops on the measured condition, not on hard refusal, and keeps
 * a real free-memory margin while doing it: pagefault_out_of_memory() only
 * warns and retries the fault, so touching into true exhaustion is how a
 * drill turns into either a hang or -- with an unkillable process -- "System
 * is deadlocked on memory" and a guest panic.
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
 * The allocation loop also lowers vm.min_free_kbytes and keeps it low
 * through the punch, the chase and the ready=1 measurement. The min
 * watermark hides free high-order blocks from userspace faults: buddyinfo
 * still lists them, but a fault cannot cross the reserve to split them, so
 * the loop stops with the test condition unmet however much room the floor
 * leaves. The read-back in the report is the value actually in force, not
 * the value asked for. It is restored only after ready=1, so the
 * channel-open exercise that follows runs against the normal reserve.
 *
 * The OOM pin is on only while the finished pattern is being held, never
 * while this process is allocating. Both wrong windows have been tried and
 * both are recorded. Pinning the allocation let out_of_memory() find no
 * killable victim once the drill shell had been reclaimed and the guest
 * panicked with "System is deadlocked on memory". Dropping the pin the
 * moment ready=1 printed let the drill shell's next fork reclaim this
 * process instead -- 2 GiB of locked pages came back, the high orders
 * reassembled, and the script measured 356 blocks where the helper had
 * just measured 1. So: allocate, punch and chase with the pin off (a kill
 * there is an honest INCONCLUSIVE_NO_PATTERN and the guest survives), pin
 * once ready=1 has been printed, and hold that pin for the whole hold so
 * the channel-open exercise runs against a pattern that is still in force.
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
 * Allocation-phase floor, measured against MemFree and only consulted while
 * order-7 remains: the test condition itself is the stop once it is met, so
 * a floor has nothing left to guard.
 *
 * MemFree, not MemAvailable. MemAvailable is a conservative estimate that
 * subtracts the watermark and unreclaimable state, so it reads ~0 while
 * buddyinfo still lists allocatable high-order blocks. A floor on that
 * estimate stops the loop with the test condition unmet -- which is what
 * runs 36798464398 and 36800977305 both did, at 2 MiB and at 256 kB. The
 * 256 kB figure then drove the remaining faults into
 * pagefault_out_of_memory() with this process pinned, and the guest panicked
 * instead of failing the drill.
 *
 * 96 MiB is the margin the rest of the run needs to stay alive: the chase,
 * the punch, the drill shell's forks, and the ring allocation the
 * channel-open exercise is about to make. Anything the main loop leaves
 * above that is still holding high-order blocks, and the chase below is what
 * splits them.
 *
 * The level matters because the chase's peak RSS is main_pattern + held.
 * Run 36813796538 stopped at a 32 MiB margin and the OOM killer took the
 * helper at anon-rss 1950 MiB on a 2048 MiB guest: the main pattern held
 * 1920 MiB after the punch, the chase needed ~48 MiB more to drain the
 * 16 MiB of punch fuel and split the remaining high-order blocks, and there
 * was not 48 MiB left. Raising the margin shrinks the main pattern by
 * exactly the amount it grows the high-order residue, so the margin alone
 * does not create headroom -- the chase recycle below does. The margin is
 * the room the recycle works in, not a reserve the chase is kept out of.
 *
 * Run 36823337153 measured the residue that recycle has to pay for:
 * buddyinfo at the chase stop read 0 1 0 1 0 1 1 2 0 1 10, so the 13
 * remaining high-order blocks are ten order-10, one order-9 and two
 * order-7 -- 11008 free pages, 43 MiB, of which 40 MiB sit in order-10.
 * At full hold that is 43 MiB of chase RSS on top of a 1853 MiB main
 * pattern, past the 1950 MiB ceiling. Recycle halves the net cost.
 */
#define FRAG_HARD_KB 98304L
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
 * Chase phase: split the high-order blocks the main loop's margin left
 * behind. The punch has just freed FRAG_HOLE_CAP isolated order-0 pages, so
 * there is room to allocate again without touching the floor; and because
 * those holes cannot form high orders, further faults must come out of the
 * remaining order-7-and-up blocks. That is the split we want.
 *
 * The floor is counted in unsplit free pages -- the order-0 buddy lists plus
 * the per-cpu page cache -- which is the only free-memory figure that means
 * what a fault can actually take without splitting something. Neither
 * /proc/meminfo nor /proc/buddyinfo reports it. MemFree is most of a
 * still-free high-order block (run 36803317912 stopped at 7.5 MiB with 7.0 of
 * that inside six order-7-and-up blocks), so a MemFree floor fires exactly
 * when the split it is supposed to allow has not happened yet. Both files
 * also omit the pcp, which is where a freshly freed page lands, so a floor on
 * either raw count fires right after the punch that is supposed to make room
 * -- run 36806229800 punched 4096 pages and buddyinfo still read 1, and run
 * 36808657969 got chase=0 from a "MemFree minus the high orders" floor that
 * is algebraically buddyinfo's order-0 again. See unsplit_free_pages().
 *
 * The level of that floor matters as much as the quantity. Run 36811867648
 * stopped at unsplit=504 (windows-2025) and 498 (windows-latest) with
 * high_order_7plus still at 9 and 8, while buddyinfo held 6101 and 3658
 * free pages in total -- most of it in the very blocks the chase exists to
 * split, including five order-10 blocks on windows-2025. Unsplit excludes
 * high orders, so it read as starvation while the guest still had 24 MiB.
 * The chase had just finished draining the pcp (254 and 405 iterations) and
 * was at the point where the next fault starts splitting; the floor stopped
 * it there, which is why only one or three high-order blocks were consumed.
 *
 * Splitting refills unsplit (an order-10 yields 1024 order-0 pages; a
 * 64 KiB chase chunk takes 16 and leaves 1008), so while any block above
 * order-0 remains the floor cannot bind -- it only binds when there is
 * nothing left to split. A low floor is therefore a real starvation guard
 * and still lets the split cycle run until high==0, which is what
 * exhausted=1 measures.
 *
 * FRAG_CHASE_UNSPLIT is one order-0 page count of the watermark the drill
 * has already set (min_free_kbytes=512 KiB = 128 pages), not a separate
 * reservation on top of it.
 *
 * FRAG_CHASE_CAP is the backstop, not the guard. The chase stops on the
 * measured condition first, then on the unsplit floor, then on the memfree
 * safety stop below, and only then on the cap. The chase holds every chunk
 * now: 4096 * 64 KiB is 256 MiB of headroom against the ~80 MiB the memfree
 * floor below will actually allow on a 2 GiB guest, so the floor binds
 * first and the cap is pure backstop. It is not a guess and it is not a
 * lever on the residue.
 *
 * The recycle is gone. It was anti-correlated with the goal across six
 * runs: every page returned as isolated order-0 is a page the next fault
 * takes instead of splitting, because __rmqueue_smallest drains order-0
 * before it ever looks at a higher order. 36823337153 (no recycle) held
 * 43.75 MiB and left 43 MiB of residue; 36883261849 (10.15 holes/chunk)
 * held 73.7 MiB and left 14 MiB; 36887735012 (15.68 holes/chunk, the
 * * 8 cap de-truncated) held 21 MiB and left 66 MiB, with order-10
 * blocks 2/3 -> 16/16 -- the high-order blocks were never touched. More
 * recycle, less hold, bigger residue, every time. Holding is what splits.
 * The recycle's proper place is phase 2, where it makes room for the ring,
 * not phase 1, where it exists to force splits.
 *
 * Watermark boost is the real limiter, not the floor constant. Run
 * 36899458556 drove the chase floor off the live min+boost watermark and
 * made things worse: fragmenting is exactly what boost_watermark() reacts
 * to, so the floor chased a rising target. windows-latest OOMed its own
 * helper at `free:7024kB boost:6924kB min:7436kB` -- min is already
 * 512+6924 -- and windows-2025 panicked on init's clone() with
 * `free:12588kB` all order-0, so an order-1 GFP_KERNEL had nothing to
 * split. Suppressing boost is what makes a low floor meaningful:
 * watermark_boost_factor=0 calls setup_per_zone_wmarks(), which zeroes
 * zone->watermark_boost and keeps min at the value we wrote. The floor is
 * driven off min_free_set -- the number this run asked for -- and not off
 * live_min_watermark_kb(), whose sum includes whatever boost is standing
 * and therefore chases a rising target. Both sysctls are read back and
 * fail closed: run 36879744347 killed two guests on a silent
 * min_free_kbytes write that never took. Run 36906731699 proved the low
 * floor is reachable (order-7+ down to a single block) and that restoring
 * the watermarks before the hold is what kills the guest.
 *
 * The residue does not have to be held. __rmqueue_smallest serves order-0
 * first, so the chase must empty the no-split budget (unsplit) before it
 * will touch an order-10; once order-0 is empty, clearing one order-10
 * down to order-6-and-below costs 15 held pages (1+2+4+8) and the rest of
 * the block stays free as order-6 and below -- exactly the state
 * high_order_7plus=0 asks for. Runs 36892314395 (16 MiB floor) and
 * 36894869321 (8 MiB floor) left 2816 and 1408/1280 pages standing
 * because the MemFree floor fired while unsplit was still the allocator's
 * preferred source. Estimated further hold to finish is unsplit plus ~25
 * pages of forced splits, about 3 MiB.
 */
#define FRAG_CHASE_CAP 4096L
#define FRAG_CHASE_UNSPLIT 128L
#define FRAG_CHASE_HARD_KB 1024L
#define FRAG_CHASE_WMARK_MARGIN 256L
/*
 * Order-2 reserve. Taken before the pattern is built, while the buddy
 * still hands out contiguous runs, and released as verified order-2
 * groups just before ready=1 is printed.
 *
 * Run 36911059487 is what a fully starved buddy costs. The pattern
 * emptied every free block of order 2 and above along with order 7 --
 * the OOM dump read `1451*4kB (UM) 0*8kB 0*16kB` -- so copy_process()
 * could not get its order-2 stack, the OOM killer took the drill shell
 * (this process is pinned at -1000 and unkillable), init then found no
 * killable process and the guest panicked on "System is deadlocked on
 * memory". windows-2025 died the same way and lost its FRAGMENT_BUDDY
 * lines to the panic.
 *
 * The measured claim is order-7-and-up, not order-2. Leaving a few
 * order-2 blocks does not weaken it and does not help the ring path:
 * on ordinary x86_64 vmbus_alloc_buffer() takes vzalloc() anyway. The
 * groups are released just before ready=1, so the shell's first
 * fork finds stacks while the buddy at the moment of the claim
 * still holds no free block of order 7 or above -- order-2 is
 * invisible to that count and cannot coalesce past order-2.
 *
 * mmap faults order-0, so held pages are physically scattered and an
 * order-2 block is found by PFN, not assumed from a virtual run. Each
 * released group is eight consecutive PFNs on a 4-page alignment: the
 * first four are unmapped, the last four are kept. The kept half is
 * the freed half's order-2 buddy, so the freed half stays order-2 and
 * cannot coalesce past it -- the order-7 depletion just measured is
 * not undone. Cap the groups like the holes: each munmap splits a VMA.
 */
#define FRAG_RESERVE_CHUNKS 48
#define FRAG_ORDER2_GROUPS 32

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
 * watermark_boost_factor scales zone->_watermark_boost. Every free that
 * cannot coalesce raises the watermark by that factor of the fragmented
 * gap, which is precisely the pattern this drill builds: run 36899458556
 * measured boost:6924kB on a guest whose min_free_kbytes was 512, so the
 * effective watermark sat at 7436 kB and an order-0 fault OOMed the
 * helper mid-chase. 0 disables the boost entirely and is the only way a
 * floor below the boosted watermark is reachable. Restored after ready=1
 * like min_free_kbytes, because the ring allocation that follows wants
 * the guest's normal anti-fragmentation behaviour.
 */
static long boost_factor_read(void)
{
	FILE *f = fopen("/proc/sys/vm/watermark_boost_factor", "r");
	char buf[64];
	long v = -1;

	if (!f)
		return -1;
	if (fgets(buf, sizeof(buf), f))
		v = strtol(buf, NULL, 10);
	fclose(f);
	return v;
}

static void boost_factor_write(long v)
{
	char buf[32];
	int fd, n;

	fd = open("/proc/sys/vm/watermark_boost_factor", O_WRONLY);
	if (fd < 0)
		return;
	n = snprintf(buf, sizeof(buf), "%ld\n", v);
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

/*
 * Free memory as the kernel counts it, not as MemAvailable estimates it.
 * MemAvailable subtracts the watermark and unreclaimable state and so reads
 * near zero while buddyinfo still holds allocatable high-order blocks; a
 * floor on it stops the loop with the test condition unmet.
 *
 * MemFree is not the pcp either: it tracks the buddy free lists only, the
 * same set buddyinfo reports, and both omit the per-cpu page cache. That is
 * why the chase floor is computed in unsplit_free_pages() rather than read
 * straight off this file.
 */
static long mem_free_kb(void)
{
	FILE *f = fopen("/proc/meminfo", "r");
	char line[256];
	long kb = -1;

	if (!f)
		return -1;
	while (fgets(line, sizeof(line), f)) {
		if (strncmp(line, "MemFree:", 8) == 0) {
			kb = strtol(line + 8, NULL, 10);
			break;
		}
	}
	fclose(f);
	return kb;
}

/*
 * The watermark the allocator will actually refuse below, in kB: every
 * zone's `min` plus its `boost`, from /proc/zoneinfo. Both are in pages.
 *
 * `boost` is the number to watch. boost_watermark() raises it under
 * fragmentation -- which is the whole point of this drill -- so the
 * watermark is not a fixed multiple of min_free_kbytes. Run 36879744347
 * saw it go 0 -> 4096 kB (windows-latest) and 0 -> 12288 kB
 * (windows-2025) after the first OOM. A floor that ignores boost is the
 * same class of mistake as a floor below min_free_kbytes.
 */
static long live_min_watermark_kb(void)
{
	FILE *f = fopen("/proc/zoneinfo", "r");
	char line[256];
	long min_pages = 0, boost_pages = 0, saw = 0;

	if (!f)
		return -1;
	while (fgets(line, sizeof(line), f)) {
		char *p = line;

		while (*p == ' ' || *p == '\t')
			p++;
		if (strncmp(p, "min", 3) == 0 &&
		    (p[3] == ' ' || p[3] == '\t')) {
			min_pages += strtol(p + 3, NULL, 10);
			saw++;
		} else if (strncmp(p, "boost", 5) == 0 &&
			   (p[5] == ' ' || p[5] == '\t')) {
			boost_pages += strtol(p + 5, NULL, 10);
		}
	}
	fclose(f);
	if (!saw)
		return -1;
	return (min_pages + boost_pages) * 4;
}

/*
 * Physical page count as MemTotal describes it. Used only to size the
 * held-page bitmap: the chase takes its pages from the leftover region,
 * above the main pattern's max_pfn, and a bitmap sized from max_pfn alone
 * would be overrun the first time a chase page sat higher. PFNs are never
 * printed.
 */
static long mem_total_pages(void)
{
	FILE *f = fopen("/proc/meminfo", "r");
	char line[256];
	long kb = -1;

	if (!f)
		return -1;
	while (fgets(line, sizeof(line), f)) {
		if (strncmp(line, "MemTotal:", 9) == 0) {
			kb = strtol(line + 9, NULL, 10);
			break;
		}
	}
	fclose(f);
	return kb < 0 ? -1 : kb / 4;
}

/*
 * Free pages at order 0 on the buddy free lists, across all zones.
 *
 * buddyinfo field 5 is order 0; see high_order_blocks() for the layout.
 * This is only half of the unsplit figure -- the per-cpu page cache is not
 * on these lists and is counted separately by pcp_free_pages().
 */
static long buddy_order0_pages(void)
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
			if (field == 5) {
				char *end = NULL;
				long v = strtol(tok, &end, 10);

				if (end != tok && v > 0)
					sum += v;
			}
			tok = strtok_r(NULL, " \t\n", &save);
		}
	}
	fclose(f);
	return sum;
}

/*
 * Free pages sitting in the per-cpu page cache, summed over every cpu and
 * zone. Reported by /proc/zoneinfo's `pagesets` block as pcp->count.
 *
 * This is the quantity neither MemFree nor buddyinfo reports. A freed page
 * goes to the pcp first -- free_frozen_page_commit() adds it to pcp->count
 * and never calls account_freepages() -- and NR_FREE_PAGES is bumped only
 * when the pcp drains into the buddy. So a page freed by munmap is free,
 * and invisible to both /proc files, until something else drains it.
 */
static long pcp_free_pages(void)
{
	FILE *f = fopen("/proc/zoneinfo", "r");
	char line[256];
	long sum = 0;
	int in_pagesets = 0;

	if (!f)
		return -1;
	while (fgets(line, sizeof(line), f)) {
		char *p = line;

		if (strncmp(line, "Node ", 5) == 0) {
			in_pagesets = 0;
			continue;
		}
		if (strncmp(line, "  pagesets", 10) == 0) {
			in_pagesets = 1;
			continue;
		}
		if (!in_pagesets)
			continue;
		while (*p == ' ' || *p == '\t')
			p++;
		if (strncmp(p, "count:", 6) == 0) {
			char *end = NULL;
			long v = strtol(p + 6, &end, 10);

			if (end != p + 6 && v > 0)
				sum += v;
		}
	}
	fclose(f);
	return sum;
}

/*
 * Free pages a fault can take without splitting a higher-order block: the
 * order-0 buddy lists plus the per-cpu page cache. This is the floor the
 * chase is allowed to leave behind.
 *
 * A fault tries the pcp first and then the order-0 buddy lists; only when
 * both are empty does the allocator split a higher-order block. So this sum
 * is exactly the no-split budget, and it is not what any single /proc file
 * reports. MemFree equals the buddy lists and omits the pcp entirely;
 * buddyinfo's own order-0 count omits the pcp as well. Run 36806229800
 * punched 4096 isolated pages, which went to the pcp, and buddyinfo still
 * read 1 -- a floor on either raw figure fires right after the punch that is
 * supposed to make room. Run 36808657969 repeated that with a
 * "MemFree minus the high orders" floor, which is algebraically just
 * buddyinfo's order-0 again, and got chase=0 for the same reason.
 *
 * The number rises whenever the allocator splits a high order (order-10 ->
 * 1024 order-0 pages, take 16, +1008), so the floor cannot fire while high
 * orders remain. It is a starvation guard; high==0 is what ends the chase.
 */
static long unsplit_free_pages(void)
{
	long order0 = buddy_order0_pages();
	long pcp = pcp_free_pages();

	if (order0 < 0 || pcp < 0)
		return -1;
	return order0 + pcp;
}

/*
 * Pin or unpin this process against the OOM killer. The pin covers exactly
 * the hold: the window in which a finished pattern must survive the drill
 * shell's measurement and the channel-open exercise. It is deliberately off
 * before that -- while this process allocates -- because an unkillable fault
 * in a guest with nothing else left to reclaim is how out_of_memory() reaches
 * "no killable processes" and panics. It is dropped when the hold ends, so
 * the teardown can be reclaimed like any other process.
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
	long high_before, high_locked, high_after, free_kb = -1;
	long wmark_kb = -1, floor_kb = -1;
	long min_free_saved, min_free_set, min_free_now;
	long boost_saved, boost_now;
	long chase = 0, chase_locked = 0;
	long chase_holes = 0, chase_pairs = 0;
	long unsplit_at_stop = -1;
	void **maps, **chase_maps;
	void *reserve[FRAG_RESERVE_CHUNKS];
	long reserve_got = 0, order2_groups = 0, order2_pages = 0;
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
	 * quota. Passing a share of what looks free stops the loop while the
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
	if (min_free_now != min_free_set) {
		/*
		 * Fail closed. Run 36879744347 is what an unverified write
		 * costs: min_free_kbytes stayed at its saved ~5694 kB, the
		 * MemFree floor never bound (buddy free 5376 kB > the 4 MiB
		 * floor, with 15 MiB stranded in pcp), an order-1 GFP_KERNEL
		 * from sh hit the un-lowered watermark, and both guests died.
		 * No floor below ~6 MiB is meaningful unless the watermark
		 * is actually the one this run asked for.
		 */
		printf("FRAGMENT_BUDDY ready=0 pagemap=0 reason=min-free-not-lowered\n");
		fflush(stdout);
		return 3;
	}

	/*
	 * Same discipline for boost. min_free_kbytes alone is not the
	 * watermark: zone->_watermark_boost is added on top of it, and
	 * fragmenting is what makes boost move. Run 36899458556 left this
	 * at the default and the floor chased it from 2048 kB up past
	 * 9484 kB, so the change that was meant to go deeper than the 8 MiB
	 * run stopped earlier and then OOMed anyway.
	 */
	boost_saved = boost_factor_read();
	boost_factor_write(0);
	boost_now = boost_factor_read();
	printf("FRAGMENT_BUDDY watermark_boost_factor saved=%ld set=0 now=%ld\n",
	       boost_saved, boost_now);
	fflush(stdout);
	if (boost_now != 0) {
		printf("FRAGMENT_BUDDY ready=0 pagemap=0 reason=boost-not-disabled\n");
		fflush(stdout);
		if (min_free_saved > 0)
			min_free_kb_write(min_free_saved);
		return 3;
	}

	/*
	 * Deliberately not pinned here. Allocation, punch and chase all run
	 * with the OOM killer able to take this process: a kill in that window
	 * loses the pattern and reports INCONCLUSIVE_NO_PATTERN, which is a
	 * recoverable outcome. Pinning it instead, as run 36800977305 did,
	 * let out_of_memory() reclaim the drill shell and then find nothing
	 * killable at all -- "System is deadlocked on memory", guest panic.
	 * The pin goes on once ready=1 has been printed and the pattern has to
	 * outlive this process's next fault.
	 */
	maps = calloc((size_t)want, sizeof(*maps));
	pfns = calloc((size_t)want * (FRAG_CHUNK / FRAG_PAGE), sizeof(*pfns));
	chase_maps = calloc((size_t)FRAG_CHASE_CAP, sizeof(*chase_maps));
	if (!maps || !pfns || !chase_maps) {
		printf("HELPER calloc: %s\n", strerror(errno));
		fflush(stdout);
		free(maps);
		free(pfns);
		free(chase_maps);
		return 1;
	}

	high_before = high_order_blocks();
	printf("FRAGMENT_BUDDY start cap_chunks=%ld high_order_7plus=%ld\n",
	       want, high_before);
	fflush(stdout);

	/*
	 * Take the order-2 reserve first so it is carved out of an
	 * untouched buddy and comes back as contiguous runs. Held for the
	 * whole pattern, which keeps it out of the allocation being
	 * measured; released just before ready=1, see below.
	 */
	for (i = 0; i < FRAG_RESERVE_CHUNKS; i++) {
		char *m;

		m = mmap(NULL, FRAG_CHUNK, PROT_READ | PROT_WRITE,
			 MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
		if (m == MAP_FAILED)
			break;
		for (off = 0; off < FRAG_CHUNK; off += FRAG_PAGE)
			m[off] = 1;
		if (mlock(m, FRAG_CHUNK) != 0) {
			munmap(m, FRAG_CHUNK);
			break;
		}
		reserve[i] = m;
		reserve_got++;
	}

	/*
	 * Allocate 64 KiB chunks and touch every page. Stop when the buddy
	 * no longer holds a free block of order 7 or above -- that is the
	 * test condition, measured, not inferred -- or when the MemFree floor
	 * says the rest of the run still needs room to breathe.
	 *
	 * The order-7 test runs every chunk and BEFORE the floor. A 32-chunk
	 * cadence walked past blocks that a single chunk would have split, and
	 * a floor on MemAvailable stopped the loop while order-7 was still
	 * present. Both are ways to leave the test condition unmet without
	 * ever being short of memory.
	 *
	 * The floor is a margin, not a measure of exhaustion. It is only
	 * consulted while order-7 remains; once the condition is met the first
	 * check breaks out. What it leaves behind is split by the chase below.
	 */
	for (i = 0; i < want; i++) {
		char *m;
		long high;

		high = high_order_blocks();
		if (high == 0) {
			stopped = 1;
			stop_reason = "order7-depleted";
			break;
		}

		free_kb = mem_free_kb();
		if (free_kb >= 0 && free_kb < FRAG_HARD_KB) {
			stopped = 1;
			stop_reason = "memfree-margin";
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
		free(chase_maps);
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
		free(chase_maps);
		return 2;
	}

	/*
	 * Size the held bitmap for the whole guest, not for the main
	 * pattern. The chase takes its pages from the leftover region and
	 * their PFNs sit above the main pattern's max_pfn; a bitmap sized
	 * from max_pfn alone is overrun the first time a chase survivor is
	 * recorded. mem_total_pages() is the physical page count, which is
	 * the upper bound on any PFN this guest will hand out.
	 */
	{
		long total = mem_total_pages();
		unsigned long bound = max_pfn;

		if (total > 0 && (unsigned long)total > bound)
			bound = (unsigned long)total;
		seen_words = bound / (8 * sizeof(long)) + 2;
	}
	seen = calloc(seen_words, sizeof(*seen));
	if (!seen) {
		printf("HELPER calloc pfn-set: %s\n", strerror(errno));
		fflush(stdout);
		close(pmfd);
		free(maps);
		free(pfns);
		free(chase_maps);
		return 1;
	}
	for (i = 0; i < got * (FRAG_CHUNK / FRAG_PAGE); i++) {
		if (pfns[i])
			pfn_setbit(seen, pfns[i] - 1);
	}

	/*
	 * Chase first, punch after. The punch used to run before the chase,
	 * and that ordering is what stopped the chase from splitting anything:
	 * the 4096 holes are isolated order-0 pages, an allocator serves
	 * order-0 before it splits, and run 36811867648's chase took exactly
	 * 254 chunks x 16 pages = 15.9 MiB of them and then hit the unsplit
	 * floor with high_order_7plus still at 9. The fuel drained, the
	 * high-order blocks were never touched.
	 *
	 * Punching after means the chase faults into an untouched margin whose
	 * only free memory is the order-0-and-up the main loop left behind.
	 * The small-order residue goes first, and then every subsequent chunk
	 * is a split of an order-7-and-up block. It also keeps the 16 MiB out
	 * of the chase's RSS: the ring allocation gets it later, which is the
	 * point of the holes in the first place.
	 *
	 * Chase: split the high-order blocks the main loop's margin left
	 * behind. Checked every chunk, against the same measured condition as
	 * the main loop. Stops on that condition, then on the unsplit floor,
	 * then on the memfree backstop, then on the cap. Still unpinned: if
	 * this is where the guest runs out, the OOM killer takes us and the
	 * drill reports INCONCLUSIVE_NO_PATTERN instead of panicking.
	 */
	for (i = 0; i < FRAG_CHASE_CAP; i++) {
		char *m;
		long high, unsplit;

		high = high_order_blocks();
		if (high == 0) {
			stopped = 1;
			stop_reason = "order7-depleted";
			break;
		}

		unsplit = unsplit_free_pages();
		unsplit_at_stop = unsplit;
		if (unsplit >= 0 && unsplit < FRAG_CHASE_UNSPLIT) {
			stopped = 1;
			stop_reason = "chase-unsplit-floor";
			break;
		}

		free_kb = mem_free_kb();
		wmark_kb = live_min_watermark_kb();
		floor_kb = FRAG_CHASE_HARD_KB;
		if (min_free_set > 0 &&
		    min_free_set + FRAG_CHASE_WMARK_MARGIN > floor_kb)
			floor_kb = min_free_set + FRAG_CHASE_WMARK_MARGIN;
		if (free_kb >= 0 && free_kb < floor_kb) {
			stopped = 1;
			stop_reason = "chase-memfree-margin";
			break;
		}

		m = mmap(NULL, FRAG_CHUNK, PROT_READ | PROT_WRITE,
			 MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
		if (m == MAP_FAILED) {
			stopped = 1;
			stop_reason = "chase-mmap-refused";
			break;
		}
		for (off = 0; off < FRAG_CHUNK; off += FRAG_PAGE)
			m[off] = 1;
		if (mlock(m, FRAG_CHUNK) == 0)
			chase_locked++;
		/*
		 * Hold every page. No recycle here: the recycled pages came
		 * back as isolated order-0 and the next chunk's faults took
		 * those first (__rmqueue_smallest drains order-0 before it
		 * splits), so the chase split almost nothing and the
		 * high-order blocks survived untouched. Holding is what
		 * forces the split -- a held page is gone from every list
		 * the allocator can serve, so the next fault has to come out
		 * of a block that is still free. pmfd is open across this
		 * loop; it is used below to record what is held, and by
		 * pass 2 to punch the room the ring needs.
		 */
		/*
		 * Every page of this chunk is a survivor and becomes a held
		 * partner for pass 2. The bitmap must only ever gain pages
		 * that are still mapped: a freed page sitting in it as if it
		 * were held would let a later lookup release its buddy into
		 * a pair that can coalesce and rebuild the high orders just
		 * drained.
		 */
		if (pmfd >= 0) {
			for (off = 0; off < FRAG_CHUNK; off += FRAG_PAGE) {
				unsigned long spfn = 0;

				if (pagemap_pfn(pmfd, m + off, &spfn) == 0)
					pfn_setbit(seen, spfn);
			}
		}
		chase_maps[chase++] = m;
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
	 * unkillable. Cap the holes at FRAG_HOLE_CAP and the slab demand
	 * disappears: 4096 vm_area_structs cannot starve Unmovable.
	 * FRAG_HOLE_CAP holes still free 16 MiB of order-0 -- far more than the
	 * 2 MiB ring needs -- and the held buddies keep order-7 from reforming.
	 */
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
	if (stopped == 0 && high_order_blocks() == 0) {
		stopped = 1;
		stop_reason = "order7-depleted";
	}

	high_after = high_order_blocks();
	printf("FRAGMENT_BUDDY chase=%ld locked=%ld holes=%ld pairs=%ld high_order_7plus=%ld->%ld\n",
	       chase, chase_locked, chase_holes, chase_pairs, high_locked,
	       high_after);
	fflush(stdout);

	/*
	 * The pattern is built and measured. Pin now, and keep the pin for the
	 * whole hold: the drill shell's post-ready work -- the buddy snapshot,
	 * the high-order count and the channel-open rebind -- all run against
	 * this process holding the pages, and a reclaim in that window is what
	 * turned a finished pattern into 356 free high-order blocks and a
	 * rebind that failed on a buddy that had reassembled. The pin is
	 * dropped again when the hold ends.
	 */
	set_oom_adj("-1000\n");

	/*
	 * Return order-2 stacks BEFORE ready=1 is printed, not after. The
	 * drill shell's wait loop greps the log for ready=1 and forks
	 * immediately -- grep | tail | tee, buddy(), the channel rebind --
	 * and every one of those forks needs an order-2 stack. Releasing
	 * after the printf is a race that run 36914537690 lost: the shell
	 * forked, copy_process() found no order-2 block, the OOM killer
	 * took the shell (this process is pinned at -1000), init found
	 * nothing killable and the guest panicked. The OOM dump still read
	 * `8*8kB (U)` because this loop had run by then -- too late.
	 *
	 * Returning the groups first does not weaken the measured claim.
	 * high_order_7plus counts orders 7 and up; order-2 blocks are
	 * invisible to it. Each group is eight consecutive PFNs on a
	 * 4-page alignment whose first four are unmapped while the last
	 * four stay held, so the freed half cannot coalesce past order-2
	 * and the order-7 depletion just measured is not undone.
	 */
	for (i = 0; i < reserve_got && order2_groups < FRAG_ORDER2_GROUPS; i++) {
		char *m = reserve[i];

		off = 0;
		while (off + 8 * FRAG_PAGE <= FRAG_CHUNK &&
		       order2_groups < FRAG_ORDER2_GROUPS) {
			unsigned long p0 = 0, p = 0;
			int run = 1, j;

			if (pagemap_pfn(pmfd, m + off, &p0) != 0) {
				off += FRAG_PAGE;
				continue;
			}
			/*
			 * Walk to the next 4-page-aligned PFN, not to the next
			 * 4-page virtual step. mmap faults order-0 pages, so a
			 * chunk that starts at PFN 4k+2 is physically fine but
			 * no offset that is a multiple of 4 pages from the start
			 * is PFN-aligned. Stepping `off += 4 * FRAG_PAGE` from
			 * there skipped every candidate and reported
			 * `order2_reserve groups=0` on run 36914537690 w25
			 * while the chunk held contiguous pages.
			 */
			if ((p0 & 3UL) != 0) {
				off += (4UL - (p0 & 3UL)) * FRAG_PAGE;
				continue;
			}
			for (j = 1; j < 8; j++) {
				if (pagemap_pfn(pmfd, m + off + j * FRAG_PAGE, &p) != 0 ||
					p != p0 + (unsigned long)j) {
					run = 0;
					break;
				}
			}
			if (!run) {
				off += 4 * FRAG_PAGE;
				continue;
			}
			for (j = 0; j < 4; j++) {
				if (munmap(m + off + j * FRAG_PAGE,
					   (size_t)FRAG_PAGE) == 0)
					order2_pages++;
			}
			order2_groups++;
			off += 8 * FRAG_PAGE;
		}
	}
	printf("FRAGMENT_BUDDY order2_reserve groups=%ld pages=%ld reserve=%ld\n",
	       order2_groups, order2_pages, reserve_got);
	fflush(stdout);

	/*
	 * exhausted=1 means the measured condition holds: no free block of
	 * order 7 or above. It is not "the loop stopped", which is true of
	 * every floor and every refusal as well.
	 */
	printf("FRAGMENT_BUDDY ready=1 chunks=%ld pages=%ld held=%ld freed=%ld locked=%ld pairs=%ld chunk_kib=64 cap_chunks=%ld hole_cap=%ld chase=%ld chase_holes=%ld chase_pairs=%ld unsplit=%ld exhausted=%ld pagemap=1 stop=%s high_order_7plus=%ld->%ld->%ld floor_kb=%ld wmark_kb=%ld free_kb=%ld\n",
	       got, got * (FRAG_CHUNK / FRAG_PAGE), held_pages, freed_pages,
	       locked, pairs, want, FRAG_HOLE_CAP, chase, chase_holes,
	       chase_pairs, unsplit_at_stop, high_after == 0 ? 1L : 0L,
	       stop_reason, high_before, high_locked, high_after,
	       floor_kb, wmark_kb, free_kb);
	fflush(stdout);

	/*
	 * The watermarks stay where the pattern put them -- min_free_kbytes
	 * at 512 and watermark_boost_factor at 0 -- for the whole hold,
	 * including the drill's channel-open exercise. Restoring them here
	 * is what killed run 36906731699: with ~2 GiB locked and this
	 * process pinned at -1000, writing back min=5704 and boost_factor
	 * 15000 let boost_watermark() rebuild a 14712 kB boost against
	 * 9764 kB free. wmark_pages() adds boost on top of min, so the
	 * effective mark was 20416 kB, every GFP_HIGHUSER_MOVABLE fault
	 * failed, out_of_memory() found no killable process, and the guest
	 * panicked on "System is deadlocked on memory". The variable under
	 * test is fragmentation, not the reserve; a 20 MB watermark against
	 * a 9 MB free guest makes the test impossible regardless of the ring
	 * code. Restore only after the munmaps have given the memory back.
	 */
	if (hold > 0)
		sleep((unsigned int)hold);

	set_oom_adj("0\n");
	for (i = 0; i < got; i++)
		munmap(maps[i], FRAG_CHUNK);
	for (i = 0; i < chase; i++)
		munmap(chase_maps[i], FRAG_CHUNK);
	for (i = 0; i < reserve_got; i++)
		munmap(reserve[i], FRAG_CHUNK);
	free(seen);
	free(maps);
	free(pfns);
	free(chase_maps);
	close(pmfd);
	/*
	 * The memory is back before the reserve rises. Restoring earlier --
	 * even after the pin is dropped -- leaves ~2 GiB locked while min
	 * and boost climb, which is the window that panicked 36906731699.
	 */
	if (min_free_saved > 0)
		min_free_kb_write(min_free_saved);
	if (boost_saved >= 0)
		boost_factor_write(boost_saved);
	printf("FRAGMENT_BUDDY released held=%ld freed=%ld chase_holes=%ld\n",
	       held_pages, freed_pages, chase_holes);
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
