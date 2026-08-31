#include <linux/module.h>
#include <linux/moduleparam.h>
#include <linux/kernel.h>
#include <linux/string.h>
#include <linux/errno.h>

static unsigned long capacity_mb = 1024;

static int capacity_mb_set(const char *val, const struct kernel_param *kp)
{
	unsigned long new_cap;
	size_t len;
	int ret;

	if (!val)
		return -EINVAL;

	len = strlen(val);
	if (len == 0 || len > 32)
		return -EINVAL;

	ret = kstrtoul(val, 0, &new_cap);
	if (ret)
		return ret;

	if (new_cap == 0 || new_cap > (1UL << 20))
		return -ERANGE;

	return param_set_ulong(val, kp);
}

static const struct kernel_param_ops capacity_mb_ops = {
	.set = capacity_mb_set,
	.get = param_get_ulong,
};
module_param_cb(capacity_mb, &capacity_mb_ops, &capacity_mb, 0644);
