#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/string.h>

static unsigned long test_var;
static int my_set(const char *val, const struct kernel_param *kp) {
    return param_set_ulong(val, kp);
}
