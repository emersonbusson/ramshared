// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared VRAM Block Driver — PCIe GPU VRAM Backend
 *
 * Copyright (C) 2026 Emerson Busson 
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/pci.h>
#include <linux/io.h>
#include <linux/highmem.h>
#include <linux/slab.h>
#include "ramshared_driver.h"

/*
 * Discover and map GPU VRAM PCIe BAR aperture with Write-Combining (WC)
 * to maximize PCIe throughput on memory tier transfers.
 */
int ramshared_vram_init(struct ramshared_device *dev)
{
	struct pci_dev *pdev = NULL;
	int bar = 1; /* Standard Resizable BAR / Direct VRAM Aperture */
	size_t chunk_idx;

	/* Find first discrete VGA/3D accelerator (NVIDIA, AMD, Intel dGPU) */
	while ((pdev = pci_get_class(PCI_CLASS_DISPLAY_VGA << 8, pdev)) != NULL ||
	       (pdev = pci_get_class(PCI_CLASS_DISPLAY_3D << 8, pdev)) != NULL) {
		if (pci_resource_len(pdev, bar) >= RAMSHARED_CHUNK_SIZE)
			break;
	}

	if (!pdev) {
		pr_warn("ramshared: No suitable PCIe GPU with large BAR found; running in SSD-only tier\n");
		dev->state = RAMSHARED_TIER_SSD_ONLY;
		return 0;
	}

	dev->pdev = pdev;
	dev->vram_bar_start = pci_resource_start(pdev, bar);
	dev->vram_bar_len = pci_resource_len(pdev, bar);

	/* Cap to requested logical capacity or physical BAR size */
	dev->vram_allocated = min_t(resource_size_t, dev->vram_bar_len,
				    dev->logical_capacity);

	/* Map PCIe aperture with Write-Combining attributes */
	dev->vram_bar_base = pci_iomap_wc(pdev, bar, dev->vram_allocated);
	if (!dev->vram_bar_base) {
		pr_err("ramshared: Failed to ioremap PCIe BAR%d at %pa (%llu bytes)\n",
		       bar, &dev->vram_bar_start, (u64)dev->vram_allocated);
		pci_dev_put(pdev);
		dev->pdev = NULL;
		dev->state = RAMSHARED_TIER_SSD_ONLY;
		return -ENOMEM;
	}

	/* Allocate chunk descriptor table */
	dev->num_chunks = dev->vram_allocated / RAMSHARED_CHUNK_SIZE;
	if (dev->num_chunks == 0)
		dev->num_chunks = 1;

	dev->chunks = kcalloc(dev->num_chunks, sizeof(struct ramshared_vram_chunk),
			      GFP_KERNEL);
	if (!dev->chunks) {
		pci_iounmap(pdev, dev->vram_bar_base);
		pci_dev_put(pdev);
		dev->pdev = NULL;
		return -ENOMEM;
	}

	for (chunk_idx = 0; chunk_idx < dev->num_chunks; chunk_idx++) {
		struct ramshared_vram_chunk *chunk = &dev->chunks[chunk_idx];

		chunk->physical_offset = chunk_idx * RAMSHARED_CHUNK_SIZE;
		chunk->vaddr = dev->vram_bar_base + chunk->physical_offset;
		chunk->generation = 1;
		chunk->valid = true;
		spin_lock_init(&chunk->lock);
	}

	dev->state = RAMSHARED_TIER_ONLINE;
	pr_info("ramshared: GPU VRAM tier initialized: %pa (%llu MiB across %zu chunks)\n",
		&dev->vram_bar_start, (u64)(dev->vram_allocated >> 20), dev->num_chunks);

	return 0;
}

void ramshared_vram_cleanup(struct ramshared_device *dev)
{
	if (dev->chunks) {
		kfree(dev->chunks);
		dev->chunks = NULL;
	}

	if (dev->vram_bar_base && dev->pdev) {
		pci_iounmap(dev->pdev, dev->vram_bar_base);
		dev->vram_bar_base = NULL;
	}

	if (dev->pdev) {
		pci_dev_put(dev->pdev);
		dev->pdev = NULL;
	}

	dev->state = RAMSHARED_TIER_OFFLINE;
}

/*
 * High-speed VRAM page transfer function via PCIe WC mapping.
 */
int ramshared_vram_transfer(struct ramshared_device *dev, struct bio_vec *bvec,
			   sector_t sector, enum req_op op)
{
	u64 offset = sector << RAMSHARED_SECTOR_SHIFT;
	size_t len = bvec->bv_len;
	size_t chunk_idx;
	u64 chunk_offset;
	void __iomem *vram_ptr;
	void *mem_ptr;
	unsigned long flags;

	if (offset + len > dev->vram_allocated)
		return -EIO;

	chunk_idx = offset / RAMSHARED_CHUNK_SIZE;
	chunk_offset = offset % RAMSHARED_CHUNK_SIZE;

	if (chunk_idx >= dev->num_chunks)
		return -EIO;

	vram_ptr = dev->chunks[chunk_idx].vaddr + chunk_offset;

	mem_ptr = kmap_local_page(bvec->bv_page) + bvec->bv_offset;

	spin_lock_irqsave(&dev->chunks[chunk_idx].lock, flags);

	if (op == REQ_OP_WRITE) {
		/* Host to Device (H2D) Write */
		memcpy_toio(vram_ptr, mem_ptr, len);
		atomic64_add(len, &dev->stats.vram_write_bytes);
		dev->chunks[chunk_idx].valid = true;
	} else if (op == REQ_OP_READ) {
		/* Device to Host (D2H) Read */
		if (dev->chunks[chunk_idx].valid) {
			memcpy_fromio(mem_ptr, vram_ptr, len);
			atomic64_add(len, &dev->stats.vram_read_bytes);
			atomic64_inc(&dev->stats.vram_hits);
		} else {
			/* Cache Miss: zero page or trigger fallback */
			memset(mem_ptr, 0, len);
			atomic64_inc(&dev->stats.vram_misses);
		}
	}

	spin_unlock_irqrestore(&dev->chunks[chunk_idx].lock, flags);

	kunmap_local(mem_ptr);
	return 0;
}
