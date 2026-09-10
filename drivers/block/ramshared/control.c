// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared - IOCTL control plane and security
 *
 * Copyright (C) 2026 Emerson Busson
 */

#include <linux/blkdev.h>
#include <linux/capability.h>
#include "ramshared.h"

int ramshared_ioctl(struct block_device *bdev, blk_mode_t mode,
		    unsigned int cmd, unsigned long arg)
{
	if (!capable(CAP_SYS_ADMIN))
		return -EPERM;

	/* Placeholder for device reset, format, and repartition logic */
	switch (cmd) {
	default:
		return -ENOTTY;
	}
}
