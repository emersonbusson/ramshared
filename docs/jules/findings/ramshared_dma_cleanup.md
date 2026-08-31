# Findings: ramshared_dma_cleanup Fix

## Proposed Fix
The function `ramshared_dma_cleanup` in `drivers/block/ramshared/dma.c` requires an explicit unmap operation guarded by a null check. The `struct ramshared_device` in `ramshared.h` has a field `struct device *dev;` which is required for `devm_iounmap(rs_dev->dev, rs_dev->dma.cpu_addr)`.

The fix should modify the function as follows:
```c
void ramshared_dma_cleanup(struct ramshared_device *rs_dev)
{
	if (!rs_dev)
		return;

	if (rs_dev->dma.cpu_addr) {
		devm_iounmap(rs_dev->dev, rs_dev->dma.cpu_addr);
		rs_dev->dma.cpu_addr = NULL;
	}

	rs_dev->dma.size = 0;
}
```

## Compilation Issues
The kernel build environment is missing (no `/lib/modules/$(uname -r)/build`), and there is no mock or unit test framework for this C code within the repository.

Per the immutable contracts, we cannot merge C code modifications without clean compilation and testing. Therefore, we are providing a FINDING_ONLY report.
