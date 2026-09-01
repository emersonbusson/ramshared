/* SPDX-License-Identifier: GPL-2.0-only */
#ifndef _RAMSHARED_H
#define _RAMSHARED_H

#include <linux/types.h>
#include <linux/blkdev.h>
#include <linux/blk-mq.h>
#include <linux/pci.h>
#include <linux/mutex.h>
#include <linux/atomic.h>
#include <linux/ioctl.h>

#define RAMSHARED_DRIVER_NAME		"ramshared"
#define RAMSHARED_DRIVER_VERSION	"0.9.0-beta.2"
#define RAMSHARED_SECTOR_SIZE		512
#define RAMSHARED_SECTOR_SHIFT		9
#define RAMSHARED_DEFAULT_QUEUE_DEPTH	256
#define RAMSHARED_MMIO_POISON_VALUE	0xFFFFFFFFU

#define RAMSHARED_IOCTL_MAGIC 'R'

/* Payload structures for IOCTLs (matching protocol.h ABI) */
struct ramshared_register {
	u32 abi_version;
	u32 disk_id;
	u32 queue_depth;
	u32 block_size;
	u32 max_io_bytes;
	u32 reserved;
	u64 sq_ring_va;
	u64 cq_ring_va;
	u64 data_area_va;
	u64 data_area_len;
	u64 sq_event_handle;
	u64 cq_event_handle;
};

struct ramshared_disk_params {
	u64 size_bytes;
	u32 block_size;
	u32 reserved;
	u8 serial[16];
};

/* Define IOCTL numbers with magic, direction, and size */
#define RAMSHARED_IOC_REGISTER_QUEUE	_IOW(RAMSHARED_IOCTL_MAGIC, 0, struct ramshared_register)
#define RAMSHARED_IOC_UNREGISTER_QUEUE	_IO(RAMSHARED_IOCTL_MAGIC, 1)
#define RAMSHARED_IOC_COMMIT_AND_FETCH	_IO(RAMSHARED_IOCTL_MAGIC, 2)
#define RAMSHARED_IOC_CREATE_DISK	_IOW(RAMSHARED_IOCTL_MAGIC, 3, struct ramshared_disk_params)
#define RAMSHARED_IOC_DESTROY_DISK	_IO(RAMSHARED_IOCTL_MAGIC, 4)

/**
 * struct ramshared_dma_region - PCIe DMA memory descriptor
 * @pci_addr: Physical PCIe bus address
 * @cpu_addr: Kernel virtual address mapped with write-combining
 * @size: Size of the allocated region in bytes
 * @dma_handle: DMA bus address mapping
 */
struct ramshared_dma_region {
	phys_addr_t	pci_addr;
	void __iomem	*cpu_addr;
	size_t		size;
	dma_addr_t	dma_handle;
};

/**
 * struct ramshared_device - Main in-tree block device state
 * @disk: gendisk descriptor
 * @tag_set: blk-mq tag set
 * @dma: DMA memory region
 * @capacity_bytes: Total available VRAM capacity in bytes
 * @dev: Pointer to underlying struct device
 * @lock: Mutex protecting device state transitions
 * @dma_transfers_total: Diagnostic counter for completed transfers
 * @read_bytes: Total bytes read
 * @write_bytes: Total bytes written
 */
struct ramshared_device {
	struct gendisk			*disk;
	struct blk_mq_tag_set		tag_set;
	struct ramshared_dma_region	dma;
	u64				capacity_bytes;
	struct device			*dev;
	struct mutex			lock;
	atomic64_t			dma_transfers_total;
	atomic64_t			read_bytes;
	atomic64_t			write_bytes;
};

int ramshared_dma_init(struct ramshared_device *rs_dev, struct pci_dev *pdev);
void ramshared_dma_cleanup(struct ramshared_device *rs_dev);
int ramshared_queue_init(struct ramshared_device *rs_dev, struct device *parent_dev, unsigned int q_depth);
void ramshared_queue_cleanup(struct ramshared_device *rs_dev);

#endif /* _RAMSHARED_H */
