1. RULES
   - Adhere to the immutable contract and architectural specifications.
   - Use guard clauses over nested if/else.
   - Apply physical sanity checks.
   - Return specific and semantic kernel errors.

2. MAIN_DIFF
   - Modify `drivers/block/ramshared/main.c` function `ramshared_pci_remove`.
   - Add `pci_clear_master(pdev);` to clean up `pci_set_master(pdev);` called in `ramshared_pci_probe`.
   - Ensure the sequence of cleanup exactly matches the reverse order of initialization in `ramshared_pci_probe` with guard checks on pointers.
   - Note: The current teardown is missing `pci_clear_master(pdev);` and `del_gendisk` is handled by `ramshared_queue_cleanup(rs_dev);`. `add_disk()` happens after `ramshared_queue_init()`, so `del_gendisk()` happens inside `ramshared_queue_cleanup()`. Wait, we should probably check if we need to remove the disk explicitly in `remove`, or if `ramshared_queue_cleanup` doing it is sufficient. In probe, `add_disk` is called *after* `ramshared_queue_init`. If we just call `ramshared_queue_cleanup`, it deletes the disk *and* frees the queue. It might be better to split them, but since we're only tasked with linearizing `pci_remove` teardown sequence with guard checks, we should ensure the order is exactly reverse of probe.

   Reverse order in `probe`:
   ```
   1. pci_enable_device_mem(pdev)
   2. pci_set_master(pdev)
   3. dma_set_mask_and_coherent
   4. pci_request_mem_regions
   5. ramshared_dma_init
   6. ramshared_queue_init
   7. add_disk
   ```
   Cleanup sequence in `remove` should be:
   ```c
static void ramshared_pci_remove(struct pci_dev *pdev)
{
	struct ramshared_device *rs_dev = pci_get_drvdata(pdev);

	if (!rs_dev)
		return;

	/* 7 & 6: del_gendisk and blk_mq_free_tag_set are inside ramshared_queue_cleanup */
	ramshared_queue_cleanup(rs_dev);

	/* 5. ramshared_dma_cleanup */
	ramshared_dma_cleanup(rs_dev);

	/* 4. pci_release_mem_regions */
	pci_release_mem_regions(pdev);

	/* 2. pci_clear_master */
	pci_clear_master(pdev);

	/* 1. pci_disable_device */
	pci_disable_device(pdev);

	dev_info(&pdev->dev, "RamShared device removed successfully\n");
}
   ```

3. FILES
   - `drivers/block/ramshared/main.c`

4. INVARIANTS
   - Clean up must be exactly in reverse order of initialization.
   - Pointer guards must be retained.

5. COUNTERFACTUAL
   - Without `pci_clear_master(pdev)`, bus-mastering could remain enabled for the disabled device, potentially causing spurious DMAs or issues upon unbinding and rebinding.

6. RED_TEST
   - Not directly testable via user space unit tests without kernel module loading/unloading, which we might not be able to easily run in this container, but we'll run `make` or similar compilation. Wait, does this repository have a Makefile for the kernel module? Let's check `ls drivers/block/ramshared/`.

7. COVERAGE
   - Static analysis tools will complain if `pci_clear_master` isn't called, and humans reviewing kernel code will notice the discrepancy.

8. REAL_PROOF
   - Ensure clean compilation. `make -C /lib/modules/$(uname -r)/build M=$PWD/drivers/block/ramshared modules` or however the project is configured.

9. ROLLBACK
   - Revert changes to `drivers/block/ramshared/main.c` if kernel panic occurs on module unload.

10. PR_BOUNDARY
   - Strictly against `jules/inbox`. Do not merge.
