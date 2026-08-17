#!/bin/busybox sh
# Minimal isolated guest for WSL config capability validation.
set -eu

fail() {
  echo "KTEST-FAIL=$1"
  /bin/busybox poweroff -f
  exit 1
}

/bin/busybox mkdir -p /proc /sys /dev /tmp
/bin/busybox mount -t proc proc /proc
/bin/busybox mount -t sysfs sysfs /sys
/bin/busybox mount -t devtmpfs devtmpfs /dev

echo 'KTEST-BEGIN'
echo "KTEST-UNAME=$(/bin/busybox uname -r)"
test ! -e /dev/ublk-control || fail ublk_autoloaded
test ! -e /sys/block/zram0 || fail zram_autoloaded
echo 'KTEST-PRELOAD=inactive'

/bin/busybox insmod /modules/zsmalloc.ko || fail zsmalloc_insmod
/bin/busybox insmod /modules/zram.ko num_devices=1 || fail zram_insmod
/bin/busybox insmod /modules/ublk_drv.ko || fail ublk_insmod

test -c /dev/ublk-control || fail ublk_control_missing
test -e /sys/block/zram0/backing_dev || fail zram_backing_dev_missing
test -e /sys/block/zram0/writeback || fail zram_writeback_missing
test -b /dev/sda1 || fail backing_partition_missing
echo 'KTEST-UBLK-CONTROL=present'
echo 'KTEST-ZRAM-WRITEBACK-INTERFACE=present'

echo /dev/sda1 > /sys/block/zram0/backing_dev || fail backing_dev_attach
echo 16M > /sys/block/zram0/disksize || fail zram_disksize
/bin/busybox dd if=/dev/urandom of=/dev/zram0 bs=4096 count=1024 2>/dev/null ||
  fail zram_write
echo all > /sys/block/zram0/idle || fail zram_idle_mark
echo idle > /sys/block/zram0/writeback || fail zram_writeback

set -- $(/bin/busybox cat /sys/block/zram0/bd_stat)
test "$1" -gt 0 || fail zram_bd_count_zero
test "$3" -gt 0 || fail zram_bd_writes_zero
echo "KTEST-ZRAM-BD-STAT=$1,$2,$3"
echo 'KTEST-ZRAM-WRITEBACK-IO=pass'

echo 1 > /sys/block/zram0/reset || fail zram_reset
/bin/busybox rmmod ublk_drv || fail ublk_rmmod
/bin/busybox rmmod zram || fail zram_rmmod
/bin/busybox rmmod zsmalloc || fail zsmalloc_rmmod
test ! -e /dev/ublk-control || fail ublk_control_residual
test ! -e /sys/block/zram0 || fail zram_residual
echo 'KTEST-CLEANUP=pass'
echo 'KTEST-END'
/bin/busybox poweroff -f
