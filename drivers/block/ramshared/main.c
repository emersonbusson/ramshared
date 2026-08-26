// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared - Hardware-Accelerated VRAM Block Driver
 *
 * Copyright (C) 2026 Emerson Busson
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/init.h>
#include <linux/blkdev.h>
#include <linux/blk-mq.h>
#include <linux/mutex.h>
#include "ramshared.h"

MODULE_AUTHOR("Emerson Busson");
MODULE_DESCRIPTION("Hardware-Accelerated VRAM Block Driver");
MODULE_LICENSE("GPL");
MODULE_VERSION(RAMSHARED_DRIVER_VERSION);

static unsigned long capacity_mb = 1024;
module_param(capacity_mb, ulong, 0444);
MODULE_PARM_DESC(capacity_mb, "Initial VRAM block device capacity in MiB (default: 1024)");

static unsigned int queue_depth = RAMSHARED_DEFAULT_QUEUE_DEPTH;
module_param(queue_depth, uint, 0444);
MODULE_PARM_DESC(queue_depth, "Hardware queue depth (default: 128)");

static struct ramshared_device g_ramshared_dev;
static int g_major;

static const struct block_device_operations ramshared_fops = {
	.owner = THIS_MODULE,
};

static int __init ramshared_init(void)
{
	size_t cap_bytes;
	int ret;

	pr_info("%s: loading version %s (capacity=%lu MiB, queue_depth=%u)\n",
		RAMSHARED_DRIVER_NAME, RAMSHARED_DRIVER_VERSION,
		capacity_mb, queue_depth);

	if (capacity_mb == 0 || capacity_mb > (1UL << 20)) {
		pr_err("%s: invalid capacity_mb parameter: %lu\n",
			RAMSHARED_DRIVER_NAME, capacity_mb);
		return -EINVAL;
	}

	mutex_init(&g_ramshared_dev.lock);

	g_major = register_blkdev(0, RAMSHARED_DRIVER_NAME);
	if (g_major < 0) {
		pr_err("%s: failed to register blkdev\n", RAMSHARED_DRIVER_NAME);
		return g_major;
	}

	cap_bytes = (size_t)capacity_mb * 1024 * 1024;
	ret = ramshared_dma_init(&g_ramshared_dev, cap_bytes);
	if (ret)
		goto err_blkdev;

	ret = ramshared_queue_init(&g_ramshared_dev, queue_depth);
	if (ret)
		goto err_dma;

	pr_info("%s: block device initialized successfully (major %d)\n",
		RAMSHARED_DRIVER_NAME, g_major);
	return 0;

err_dma:
	ramshared_dma_cleanup(&g_ramshared_dev);
err_blkdev:
	unregister_blkdev(g_major, RAMSHARED_DRIVER_NAME);
	return ret;
}

static void __exit ramshared_exit(void)
{
	ramshared_queue_cleanup(&g_ramshared_dev);
	ramshared_dma_cleanup(&g_ramshared_dev);
	unregister_blkdev(g_major, RAMSHARED_DRIVER_NAME);
	pr_info("%s: driver unloaded\n", RAMSHARED_DRIVER_NAME);
}

module_init(ramshared_init);
module_exit(ramshared_exit);
