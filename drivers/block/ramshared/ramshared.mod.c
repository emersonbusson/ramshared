#include <linux/module.h>
#define INCLUDE_VERMAGIC
#include <linux/build-salt.h>
#include <linux/elfnote-lto.h>
#include <linux/export-internal.h>
#include <linux/vermagic.h>
#include <linux/compiler.h>

#ifdef CONFIG_UNWINDER_ORC
#include <asm/orc_header.h>
ORC_HEADER;
#endif

BUILD_SALT;
BUILD_LTO_INFO;

MODULE_INFO(vermagic, VERMAGIC_STRING);
MODULE_INFO(name, KBUILD_MODNAME);

__visible struct module __this_module
__section(".gnu.linkonce.this_module") = {
	.name = KBUILD_MODNAME,
	.init = init_module,
#ifdef CONFIG_MODULE_UNLOAD
	.exit = cleanup_module,
#endif
	.arch = MODULE_ARCH_INIT,
};

#ifdef CONFIG_MITIGATION_RETPOLINE
MODULE_INFO(retpoline, "Y");
#endif



static const char ____versions[]
__used __section("__versions") =
	"\x18\x00\x00\x00\xf1\x96\x0d\x68"
	"param_ops_uint\0\0"
	"\x18\x00\x00\x00\x28\x17\x6b\xfd"
	"param_ops_ulong\0"
	"\x28\x00\x00\x00\x42\x40\xc6\x6b"
	"blk_queue_logical_block_size\0\0\0\0"
	"\x28\x00\x00\x00\x73\x2b\xcf\x4a"
	"pci_request_selected_regions\0\0\0\0"
	"\x28\x00\x00\x00\x97\x8d\x66\xc2"
	"pci_release_selected_regions\0\0\0\0"
	"\x18\x00\x00\x00\x19\x53\xe1\x51"
	"devm_kmalloc\0\0\0\0"
	"\x1c\x00\x00\x00\x20\x06\x0d\xc6"
	"__num_online_cpus\0\0\0"
	"\x20\x00\x00\x00\x43\x0b\x9f\xf9"
	"pci_enable_device_mem\0\0\0"
	"\x24\x00\x00\x00\xb4\xe0\xd3\x09"
	"blk_queue_max_segment_size\0\0"
	"\x18\x00\x00\x00\x0f\x40\x8a\xc2"
	"device_add_disk\0"
	"\x20\x00\x00\x00\xe8\x66\x72\xcc"
	"__pci_register_driver\0\0\0"
	"\x18\x00\x00\x00\x44\x84\x62\x7d"
	"memcpy_fromio\0\0\0"
	"\x20\x00\x00\x00\xab\x08\x8e\x3c"
	"blk_queue_dma_alignment\0"
	"\x1c\x00\x00\x00\x88\x73\x3a\x59"
	"blk_mq_end_request\0\0"
	"\x1c\x00\x00\x00\x82\xfb\x44\xba"
	"__blk_mq_alloc_disk\0"
	"\x20\x00\x00\x00\x14\x68\xfd\xef"
	"pci_unregister_driver\0\0\0"
	"\x14\x00\x00\x00\xbb\x6d\xfb\xbd"
	"__fentry__\0\0"
	"\x14\x00\x00\x00\x61\xe2\x83\xe7"
	"sysfs_emit\0\0"
	"\x20\x00\x00\x00\x90\x5a\xac\x06"
	"blk_mq_alloc_tag_set\0\0\0\0"
	"\x14\x00\x00\x00\x9c\x42\xe7\xb9"
	"memcpy_toio\0"
	"\x14\x00\x00\x00\x98\x3a\xd6\x61"
	"put_disk\0\0\0\0"
	"\x18\x00\x00\x00\x81\xc8\x24\x1d"
	"___ratelimit\0\0\0\0"
	"\x28\x00\x00\x00\xfb\x6f\x7c\xc9"
	"blk_queue_physical_block_size\0\0\0"
	"\x1c\x00\x00\x00\x9d\x02\x6d\x73"
	"blk_queue_flag_set\0\0"
	"\x14\x00\x00\x00\x90\xf6\x65\xb1"
	"_dev_info\0\0\0"
	"\x18\x00\x00\x00\xac\xbe\x78\xeb"
	"pci_select_bars\0"
	"\x1c\x00\x00\x00\x5e\xd7\xd8\x7c"
	"page_offset_base\0\0\0\0"
	"\x1c\x00\x00\x00\xa2\xc3\x3a\xa1"
	"pci_clear_master\0\0\0\0"
	"\x14\x00\x00\x00\x45\xec\x71\x76"
	"_dev_err\0\0\0\0"
	"\x18\x00\x00\x00\x5e\x50\x6a\x2f"
	"set_capacity\0\0\0\0"
	"\x1c\x00\x00\x00\x6b\x1f\xff\x00"
	"blk_mq_free_tag_set\0"
	"\x14\x00\x00\x00\xdf\x40\x41\x81"
	"del_gendisk\0"
	"\x28\x00\x00\x00\x06\x27\x15\x28"
	"blk_queue_max_discard_sectors\0\0\0"
	"\x18\x00\x00\x00\x9f\x0c\xfb\xce"
	"__mutex_init\0\0\0\0"
	"\x1c\x00\x00\x00\xcf\x68\x6c\x42"
	"pci_restore_state\0\0\0"
	"\x14\x00\x00\x00\x74\xce\xda\x86"
	"_dev_warn\0\0\0"
	"\x18\x00\x00\x00\xb4\x44\x75\x2e"
	"pci_set_master\0\0"
	"\x20\x00\x00\x00\xf9\xa3\x24\x26"
	"blk_queue_max_segments\0\0"
	"\x1c\x00\x00\x00\xca\x39\x82\x5b"
	"__x86_return_thunk\0\0"
	"\x20\x00\x00\x00\xb1\x13\x32\x71"
	"dma_set_coherent_mask\0\0\0"
	"\x18\x00\x00\x00\x6c\x1e\x65\x97"
	"vmemmap_base\0\0\0\0"
	"\x1c\x00\x00\x00\xc5\xbc\xb8\x80"
	"blk_queue_io_min\0\0\0\0"
	"\x20\x00\x00\x00\x5f\xde\x2b\x7a"
	"blk_mq_start_request\0\0\0\0"
	"\x18\x00\x00\x00\xd2\x17\x93\xa4"
	"dma_set_mask\0\0\0\0"
	"\x18\x00\x00\x00\x91\xcb\xb7\x49"
	"devm_ioremap_wc\0"
	"\x14\x00\x00\x00\x0b\x1c\x19\xa4"
	"memset_io\0\0\0"
	"\x24\x00\x00\x00\x64\x5d\x21\xfc"
	"blk_queue_max_hw_sectors\0\0\0\0"
	"\x1c\x00\x00\x00\x88\x3f\xfc\x16"
	"blk_queue_io_opt\0\0\0\0"
	"\x18\x00\x00\x00\x13\xc9\x9a\x0d"
	"module_layout\0\0\0"
	"\x00\x00\x00\x00\x00\x00\x00\x00";

MODULE_INFO(depends, "");

MODULE_ALIAS("pci:v*d*sv*sd*bc03sc00i*");
MODULE_ALIAS("pci:v*d*sv*sd*bc03sc80i*");
MODULE_ALIAS("pci:v*d*sv*sd*bc12sc00i*");

MODULE_INFO(srcversion, "7D93E942D10194F81E7D8B8");
