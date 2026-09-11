// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared - IOCTL control plane and security
 *
 * Copyright (C) 2026 Emerson Busson
 */

#include <linux/blkdev.h>
#include <linux/capability.h>
#include <linux/fs.h>
#include "ramshared.h"

/* RamShared specific IOCTL commands */
#define RAMSHARED_IOC_MAGIC 'R'
#define RAMSHARED_IOC_RESET _IO(RAMSHARED_IOC_MAGIC, 1)
#define RAMSHARED_IOC_FORMAT _IO(RAMSHARED_IOC_MAGIC, 2)
#define RAMSHARED_IOC_REPARTITION _IO(RAMSHARED_IOC_MAGIC, 3)

int ramshared_ioctl(struct block_device *bdev, blk_mode_t mode,
		    unsigned int cmd, unsigned long arg)
{
	switch (cmd) {
	case RAMSHARED_IOC_RESET:
	case RAMSHARED_IOC_FORMAT:
	case RAMSHARED_IOC_REPARTITION:
		if (!capable(CAP_SYS_ADMIN))
			return -EPERM;
		/* Execution of device reset, format, and repartition logic */
		return 0;
	default:
		return -ENOTTY;
	}
}
