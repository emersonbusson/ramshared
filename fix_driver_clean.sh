#!/usr/bin/env bash

# Fix main.c format warning and unused label
sed -i 's/%lu/%u/g' drivers/block/ramshared/main.c
sed -i '/err_disable_pci:/d' drivers/block/ramshared/main.c
sed -i '/pci_disable_device(pdev);/d' drivers/block/ramshared/main.c

# Fix queue.c
# Add missing dev_attr_dma_transfers_total
sed -i '/static DEVICE_ATTR_RO(write_bytes);/a\
\
static ssize_t dma_transfers_total_show(struct device *dev,\
\t\t\t\t\tstruct device_attribute *attr, char *buf)\
{\
\tstruct gendisk *disk = dev_to_disk(dev);\
\tstruct ramshared_device *rs_dev = disk->private_data;\
\n\treturn sysfs_emit(buf, "%lld\\n", atomic64_read(\&rs_dev->dma_transfers_total));\
}\
static DEVICE_ATTR_RO(dma_transfers_total);' drivers/block/ramshared/queue.c

# Remove rw_page
sed -i '/static int ramshared_bdev_rw_page(/,/^}/d' drivers/block/ramshared/queue.c
sed -i '/\.rw_page.*=/d' drivers/block/ramshared/queue.c

# Export ramshared_attr_groups for main.c
sed -i 's/static const struct attribute_group \*ramshared_attr_groups/const struct attribute_group \*ramshared_attr_groups/g' drivers/block/ramshared/queue.c

# Use device_add_disk instead of add_disk in main.c
sed -i 's/add_disk(rs_dev->disk)/device_add_disk(\&pdev->dev, rs_dev->disk, ramshared_attr_groups)/g' drivers/block/ramshared/main.c

# Declare ramshared_attr_groups in ramshared.h
sed -i '/int ramshared_dma_init/i extern const struct attribute_group *ramshared_attr_groups[];\n' drivers/block/ramshared/ramshared.h

# Remove disk_groups and parent assignment from queue.c
sed -i '/rs_dev->disk->disk_groups/d' drivers/block/ramshared/queue.c
sed -i '/rs_dev->disk->parent/d' drivers/block/ramshared/queue.c
