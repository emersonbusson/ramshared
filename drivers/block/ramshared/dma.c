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

	if (!rs_dev || !pdev)
		return -EINVAL;

	bar_start = pci_resource_start(pdev, bar);
	bar_len = pci_resource_len(pdev, bar);

	if (!bar_start) {
		dev_err(&pdev->dev, "invalid PCIe BAR0 resource\n");
		return -ENODEV;
	}

	if (bar_len == 0) {
		dev_err(&pdev->dev, "PCIe BAR0 length is zero\n");
		return -ENODEV;
	}

	/* SPEC: kernel-pci-bar-capacity-contract §RF-2. */
	if ((u64)bar_len < rs_dev->capacity_bytes) {
		dev_err(&pdev->dev,
			"BAR0 is smaller than requested capacity (%llu < %llu)\n",
			(unsigned long long)bar_len,
			(unsigned long long)rs_dev->capacity_bytes);
		return -ERANGE;
	}

	if (rs_dev->capacity_bytes > SIZE_MAX) {
		dev_err(&pdev->dev, "requested capacity exceeds mapping width\n");
		return -EOVERFLOW;
	}

	if (dma_set_mask(&pdev->dev, DMA_BIT_MASK(64))) {
		dev_warn(&pdev->dev, "64-bit DMA mask failed, attempting 32-bit\n");
		if (dma_set_mask(&pdev->dev, DMA_BIT_MASK(32))) {
			dev_err(&pdev->dev, "no usable DMA mask configuration\n");
			return -EFAULT;
		}
	}

	if (dma_set_coherent_mask(&pdev->dev, DMA_BIT_MASK(64))) {
		dev_warn(&pdev->dev, "64-bit coherent DMA mask failed, attempting 32-bit\n");
		if (dma_set_coherent_mask(&pdev->dev, DMA_BIT_MASK(32))) {
			dev_err(&pdev->dev, "no usable coherent DMA mask configuration\n");
			return -EFAULT;
		}
	}

	rs_dev->dma.pci_addr = bar_start;
	rs_dev->dma.size = (size_t)rs_dev->capacity_bytes;

	if (!IS_ALIGNED(rs_dev->dma.pci_addr, PAGE_SIZE)) {
		dev_err(&pdev->dev, "PCIe BAR0 address not %lu-byte aligned\n", PAGE_SIZE);
		return -EINVAL;
	}

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

	if (rs_dev->dma.cpu_addr) {
		rs_dev->dma.cpu_addr = NULL;
		rs_dev->dma.size = 0;
	}
}
