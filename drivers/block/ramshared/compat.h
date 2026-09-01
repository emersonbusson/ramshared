/* SPDX-License-Identifier: GPL-2.0-only */
/*
 * RamShared - Multi-Kernel Compatibility Abstraction Layer
 * Supports: Linux 5.15 LTS (Ubuntu 22.04) -> 6.8 (Ubuntu 24.04) ->
 *           6.10 (Fedora 40) -> 6.12 LTS (Arch) -> 6.13+ Mainline
 *
 * Copyright (C) 2026 Emerson Busson
 */

#ifndef _RAMSHARED_COMPAT_H
#define _RAMSHARED_COMPAT_H

#include <linux/version.h>
#include <linux/blkdev.h>
#include <linux/blk-mq.h>
#include <linux/io.h>
#include <linux/err.h>

#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 11, 0)
/* Linux 6.11+ Paradigm: Features embedded in struct queue_limits */
#define RAMSHARED_HAVE_QUEUE_LIMITS_FEATURES	1
#elif LINUX_VERSION_CODE >= KERNEL_VERSION(6, 9, 0)
/* Linux 6.9 - 6.10 Paradigm: 3-arg blk_mq_alloc_disk with queue_limits */
#define RAMSHARED_HAVE_QUEUE_LIMITS_PARAM	1
#else
/* Linux <= 6.8 Paradigm: 2-arg blk_mq_alloc_disk and standalone mutators */
#define RAMSHARED_HAVE_LEGACY_BLK_ALLOC		1
#endif

/**
 * ramshared_alloc_disk - Allocate gendisk across all kernel versions
 * @set: Pointer to tag set
 * @queuedata: Private device state
 * @sector_size: Logical sector size (512)
 * @max_sectors: Maximum hardware sectors per request
 */
static inline struct gendisk *ramshared_alloc_disk(struct blk_mq_tag_set *set,
						   void *queuedata,
						   unsigned int sector_size,
						   unsigned int max_sectors)
{
#if defined(RAMSHARED_HAVE_QUEUE_LIMITS_FEATURES)
	struct queue_limits lim = {
		.logical_block_size	= sector_size,
		.physical_block_size	= PAGE_SIZE,
		.io_min			= PAGE_SIZE,
		.io_opt			= 64 * 1024,
		.max_hw_sectors		= max_sectors,
		.max_segments		= USHRT_MAX,
		.max_segment_size	= UINT_MAX,
		.dma_alignment		= 511,
		.max_hw_discard_sectors	= UINT_MAX,
		.discard_granularity	= PAGE_SIZE,
		.features		= BLK_FEAT_NOWAIT | BLK_FEAT_SYNCHRONOUS,
	};
	return blk_mq_alloc_disk(set, &lim, queuedata);

#elif defined(RAMSHARED_HAVE_QUEUE_LIMITS_PARAM)
	struct queue_limits lim = {
		.logical_block_size	= sector_size,
		.physical_block_size	= PAGE_SIZE,
		.io_min			= PAGE_SIZE,
		.io_opt			= 64 * 1024,
		.max_hw_sectors		= max_sectors,
		.max_segments		= USHRT_MAX,
		.max_segment_size	= UINT_MAX,
		.dma_alignment		= 511,
		.max_hw_discard_sectors	= UINT_MAX,
		.discard_granularity	= PAGE_SIZE,
	};
	struct gendisk *disk = blk_mq_alloc_disk(set, &lim, queuedata);

	if (!IS_ERR(disk)) {
		blk_queue_flag_set(QUEUE_FLAG_NONROT, disk->queue);
		blk_queue_flag_set(QUEUE_FLAG_SYNCHRONOUS, disk->queue);
		blk_queue_flag_set(QUEUE_FLAG_NOWAIT, disk->queue);
	}
	return disk;

#else /* Linux <= 6.8 (Ubuntu 22.04 LTS, Ubuntu 24.04 LTS) */
	struct gendisk *disk = blk_mq_alloc_disk(set, queuedata);
	struct request_queue *q;

	if (IS_ERR(disk))
		return disk;

	q = disk->queue;
	blk_queue_flag_set(QUEUE_FLAG_NONROT, q);
	blk_queue_flag_set(QUEUE_FLAG_SYNCHRONOUS, q);
	blk_queue_flag_set(QUEUE_FLAG_NOWAIT, q);

	blk_queue_logical_block_size(q, sector_size);
	blk_queue_physical_block_size(q, PAGE_SIZE);
	blk_queue_io_min(q, PAGE_SIZE);
	blk_queue_io_opt(q, 64 * 1024);
	blk_queue_max_hw_sectors(q, max_sectors);
	blk_queue_max_segments(q, USHRT_MAX);
	blk_queue_max_segment_size(q, UINT_MAX);
	blk_queue_dma_alignment(q, 511);
	blk_queue_max_discard_sectors(q, UINT_MAX);
	q->limits.discard_granularity = PAGE_SIZE;

	return disk;
#endif
}

/**
 * ramshared_devm_ioremap_wc - Fallback compatibility wrapper for devm_ioremap_wc
 * @dev: Pointer to struct device
 * @offset: Physical PCIe BAR offset
 * @size: Allocation size
 *
 * Provides guard clauses and unified ERR_PTR handling across kernel versions.
 */
static inline void __iomem *
ramshared_devm_ioremap_wc(struct device *dev, resource_size_t offset, size_t size)
{
	void __iomem *addr;

	if (!dev)
		return ERR_PTR(-ENODEV);

	if (size == 0)
		return ERR_PTR(-EINVAL);

	if ((offset & (PAGE_SIZE - 1)) != 0 || (size & (PAGE_SIZE - 1)) != 0)
		return ERR_PTR(-EINVAL);

#if defined(devm_ioremap_wc) || LINUX_VERSION_CODE >= KERNEL_VERSION(5, 15, 0)
	addr = devm_ioremap_wc(dev, offset, size);
#else
	addr = devm_ioremap(dev, offset, size);
#endif

	if (!addr)
		return ERR_PTR(-ENOMEM);

	return addr;
}

#endif /* _RAMSHARED_COMPAT_H */
