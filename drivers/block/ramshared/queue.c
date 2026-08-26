// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared - blk-mq multi-queue request handling
 */

#include <linux/module.h>
#include <linux/blk-mq.h>
#include <linux/bio.h>
#include "ramshared.h"

static blk_status_t ramshared_queue_rq(struct blk_mq_hw_ctx *hctx,
				       const struct blk_mq_queue_data *bd)
{
	struct request *rq = bd->rq;
	struct ramshared_device *rs_dev = hctx->queue->queuedata;
	struct bio_vec bvec;
	struct req_iterator iter;
	loff_t pos;

	if (!rs_dev)
		return BLK_STS_IOERR;

	blk_mq_start_request(rq);
	pos = (loff_t)blk_rq_pos(rq) << RAMSHARED_SECTOR_SHIFT;

	rq_for_each_segment(bvec, rq, iter) {
		pos += bvec.bv_len;
	}

	blk_mq_end_request(rq, BLK_STS_OK);
	return BLK_STS_OK;
}

static const struct blk_mq_ops ramshared_mq_ops = {
	.queue_rq = ramshared_queue_rq,
};

int ramshared_queue_init(struct ramshared_device *rs_dev, unsigned int q_depth)
{
	unsigned int valid_depth;

	if (!rs_dev)
		return -EINVAL;

	valid_depth = clamp_t(unsigned int, q_depth, 16U, 2048U);

	memset(&rs_dev->tag_set, 0, sizeof(rs_dev->tag_set));
	rs_dev->tag_set.ops = &ramshared_mq_ops;
	rs_dev->tag_set.nr_hw_queues = num_online_cpus();
	rs_dev->tag_set.queue_depth = valid_depth;
	rs_dev->tag_set.numa_node = NUMA_NO_NODE;
	rs_dev->tag_set.flags = BLK_MQ_F_SHOULD_MERGE;

	return blk_mq_alloc_tag_set(&rs_dev->tag_set);
}

void ramshared_queue_cleanup(struct ramshared_device *rs_dev)
{
	if (!rs_dev)
		return;

	blk_mq_free_tag_set(&rs_dev->tag_set);
}
