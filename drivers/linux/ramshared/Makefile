# SPDX-License-Identifier: GPL-2.0-only
#
# Makefile for the Linux RamShared VRAM Block Driver
#

obj-$(CONFIG_BLK_DEV_RAMSHARED) += ramshared.o

ramshared-y := ramshared_main.o ramshared_vram.o ramshared_tier.o

# Support out-of-tree builds against running kernel headers
KDIR ?= /lib/modules/$(shell uname -r)/build
PWD  := $(shell pwd)

default:
	$(MAKE) -C $(KDIR) M=$(PWD) modules

clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean
