# SPDX-License-Identifier: GPL-2.0-only
#
# Makefile for the RamShared in-tree kernel driver.
#

obj-$(CONFIG_BLK_DEV_RAMSHARED) += ramshared.o

ramshared-y := main.o dma.o queue.o

# Standalone build support
KDIR ?= /lib/modules/$(shell uname -r)/build
PWD := $(shell pwd)

default:
	$(MAKE) -C $(KDIR) M=$(PWD) CONFIG_BLK_DEV_RAMSHARED=m modules

clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean
