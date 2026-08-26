// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared VRAM Block Driver — Main Driver & blk-mq Registration
 *
 * Copyright (C) 2026 Emerson Busson 
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/init.h>
#include <linux/blkdev.h>
#include <linux/blk-mq.h>
#include <linux/slab.h>
#include "ramshared_driver.h"

static int major_num;
static struct ramshared_device *ramshared_dev;

static unsigned long logical_capacity_mib = 4096; /* 4 GiB Default */
module_param(logical_capacity_mib, ulong, 0444);
MODULE_PARM_DESC(logical_capacity_mib, "Logical device capacity in MiB (default: 4096)");

static const struct block_device_operations ramshared_fops = {
	.owner = THIS_MODULE,
};

static const struct blk_mq_ops ramshared_mq_ops = {
	.queue_rq = ramshared_queue_rq,
};

/* Sysfs Statistics Attributes */
static ssize_t vram_hits_show(struct device *d, struct device_attribute *attr, char *buf)
{
	struct gendisk *disk = dev_to_disk(d);
	struct ramshared_device *dev = disk->private_data;

	return sysfs_emit(buf, "%lld\n", atomic64_read(&dev->stats.vram_hits));
}
static DEVICE_ATTR_RO(vram_hits);

static ssize_t vram_misses_show(struct device *d, struct device_attribute *attr, char *buf)
{
	struct gendisk *disk = dev_to_disk(d);
	struct ramshared_device *dev = disk->private_data;

	return sysfs_emit(buf, "%lld\n", atomic64_read(&dev->stats.vram_misses));
}
static DEVICE_ATTR_RO(vram_misses);

static ssize_t vram_read_bytes_show(struct device *d, struct device_attribute *attr, char *buf)
{
	struct gendisk *disk = dev_to_disk(d);
	struct ramshared_device *dev = disk->private_data;

	return sysfs_emit(buf, "%lld\n", atomic64_read(&dev->stats.vram_read_bytes));
}
static DEVICE_ATTR_RO(vram_read_bytes);

static ssize_t vram_write_bytes_show(struct device *d, struct device_attribute *attr, char *buf)
{
	struct gendisk *disk = dev_to_disk(d);
	struct ramshared_device *dev = disk->private_data;

	return sysfs_emit(buf, "%lld\n", atomic64_read(&dev->stats.vram_write_bytes));
}
static DEVICE_ATTR_RO(vram_write_bytes);

static ssize_t tier_state_show(struct device *d, struct device_attribute *attr, char *buf)
{
	struct gendisk *disk = dev_to_disk(d);
	struct ramshared_device *dev = disk->private_data;
	const char *state_str = "OFFLINE";

	switch (dev->state) {
	case RAMSHARED_TIER_ONLINE:    state_str = "ONLINE (VRAM Accelerated)"; break;
	case RAMSHARED_TIER_VRAM_ONLY: state_str = "VRAM_ONLY"; break;
	case RAMSHARED_TIER_SSD_ONLY:  state_str = "SSD_ONLY (Direct Origin)"; break;
	case RAMSHARED_TIER_REVOKED:   state_str = "REVOKED"; break;
	default:                       state_str = "OFFLINE"; break;
	}

	return sysfs_emit(buf, "%s\n", state_str);
}
static DEVICE_ATTR_RO(tier_state);

