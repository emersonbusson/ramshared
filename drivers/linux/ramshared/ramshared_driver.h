/* SPDX-License-Identifier: GPL-2.0-only */
/*
 * RamShared VRAM Block Driver — Core Definitions
 *
 * Copyright (C) 2026 Emerson Busson 
 */

#ifndef _RAMSHARED_DRIVER_H
#define _RAMSHARED_DRIVER_H

#include <linux/types.h>
#include <linux/blkdev.h>
#include <linux/blk-mq.h>
#include <linux/pci.h>
#include <linux/spinlock.h>
#include <linux/mutex.h>
#include <linux/atomic.h>

#define RAMSHARED_NAME			"ramshared"
#define RAMSHARED_DEFAULT_MINORS	16
#define RAMSHARED_CHUNK_SIZE		(128ULL * 1024 * 1024) /* 128 MiB Chunks */
#define RAMSHARED_SECTOR_SHIFT		9
#define RAMSHARED_SECTOR_SIZE		(1ULL << RAMSHARED_SECTOR_SHIFT)

/* Status and State Machine */
enum ramshared_tier_state {
	RAMSHARED_TIER_ONLINE = 0,
	RAMSHARED_TIER_VRAM_ONLY,
	RAMSHARED_TIER_SSD_ONLY,
	RAMSHARED_TIER_REVOKED,
	RAMSHARED_TIER_OFFLINE,
};

/* Accounting & Performance Statistics */
struct ramshared_stats {
	atomic64_t vram_read_bytes;
	atomic64_t vram_write_bytes;
	atomic64_t vram_hits;
	atomic64_t vram_misses;
	atomic64_t ssd_fallback_reads;
	atomic64_t ssd_fallback_writes;
	atomic64_t revocations_total;
	atomic64_t zero_fills;
};

/* VRAM Segment Descriptor */
struct ramshared_vram_chunk {
	u64 physical_offset;
	void __iomem *vaddr;
	u64 generation;
	bool valid;
	spinlock_t lock;
};

/* Device Context */
struct ramshared_device {
	int id;
	dev_t devt;
	struct gendisk *disk;
	struct blk_mq_tag_set tag_set;
	struct request_queue *queue;

	/* PCIe / GPU Resources */
	struct pci_dev *pdev;
	void __iomem *vram_bar_base;
	resource_size_t vram_bar_start;
	resource_size_t vram_bar_len;
	resource_size_t vram_allocated;

	/* Chunks Table */
	struct ramshared_vram_chunk *chunks;
	size_t num_chunks;
	size_t logical_capacity;

	/* Dual-Tier State */
	enum ramshared_tier_state state;
	struct block_device *origin_bdev;
	struct file *origin_file;
	struct mutex ctl_mutex;
	spinlock_t state_lock;

	/* Statistics */
	struct ramshared_stats stats;
};

/* Function Prototypes */
int ramshared_vram_init(struct ramshared_device *dev);
void ramshared_vram_cleanup(struct ramshared_device *dev);
int ramshared_vram_transfer(struct ramshared_device *dev, struct bio_vec *bvec,
			   sector_t sector, enum req_op op);

int ramshared_tier_init(struct ramshared_device *dev);
void ramshared_tier_cleanup(struct ramshared_device *dev);
blk_status_t ramshared_queue_rq(struct blk_mq_hw_ctx *hctx,
				const struct blk_mq_queue_data *bd);

#endif /* _RAMSHARED_DRIVER_H */
