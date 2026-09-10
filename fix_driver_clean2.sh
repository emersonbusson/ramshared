#!/usr/bin/env bash
# Re-fix the format strings to use %lu in main.c
sed -i 's/%u MiB)/%lu MiB)/g' drivers/block/ramshared/main.c
sed -i 's/%u\\n"/%lu\\n"/g' drivers/block/ramshared/main.c

# Fix queue.c sparse warnings: blk_opf_t op
sed -i 's/unsigned int op = bio_op(bio);/blk_opf_t op = bio_op(bio);/g' drivers/block/ramshared/queue.c
