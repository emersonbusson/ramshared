// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared - Direct PCIe DMA memory management for discrete GPU VRAM
 *
 * Copyright (C) 2026 Emerson Busson
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/pci.h>
#include <linux/dma-mapping.h>
#include <linux/io.h>
#include "ramshared.h"

int ramshared_dma_init(struct ramshared_device *rs_dev, struct pci_dev *pdev)
{
	int bar = 0;
	resource_size_t bar_start, bar_len;

	/* Validate pointers at the top of the function */
	if (!rs_dev || !pdev)
		return -EINVAL;

	bar_start = pci_resource_start(pdev, bar);
	bar_len = pci_resource_len(pdev, bar);

	if (!bar_start || bar_len == 0) {
		dev_err(&pdev->dev, "invalid PCIe BAR0 resource\n");
		return -ENODEV;
	}

	rs_dev->dma.pci_addr = bar_start;
	rs_dev->dma.size = min_t(size_t, bar_len, rs_dev->capacity_bytes);

	/* Map PCIe VRAM BAR using Write-Combining for peak throughput */
	rs_dev->dma.cpu_addr = devm_ioremap_wc(&pdev->dev, rs_dev->dma.pci_addr,
					       rs_dev->dma.size);
	if (!rs_dev->dma.cpu_addr) {
		dev_err(&pdev->dev, "failed to ioremap_wc VRAM BAR0 (%zu bytes)\n",
			rs_dev->dma.size);
		return -ENOMEM;
	}

	dev_info(&pdev->dev, "DMA engine mapped %zu MB VRAM at %pa\n",
		 rs_dev->dma.size >> 20, &rs_dev->dma.pci_addr);

	return 0;
}

void ramshared_dma_cleanup(struct ramshared_device *rs_dev)
{
	if (!rs_dev)
		return;

	rs_dev->dma.cpu_addr = NULL;
	rs_dev->dma.size = 0;
}
