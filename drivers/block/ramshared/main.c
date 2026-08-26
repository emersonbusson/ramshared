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

static struct ramshared_device g_ramshared_dev;
static int g_major;

static const struct block_device_operations ramshared_fops = {
	.owner = THIS_MODULE,
};

static int __init ramshared_init(void)
{
	int ret;

	pr_info("%s: loading version %s\n",
		RAMSHARED_DRIVER_NAME, RAMSHARED_DRIVER_VERSION);

	mutex_init(&g_ramshared_dev.lock);

	g_major = register_blkdev(0, RAMSHARED_DRIVER_NAME);
	if (g_major < 0) {
		pr_err("%s: failed to register blkdev\n", RAMSHARED_DRIVER_NAME);
		return g_major;
	}

	ret = ramshared_dma_init(&g_ramshared_dev, 1ULL << 30); /* 1 GiB default */
	if (ret)
		goto err_blkdev;

	ret = ramshared_queue_init(&g_ramshared_dev);
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
