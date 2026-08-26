// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared - Direct PCIe DMA memory management for discrete GPU VRAM
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/pci.h>
#include <linux/dma-mapping.h>
#include <linux/io.h>
#include "ramshared.h"

int ramshared_dma_init(struct ramshared_device *rs_dev, size_t size)
{
	if (!rs_dev || size == 0)
		return -EINVAL;

	rs_dev->dma.size = size;
	rs_dev->dma.cpu_addr = NULL;
	rs_dev->dma.pci_addr = 0;

	pr_info("%s: DMA engine initialized with %zu bytes\n",
		RAMSHARED_DRIVER_NAME, size);

	return 0;
}

void ramshared_dma_cleanup(struct ramshared_device *rs_dev)
{
	if (!rs_dev)
		return;

	if (rs_dev->dma.cpu_addr) {
		iounmap(rs_dev->dma.cpu_addr);
		rs_dev->dma.cpu_addr = NULL;
	}

	rs_dev->dma.size = 0;
	pr_info("%s: DMA engine released\n", RAMSHARED_DRIVER_NAME);
}
