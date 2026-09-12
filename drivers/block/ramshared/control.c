// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared - IOCTL control operations and bounds checking
 *
 * Copyright (C) 2026 Emerson Busson
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/blkdev.h>
#include <linux/uaccess.h>
#include "ramshared.h"

int ramshared_ioctl(struct block_device *bdev, fmode_t mode,
		    unsigned int cmd, unsigned long arg)
{
	struct ramshared_device *rs_dev;
	struct ramshared_info info;

	if (!bdev || !bdev->bd_disk || !bdev->bd_disk->private_data)
		return -ENODEV;

	rs_dev = bdev->bd_disk->private_data;

	switch (cmd) {
	case RAMSHARED_IOC_GET_INFO:
		if (_IOC_SIZE(cmd) < sizeof(struct ramshared_info))
			return -EINVAL;

		info.capacity_bytes = rs_dev->capacity_bytes;
		info.queue_depth = rs_dev->tag_set.queue_depth;
		info.reserved = 0;

		if (copy_to_user((void __user *)arg, &info, sizeof(info)))
			return -EFAULT;
		return 0;

	case RAMSHARED_IOC_SET_PARAM:
	{
		struct ramshared_param param;

		if (_IOC_SIZE(cmd) != sizeof(struct ramshared_param))
			return -EINVAL;

		if (copy_from_user(&param, (void __user *)arg, sizeof(param)))
			return -EFAULT;

		/* Currently unsupported */
		return -EOPNOTSUPP;
	}

	default:
		return -ENOTTY;
	}
}
