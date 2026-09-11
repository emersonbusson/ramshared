# Production Deployment Security Hardening Checklist

This checklist applies to RamShared deployment in production environments. It validates that the host operating system, kernel runtime, and container properties are correctly locked down.

## 1. Kernel Parameters (sysctl)
- [ ] `kernel.dmesg_restrict=1`: Restrict non-privileged access to dmesg.
- [ ] `kernel.kptr_restrict=2`: Hide kernel pointers.
- [ ] `kernel.perf_event_paranoid=3`: Restrict perf events.
- [ ] `vm.mmap_rnd_bits=32`: Maximize ASLR entropy (x86_64/ARM64).
- [ ] `kernel.unprivileged_bpf_disabled=1`: Prevent unprivileged eBPF.

## 2. Boot Parameters
- [ ] `slab_nomerge`: Prevent SLUB cache merging.
- [ ] `init_on_alloc=1`: Zero memory on allocation.
- [ ] `init_on_free=1`: Zero memory on free.
- [ ] `pti=on`: Force Page Table Isolation.

## 3. Mandatory Access Control (AppArmor/SELinux)
- [ ] Profile enforces `deny ptrace`.
- [ ] Profile blocks access to `/sys/kernel/debug/` and `/sys/kernel/tracing/`.
- [ ] Device access strictly limited to required `/dev/ramshared*` nodes.

## 4. File Permissions and Userspace
- [ ] Daemon runs as dedicated non-root user (e.g., `_ramshared`).
- [ ] `/dev/ramshared*` owned by `_ramshared` group, permissions `0660`.
- [ ] No world-writable files in the installation directory.
- [ ] Secrets (TLS keys, credentials) are `0400` owned by daemon user.
