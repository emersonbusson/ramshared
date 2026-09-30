// SPDX-License-Identifier: MIT
/*
 * cascade_hog - anonymous-memory hog for the cascade DEMOTE drill.
 *
 * Usage: cascade-hog <MiB> hold
 *
 * Allocates <MiB> of anonymous memory and fills every page with a
 * deterministic pattern derived from the page index, so a later re-read can
 * prove that a swap round-trip through the cascade returned the pages intact.
 * The pattern is recomputed at verify time; no copy of the data is kept, so a
 * mismatch is real corruption and not a lost reference.
 *
 * Protocol with scripts/p0/measure-cascade-demote.sh:
 *   1. fill every page, then create /tmp/cv-filled
 *   2. hold until /tmp/cv-go appears (the harness creates it after DEMOTE)
 *   3. re-read every page, report "[hog]" lines, exit 0 only if 0 mismatches
 *
 * The process is expected to be placed in a cgroup with a memory.max well
 * below <MiB> so that the excess pages are pushed into the cascade. This
 * program never touches swap configuration; all mount/demote work belongs to
 * the daemon and the harness.
 */
#define _GNU_SOURCE	/* madvise, MADV_NOHUGEPAGE */

#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/mman.h>

#define PAGE_SIZE	4096UL
#define FILLED_PATH	"/tmp/cv-filled"
#define GO_PATH		"/tmp/cv-go"
#define GO_POLL_NS	(200L * 1000L * 1000L)
/* One progress line per 256 MiB of fill. */
#define PAGES_PER_PROGRESS	((256UL * 1024UL * 1024UL) / PAGE_SIZE)

/*
 * Deterministic per-page content. Word j of page i is a mix of both indices,
 * so any reordered, duplicated, or zeroed page is detected at verify time.
 */
static uint64_t page_word(uint64_t page, uint64_t word)
{
	uint64_t x = page * 0x9E3779B97F4A7C15ULL + word;

	x ^= x >> 30;
	x *= 0xBF58476D1CE4E5B9ULL;
	x ^= x >> 27;
	x *= 0x94D049BB133111EBULL;
	x ^= x >> 31;
	return x;
}

static void fill_page(uint64_t page, uint8_t *dst)
{
	uint64_t *words = (uint64_t *)dst;
	uint64_t w;

	for (w = 0; w < PAGE_SIZE / sizeof(uint64_t); w++)
		words[w] = page_word(page, w);
}

static unsigned long verify_page(uint64_t page, const uint8_t *src)
{
	const uint64_t *words = (const uint64_t *)src;
	unsigned long bad = 0;
	uint64_t w;

	for (w = 0; w < PAGE_SIZE / sizeof(uint64_t); w++) {
		if (words[w] != page_word(page, w))
			bad++;
	}
	return bad;
}

static void poll_for_go(void)
{
	struct timespec ts = { .tv_sec = 0, .tv_nsec = GO_POLL_NS };

	for (;;) {
		if (access(GO_PATH, F_OK) == 0)
			return;
		nanosleep(&ts, NULL);
	}
}

int main(int argc, char **argv)
{
	unsigned long mib, pages, i, mismatched_pages = 0;
	unsigned long long bad_words = 0;
	size_t bytes;
	uint8_t *mem;
	char *end;
	long parsed;

	if (argc != 3 || strcmp(argv[2], "hold") != 0) {
		fprintf(stderr, "usage: %s <MiB> hold\n", argv[0]);
		return 2;
	}

	errno = 0;
	parsed = strtol(argv[1], &end, 10);
	if (errno || end == argv[1] || *end != '\0' || parsed <= 0) {
		fprintf(stderr, "[hog] invalid MiB: %s\n", argv[1]);
		return 2;
	}
	mib = (unsigned long)parsed;
	pages = (mib * 1024UL * 1024UL) / PAGE_SIZE;
	bytes = pages * PAGE_SIZE;

	mem = mmap(NULL, bytes, PROT_READ | PROT_WRITE,
		   MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
	if (mem == MAP_FAILED) {
		fprintf(stderr, "[hog] mmap %lu MiB failed: %s\n", mib,
			strerror(errno));
		return 1;
	}

	/* Prefer the kernel to reclaim these pages first, never file cache. */
	if (madvise(mem, bytes, MADV_NOHUGEPAGE) != 0)
		fprintf(stderr, "[hog] madvise MADV_NOHUGEPAGE: %s\n",
			strerror(errno));

	for (i = 0; i < pages; i++) {
		fill_page(i, mem + i * PAGE_SIZE);
		/* Under a tight cgroup cap the fill is reclaimed as it goes, so
		 * progress must be visible in the RAW log or a slow fill looks
		 * like a hang.
		 */
		if ((i + 1) % (PAGES_PER_PROGRESS) == 0) {
			printf("[hog] fill %lu / %lu pages (%lu MiB of %lu)\n",
			       i + 1, pages, ((i + 1) * PAGE_SIZE) / (1024 * 1024),
			       mib);
			fflush(stdout);
		}
	}

	printf("[hog] filled %lu pages (%lu MiB) pid=%ld\n", pages, mib,
	       (long)getpid());
	fflush(stdout);

	{
		FILE *fp = fopen(FILLED_PATH, "we");

		if (!fp) {
			fprintf(stderr, "[hog] cannot create %s: %s\n",
				FILLED_PATH, strerror(errno));
			munmap(mem, bytes);
			return 1;
		}
		fprintf(fp, "%lu\n", pages);
		fclose(fp);
	}

	printf("[hog] holding until %s\n", GO_PATH);
	fflush(stdout);
	poll_for_go();

	for (i = 0; i < pages; i++) {
		unsigned long bad = verify_page(i, mem + i * PAGE_SIZE);

		if (bad) {
			mismatched_pages++;
			bad_words += bad;
		}
	}

	printf("[hog] verified %lu pages, %lu pages with corruption, %llu bad words\n",
	       pages, mismatched_pages, bad_words);
	printf("[hog] RESULT %s\n",
	       mismatched_pages == 0 ? "PASS 0 corruption" : "FAIL corruption");
	fflush(stdout);

	munmap(mem, bytes);
	return mismatched_pages == 0 ? 0 : 1;
}
