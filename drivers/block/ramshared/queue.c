// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared - blk-mq multi-queue request handling & hardware limit setup
 *
 * Copyright (C) 2026 Emerson Busson
 */

#include <linux/module.h>
#include <linux/blk-mq.h>
#include <linux/bio.h>
#include <linux/dma-mapping.h>
#include <linux/highmem.h>
#include <linux/io.h>
#include "ramshared.h"

static blk_status_t ramshared_process_bio(struct ramshared_device *rs_dev,
					  struct bio *bio, loff_t pos)
{
	struct bio_vec bvec;
	struct bvec_iter iter;
	unsigned int op = bio_op(bio);
	void __iomem *vram_ptr = rs_dev->dma.cpu_addr + pos;

	bio_for_each_segment(bvec, bio, iter) {
		void *mem = kmap_local_page(bvec.bv_page);
		void *src_or_dst = mem + bvec.bv_offset;
		size_t len = bvec.bv_len;

		if (op == REQ_OP_READ) {
			dma_rmb();
			memcpy_fromio(src_or_dst, vram_ptr, len);
			atomic64_add(len, &rs_dev->read_bytes);
		} else if (op == REQ_OP_WRITE) {
			memcpy_toio(vram_ptr, src_or_dst, len);
			dma_wmb();
			atomic64_add(len, &rs_dev->write_bytes);
		}
		kunmap_local(mem);
		vram_ptr += len;
	}

	atomic64_inc(&rs_dev->dma_transfers_total);
	return BLK_STS_OK;
}

static blk_status_t ramshared_queue_rq(struct blk_mq_hw_ctx *hctx,
				       const struct blk_mq_queue_data *bd)
{
	struct request *rq = bd->rq;
	struct ramshared_device *rs_dev = rq->q->queuedata;
	loff_t pos = (loff_t)blk_rq_pos(rq) << RAMSHARED_SECTOR_SHIFT;
	size_t len = blk_rq_bytes(rq);
	blk_status_t status = BLK_STS_OK;
	struct bio *bio;

	if (unlikely(!rs_dev || !rs_dev->dma.cpu_addr))
		return BLK_STS_IOERR;

	if (unlikely(pos + len > rs_dev->capacity_bytes)) {
		dev_err_ratelimited(rs_dev->dev,
			"I/O bounds violation: pos=%lld, len=%zu, cap=%llu\n",
			pos, len, rs_dev->capacity_bytes);
		return BLK_STS_IOERR;
	}

	blk_mq_start_request(rq);

	switch (req_op(rq)) {
	case REQ_OP_READ:
	case REQ_OP_WRITE:
		__rq_for_each_bio(bio, rq) {
			status = ramshared_process_bio(rs_dev, bio, pos);
			if (status != BLK_STS_OK)
				break;
			pos += bio->bi_iter.bi_size;
		}
		break;
	case REQ_OP_FLUSH:
		dma_wmb();
		status = BLK_STS_OK;
		break;
	case REQ_OP_DISCARD:
	case REQ_OP_SECURE_ERASE:
		memset_io(rs_dev->dma.cpu_addr + pos, 0, len);
		dma_wmb();
		status = BLK_STS_OK;
		break;
	default:
		status = BLK_STS_NOTSUPP;
		break;
	}

	blk_mq_end_request(rq, status);
	return BLK_STS_OK;
}

static const struct blk_mq_ops ramshared_mq_ops = {
	.queue_rq = ramshared_queue_rq,
};

static const struct block_device_operations ramshared_fops = {
	.owner = THIS_MODULE,
};

/* Sysfs Attributes Group (Race-free via disk_groups) */
static ssize_t capacity_bytes_show(struct device *dev,
				   struct device_attribute *attr, char *buf)
{
	struct gendisk *disk = dev_to_disk(dev);
	struct ramshared_device *rs_dev = disk->private_data;

	return sysfs_emit(buf, "%llu\n", rs_dev->capacity_bytes);
}
static DEVICE_ATTR_RO(capacity_bytes);

