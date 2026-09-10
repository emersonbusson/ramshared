# SPDX-License-Identifier: GPL-2.0-only
#
# Makefile for the RamShared in-tree kernel driver.
#

obj-$(CONFIG_BLK_DEV_RAMSHARED) += ramshared.o

ramshared-y := main.o dma.o queue.o

# Standalone build support
KDIR ?= /lib/modules/$(shell uname -r)/build
PWD := $(shell pwd)

RAMSHARED_VERSION ?= $(shell git describe --tags --dirty --always 2>/dev/null || cat $(src)/VERSION 2>/dev/null || cat $(PWD)/VERSION 2>/dev/null || echo "unknown")
ccflags-y += -DRAMSHARED_VERSION=\"$(RAMSHARED_VERSION)\"

default:
	$(MAKE) -C $(KDIR) M=$(PWD) CONFIG_BLK_DEV_RAMSHARED=m modules

clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean || true
	rm -f *.o *.ko *.cmd *.mod *.mod.c modules.order Module.symvers .*.cmd