static struct attribute *ramshared_attrs[] = {
	&dev_attr_vram_hits.attr,
	&dev_attr_vram_misses.attr,
	&dev_attr_vram_read_bytes.attr,
	&dev_attr_vram_write_bytes.attr,
	&dev_attr_tier_state.attr,
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

static int __init ramshared_init(void)
{
	int ret = 0;
	struct ramshared_device *dev;

	pr_info("ramshared: Initializing RamShared in-kernel VRAM block driver\n");

	major_num = register_blkdev(0, RAMSHARED_NAME);
	if (major_num < 0) {
		pr_err("ramshared: Failed to register block device major\n");
		return major_num;
	}

	dev = kzalloc(sizeof(*dev), GFP_KERNEL);
	if (!dev) {
		ret = -ENOMEM;
		goto out_unregister;
	}

	ramshared_dev = dev;
	dev->id = 0;
	dev->logical_capacity = logical_capacity_mib * 1024 * 1024;

	ramshared_tier_init(dev);

	/* Initialize blk-mq tag set */
	dev->tag_set.ops = &ramshared_mq_ops;
	dev->tag_set.nr_hw_queues = num_online_cpus();
	dev->tag_set.queue_depth = 128;
	dev->tag_set.numa_node = NUMA_NO_NODE;
	dev->tag_set.flags = BLK_MQ_F_SHOULD_MERGE;
	dev->tag_set.driver_data = dev;

	ret = blk_mq_alloc_tag_set(&dev->tag_set);
	if (ret) {
		pr_err("ramshared: Failed to allocate blk-mq tag set: %d\n", ret);
		goto out_free_dev;
	}

	/* Allocate gendisk with blk-mq */
	dev->disk = blk_mq_alloc_disk(&dev->tag_set, dev);
	if (IS_ERR(dev->disk)) {
		ret = PTR_ERR(dev->disk);
		pr_err("ramshared: Failed to allocate gendisk: %d\n", ret);
		goto out_free_tags;
	}

	dev->disk->major = major_num;
	dev->disk->first_minor = 0;
	dev->disk->minors = 1;
	dev->disk->fops = &ramshared_fops;
	dev->disk->private_data = dev;
	snprintf(dev->disk->disk_name, DISK_NAME_LEN, "%s%d", RAMSHARED_NAME, dev->id);

	set_capacity(dev->disk, dev->logical_capacity >> RAMSHARED_SECTOR_SHIFT);

	/* Set block limits: 4 KiB logical block size, uninhibited throughput */
	blk_queue_logical_block_size(dev->disk->queue, 4096);
	blk_queue_physical_block_size(dev->disk->queue, 4096);
	blk_queue_max_hw_sectors(dev->disk->queue, 2048);

	/* Attempt VRAM aperture initialization */
	ret = ramshared_vram_init(dev);
	if (ret)
		pr_warn("ramshared: Initialized in degraded mode: %d\n", ret);

	/* Attach custom sysfs stats attribute group */
	dev->disk->queue->kobj.ktype->default_groups = (const struct attribute_group **)ramshared_attr_groups;

	ret = add_disk(dev->disk);
	if (ret) {
		pr_err("ramshared: Failed to add disk to system: %d\n", ret);
		goto out_cleanup_vram;
	}

	pr_info("ramshared: Block device /dev/%s ready (%lu MiB)\n",
		dev->disk->disk_name, logical_capacity_mib);

	return 0;

out_cleanup_vram:
	ramshared_vram_cleanup(dev);
	put_disk(dev->disk);
out_free_tags:
	blk_mq_free_tag_set(&dev->tag_set);
out_free_dev:
	kfree(dev);
	ramshared_dev = NULL;
out_unregister:
	unregister_blkdev(major_num, RAMSHARED_NAME);
	return ret;
}

static void __exit ramshared_exit(void)
{
	struct ramshared_device *dev = ramshared_dev;

	if (!dev)
		return;

	pr_info("ramshared: Unloading RamShared in-kernel block driver\n");

	if (dev->disk) {
		del_gendisk(dev->disk);
		put_disk(dev->disk);
	}

	ramshared_vram_cleanup(dev);
	ramshared_tier_cleanup(dev);

	blk_mq_free_tag_set(&dev->tag_set);
	unregister_blkdev(major_num, RAMSHARED_NAME);
	kfree(dev);
	ramshared_dev = NULL;

	pr_info("ramshared: Driver unloaded cleanly\n");
}

module_init(ramshared_init);
module_exit(ramshared_exit);

MODULE_AUTHOR("Emerson Busson ");
MODULE_DESCRIPTION("RamShared In-Kernel VRAM-Accelerated Block Driver");
MODULE_LICENSE("GPL");
MODULE_VERSION("1.0.0");
MODULE_ALIAS_BLOCKDEV_MAJOR(0);
