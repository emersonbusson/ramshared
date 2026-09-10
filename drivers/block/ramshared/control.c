// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared - IOCTL and Control Interface
 *
 * Copyright (C) 2026 Emerson Busson
 */

#include <linux/module.h>
#include <linux/blkdev.h>
#include <linux/compat.h>
#include <linux/uaccess.h>
#include <linux/overflow.h>
#include "ramshared.h"

static int ramshared_get_info(struct ramshared_device *rs_dev,
			      struct ramshared_info __user *argp)
{
	struct ramshared_info info;

	if (!argp)
		return -EINVAL;

	memset(&info, 0, sizeof(info));
	info.capacity_bytes = rs_dev->capacity_bytes;
	info.read_bytes = atomic64_read(&rs_dev->read_bytes);
	info.write_bytes = atomic64_read(&rs_dev->write_bytes);
	info.queue_depth = rs_dev->tag_set.queue_depth;

	if (copy_to_user(argp, &info, sizeof(info)))
		return -EFAULT;

	return 0;
}

int ramshared_ioctl(struct block_device *bdev, fmode_t mode,
		    unsigned int cmd, unsigned long arg)
{
	struct ramshared_device *rs_dev;
	void __user *argp = (void __user *)arg;

	if (!bdev || !bdev->bd_disk || !bdev->bd_disk->private_data)
		return -ENODEV;

	rs_dev = bdev->bd_disk->private_data;

	switch (cmd) {
	case RAMSHARED_IOC_GET_INFO:
		return ramshared_get_info(rs_dev, argp);
	default:
		return -ENOTTY;
	}
}

#ifdef CONFIG_COMPAT
int ramshared_compat_ioctl(struct block_device *bdev, fmode_t mode,
			   unsigned int cmd, unsigned long arg)
{
	struct ramshared_device *rs_dev;
	void __user *argp = compat_ptr(arg);

	if (!bdev || !bdev->bd_disk || !bdev->bd_disk->private_data)
		return -ENODEV;

	rs_dev = bdev->bd_disk->private_data;

	/*
	 * Map 32-bit ioctl calls cleanly without data truncation.
	 * RAMSHARED_IOC_GET_INFO structure is identical in 32-bit and 64-bit
	 * due to explicit padding, so we can route directly to the native handler
	 * using the translated pointer.
	 */
	switch (cmd) {
	case RAMSHARED_IOC_GET_INFO:
		return ramshared_get_info(rs_dev, argp);
	default:
		return -ENOTTY;
	}
}
#endif