static ssize_t dma_transfers_total_show(struct device *dev,
					struct device_attribute *attr, char *buf)
{
	struct gendisk *disk = dev_to_disk(dev);
	struct ramshared_device *rs_dev = disk->private_data;

	return sysfs_emit(buf, "%lld\n", atomic64_read(&rs_dev->dma_transfers_total));
}
static DEVICE_ATTR_RO(dma_transfers_total);

static struct attribute *ramshared_attrs[] = {
	&dev_attr_capacity_bytes.attr,
	&dev_attr_dma_transfers_total.attr,
	NULL,
};

static const struct attribute_group ramshared_attr_group = {
	.name = "ramshared",
	.attrs = ramshared_attrs,
};

static const struct attribute_group *ramshared_attr_groups[] = {
	&ramshared_attr_group,
	NULL,
};

int ramshared_queue_init(struct ramshared_device *rs_dev,
			 struct device *parent_dev, unsigned int q_depth)
{
	struct request_queue *q;
	unsigned int valid_depth;
	int ret;

	if (!rs_dev)
		return -EINVAL;

	valid_depth = clamp_t(unsigned int, q_depth, 16U, 2048U);

	memset(&rs_dev->tag_set, 0, sizeof(rs_dev->tag_set));
	rs_dev->tag_set.ops = &ramshared_mq_ops;
	rs_dev->tag_set.nr_hw_queues = num_online_cpus();
	rs_dev->tag_set.queue_depth = valid_depth;
	rs_dev->tag_set.numa_node = NUMA_NO_NODE;
	rs_dev->tag_set.flags = BLK_MQ_F_SHOULD_MERGE;

	ret = blk_mq_alloc_tag_set(&rs_dev->tag_set);
	if (ret)
		return ret;

	rs_dev->disk = blk_mq_alloc_disk(&rs_dev->tag_set, rs_dev);
	if (IS_ERR(rs_dev->disk)) {
		ret = PTR_ERR(rs_dev->disk);
		blk_mq_free_tag_set(&rs_dev->tag_set);
		return ret;
	}

	q = rs_dev->disk->queue;
	q->queuedata = rs_dev;

	/* Configure Queue Flags */
	blk_queue_flag_set(QUEUE_FLAG_NONROT, q);
	blk_queue_flag_set(QUEUE_FLAG_SYNCHRONOUS, q);
	blk_queue_flag_set(QUEUE_FLAG_NOWAIT, q);

	/* Configure Hardware & DMA Limits */
	blk_queue_logical_block_size(q, RAMSHARED_SECTOR_SIZE);
	blk_queue_physical_block_size(q, PAGE_SIZE);
	blk_queue_io_min(q, PAGE_SIZE);
	blk_queue_io_opt(q, 64 * 1024);
	blk_queue_max_hw_sectors(q, 2048);
	blk_queue_max_segments(q, USHRT_MAX);
	blk_queue_max_segment_size(q, UINT_MAX);
	blk_queue_dma_alignment(q, 511);
	blk_queue_max_discard_sectors(q, UINT_MAX);
	q->limits.discard_granularity = PAGE_SIZE;

	/* Setup gendisk descriptor */
	rs_dev->disk->major = 0;
	rs_dev->disk->first_minor = 0;
	rs_dev->disk->minors = 1;
	rs_dev->disk->fops = &ramshared_fops;
	rs_dev->disk->private_data = rs_dev;
	rs_dev->disk->disk_groups = ramshared_attr_groups;
	rs_dev->disk->flags |= GENHD_FL_NO_PART;
	rs_dev->disk->parent = parent_dev;
	snprintf(rs_dev->disk->disk_name, DISK_NAME_LEN, "ramshared0");
	set_capacity(rs_dev->disk, rs_dev->capacity_bytes >> RAMSHARED_SECTOR_SHIFT);

	return 0;
}

void ramshared_queue_cleanup(struct ramshared_device *rs_dev)
{
	if (!rs_dev)
		return;

	if (rs_dev->disk) {
		del_gendisk(rs_dev->disk);
		put_disk(rs_dev->disk);
		rs_dev->disk = NULL;
	}

	blk_mq_free_tag_set(&rs_dev->tag_set);
}
