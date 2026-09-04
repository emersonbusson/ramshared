// SPDX-License-Identifier: GPL-2.0-only
/*
 * RamShared - Hardware-Accelerated VRAM Block Driver
 *
 * Copyright (C) 2026 Emerson Busson
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/init.h>
#include <linux/pci.h>
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
MODULE_PARM_DESC(queue_depth, "Hardware queue depth (default: 256)");

static int ramshared_pci_probe(struct pci_dev *pdev,
			       const struct pci_device_id *id)
{
	struct ramshared_device *rs_dev;
	resource_size_t bar_len;
	int ret;

	if (!pdev || !id)
		return -EINVAL;

	dev_info(&pdev->dev, "probing RamShared hardware (capacity=%lu MiB)\n",
		 capacity_mb);

	if (capacity_mb == 0 || capacity_mb > (1UL << 20)) {
		dev_err(&pdev->dev, "capacity_mb (%lu MiB) invalid or exceeds 1 TiB maximum\n",
			capacity_mb);
		return -ERANGE;
	}

	bar_len = pci_resource_len(pdev, 0);
	if (!bar_len) {
		dev_err(&pdev->dev, "PCIe BAR0 length is zero or missing\n");
		return -ENODEV;
	}

	if (((u64)capacity_mb << 20) > bar_len) {
		dev_err(&pdev->dev, "capacity_mb (%lu MiB) exceeds physical PCIe BAR0 aperture (%llu bytes)\n",
			capacity_mb, (unsigned long long)bar_len);
		return -ERANGE;
	}

	if (queue_depth < 1 || queue_depth > 4096) {
		dev_warn(&pdev->dev, "clamping queue_depth (%lu) to default (256)\n", queue_depth);
		queue_depth = 256;
	}

	rs_dev = devm_kzalloc(&pdev->dev, sizeof(*rs_dev), GFP_KERNEL);
	if (!rs_dev)
		return -ENOMEM;

	rs_dev->dev = &pdev->dev;
	rs_dev->capacity_bytes = (u64)capacity_mb * 1024 * 1024;
	mutex_init(&rs_dev->lock);
	atomic64_set(&rs_dev->dma_transfers_total, 0);
	atomic64_set(&rs_dev->read_bytes, 0);
	atomic64_set(&rs_dev->write_bytes, 0);

	ret = pci_enable_device_mem(pdev);
	if (ret) {
		dev_err(&pdev->dev, "failed to enable PCIe memory device\n");
		return ret;
	}

	pci_set_master(pdev);

	ret = dma_set_mask_and_coherent(&pdev->dev, DMA_BIT_MASK(64));
	if (ret) {
		dev_warn(&pdev->dev, "64-bit DMA failed, attempting 32-bit DMA\n");
		ret = dma_set_mask_and_coherent(&pdev->dev, DMA_BIT_MASK(32));
		if (ret) {
			dev_err(&pdev->dev, "no usable DMA configuration\n");
			goto err_clear_master;
		}
	}

	ret = pci_request_mem_regions(pdev, RAMSHARED_DRIVER_NAME);
	if (ret) {
		dev_err(&pdev->dev, "failed to claim PCIe memory regions\n");
		goto err_clear_master;
	}

	ret = ramshared_dma_init(rs_dev, pdev);
	if (ret)
		goto err_release_regions;

	ret = ramshared_queue_init(rs_dev, &pdev->dev, queue_depth);
	if (ret)
		goto err_dma_cleanup;

	ret = add_disk(rs_dev->disk);
	if (ret) {
		dev_err(&pdev->dev, "failed to add block disk (err=%d)\n", ret);
		goto err_queue_cleanup;
	}

	pci_set_drvdata(pdev, rs_dev);
	dev_info(&pdev->dev, "block device /dev/%s registered successfully\n",
		 rs_dev->disk->disk_name);
	return 0;

err_queue_cleanup:
	ramshared_queue_cleanup(rs_dev);
err_dma_cleanup:
	ramshared_dma_cleanup(rs_dev);
err_release_regions:
	pci_release_mem_regions(pdev);
err_clear_master:
	pci_clear_master(pdev);
err_disable_pci:
	pci_disable_device(pdev);
	return ret;
}

static void ramshared_pci_remove(struct pci_dev *pdev)
{
	struct ramshared_device *rs_dev;

	if (!pdev)
		return;

	rs_dev = pci_get_drvdata(pdev);
	if (!rs_dev)
		return;

	ramshared_queue_cleanup(rs_dev);
	ramshared_dma_cleanup(rs_dev);
	pci_release_mem_regions(pdev);
	pci_clear_master(pdev);
	pci_disable_device(pdev);

	dev_info(&pdev->dev, "RamShared device removed successfully\n");
}

/* PCIe AER & Link Drop Error Handlers */
static pci_ers_result_t ramshared_pci_error_detected(struct pci_dev *pdev,
						     pci_channel_state_t state)
{
	dev_err(&pdev->dev, "PCIe error detected (state=%d)\n", state);
	if (state == pci_channel_io_frozen)
		return PCI_ERS_RESULT_NEED_RESET;
	return PCI_ERS_RESULT_CAN_RECOVER;
}

static pci_ers_result_t ramshared_pci_slot_reset(struct pci_dev *pdev)
{
	dev_info(&pdev->dev, "PCIe slot reset recovery initiated\n");
	pci_restore_state(pdev);
	return PCI_ERS_RESULT_RECOVERED;
}

static void ramshared_pci_resume(struct pci_dev *pdev)
{
	dev_info(&pdev->dev, "PCIe link resumed\n");
}

static const struct pci_error_handlers ramshared_pci_err_handler = {
	.error_detected = ramshared_pci_error_detected,
	.slot_reset     = ramshared_pci_slot_reset,
	.resume         = ramshared_pci_resume,
};

static const struct pci_device_id ramshared_pci_tbl[] = {
	{ PCI_DEVICE_CLASS(PCI_CLASS_DISPLAY_VGA << 8, 0xffff00) },
	{ PCI_DEVICE_CLASS(PCI_CLASS_DISPLAY_OTHER << 8, 0xffff00) },
	{ PCI_DEVICE_CLASS(PCI_CLASS_ACCELERATOR_PROCESSING << 8, 0xffff00) },
	{ 0, }
};
MODULE_DEVICE_TABLE(pci, ramshared_pci_tbl);

static struct pci_driver ramshared_pci_driver = {
	.name		= RAMSHARED_DRIVER_NAME,
	.id_table	= ramshared_pci_tbl,
	.probe		= ramshared_pci_probe,
	.remove		= ramshared_pci_remove,
	.err_handler	= &ramshared_pci_err_handler,
};

module_pci_driver(ramshared_pci_driver);
