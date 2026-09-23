# Production Deployment Security Hardening Checklist

This document serves as the mandatory checklist for deploying RamShared in production environments, specifically focusing on security hardening measures.

## 1. Kernel Parameters and `sysctl` Tunables

The following kernel parameters must be configured to reduce the attack surface and prevent common vulnerabilities:

*   **ASLR (Address Space Layout Randomization):**
    *   `kernel.randomize_va_space = 2` (Fully randomize the layout of the stack, VDSO page, shared memory, and the data segment)
*   **Restricted `dmesg` Access:**
    *   `kernel.dmesg_restrict = 1` (Prevent unprivileged users from viewing kernel syslog)
*   **Restricted `kptr`:**
    *   `kernel.kptr_restrict = 2` (Hide kernel pointers from all users)
*   **eBPF Restrictions:**
    *   `kernel.unprivileged_bpf_disabled = 1` (Disable unprivileged eBPF)
*   **YAMA ptrace Scope:**
    *   `kernel.yama.ptrace_scope = 1` (or `2` if strictly needed) (Restrict ptrace to parent processes only)
*   **Disable User Namespaces (if unused):**
    *   `user.max_user_namespaces = 0` (If unprivileged user namespaces are not required)

## 2. Mandatory Access Control (AppArmor / SELinux)

RamShared components MUST run confined by a Mandatory Access Control system.

*   **AppArmor:**
    *   Ensure specific AppArmor profiles are loaded for all RamShared daemon processes.
    *   Profiles must be in `enforce` mode in production.
    *   Profiles should follow the principle of least privilege, explicitly allowing only necessary file reads/writes, network sockets, and capabilities.
*   **SELinux:**
    *   Ensure SELinux is in `enforcing` mode.
    *   Apply appropriate SELinux contexts to RamShared binaries and configuration files.
    *   Use targeted policies to restrict RamShared processes.

## 3. File Permissions and Ownership

Strict file permissions are critical to prevent unauthorized modification of RamShared components.

*   **Configuration Files (`/etc/ramshared/`):**
    *   Ownership: `root:root`
    *   Permissions: `0600` (Read/Write for root only) or `0644` (if reading by non-root daemon is strictly necessary, but prefer dropping privileges after reading).
*   **Binaries and Scripts:**
    *   Ownership: `root:root`
    *   Permissions: `0755` (Executable by everyone, writable only by root). *No SUID/SGID bits unless explicitly audited and documented.*
*   **Log Files (`/var/log/ramshared/`):**
    *   Ownership: `ramshared:adm` (or similar dedicated user/group)
    *   Permissions: `0640` (Read/Write for owner, read for group, none for others).
*   **Data Directories (e.g., backing files):**
    *   Ownership: Dedicated `ramshared` user.
    *   Permissions: `0700` (Access restricted strictly to the dedicated user).

## 4. Operational Best Practices

*   **Dedicated Service Account:** Run RamShared services under a dedicated, unprivileged user account (e.g., `ramshared`), not `root`. Drop privileges immediately after initialization if root is required for setup (e.g., loading modules or binding to low ports).
*   **Capability Dropping:** Drop all unnecessary Linux capabilities. If a service doesn't need `CAP_SYS_ADMIN`, `CAP_NET_RAW`, etc., explicitly drop them (e.g., using service managers `CapabilityBoundingSet`).
*   **No Exec on Data Partitions:** Mount partitions containing RamShared backing data with the `noexec` flag.
