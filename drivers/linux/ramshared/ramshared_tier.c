// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared VRAM Block Driver — Dual-Tier Cache & Storage Policy
 *
 * Copyright (C) 2026 Emerson Busson 
 */

#include <linux/module.h>
#include <linux/blkdev.h>
#include <linux/bio.h>
#include "ramshared_driver.h"

int ramshared_tier_init(struct ramshared_device *dev)
{
	mutex_init(&dev->ctl_mutex);
	spin_lock_init(&dev->state_lock);

	atomic64_set(&dev->stats.vram_read_bytes, 0);
	atomic64_set(&dev->stats.vram_write_bytes, 0);
	atomic64_set(&dev->stats.vram_hits, 0);
	atomic64_set(&dev->stats.vram_misses, 0);
	atomic64_set(&dev->stats.ssd_fallback_reads, 0);
	atomic64_set(&dev->stats.ssd_fallback_writes, 0);
	atomic64_set(&dev->stats.revocations_total, 0);
	atomic64_set(&dev->stats.zero_fills, 0);

	return 0;
}

void ramshared_tier_cleanup(struct ramshared_device *dev)
{
	mutex_destroy(&dev->ctl_mutex);
}

/*
 * blk-mq Request Queue Handler
 * Processes requests dispatched by the kernel block layer.
 */
blk_status_t ramshared_queue_rq(struct blk_mq_hw_ctx *hctx,
				const struct blk_mq_queue_data *bd)
{
	struct request *rq = bd->rq;
	struct ramshared_device *dev = rq->q->queuedata;
	struct req_iterator iter;
	struct bio_vec bvec;
	sector_t sector = blk_rq_pos(rq);
	enum req_op op = req_op(rq);
	int ret = 0;

	blk_mq_start_request(rq);

	/* Check if operation is supported */
	if (op != REQ_OP_READ && op != REQ_OP_WRITE && op != REQ_OP_FLUSH) {
		blk_mq_end_request(rq, BLK_STS_NOTSUPP);
		return BLK_STS_OK;
	}

	if (op == REQ_OP_FLUSH) {
		/* Barrier/Flush: enforce cache line writebacks */
		wmb();
		blk_mq_end_request(rq, BLK_STS_OK);
		return BLK_STS_OK;
	}

	/* Iterate over bio vectors and perform PCIe transfers */
	rq_for_each_segment(bvec, rq, iter) {
		if (dev->state == RAMSHARED_TIER_ONLINE ||
		    dev->state == RAMSHARED_TIER_VRAM_ONLY) {
			ret = ramshared_vram_transfer(dev, &bvec, sector, op);
			if (ret)
				break;
		} else {
			/* Offline or Revoked: zero fill reads */
			if (op == REQ_OP_READ) {
				void *mem = kmap_local_page(bvec.bv_page) + bvec.bv_offset;
				memset(mem, 0, bvec.bv_len);
				kunmap_local(mem);
				atomic64_inc(&dev->stats.zero_fills);
			}
		}

		sector += (bvec.bv_len >> RAMSHARED_SECTOR_SHIFT);
	}

	if (ret) {
		blk_mq_end_request(rq, BLK_STS_IOERR);
	} else {
		blk_mq_end_request(rq, BLK_STS_OK);
	}

	return BLK_STS_OK;
}
