sed -i '/int ramshared_ioctl/d' drivers/block/ramshared/ramshared.h
sed -i '/unsigned int cmd/d' drivers/block/ramshared/ramshared.h
sed -i '/#endif \/\* _RAMSHARED_H \*\//i \int ramshared_ioctl(struct block_device *bdev, blk_mode_t mode,\n\t\t    unsigned int cmd, unsigned long arg);\n' drivers/block/ramshared/ramshared.h
