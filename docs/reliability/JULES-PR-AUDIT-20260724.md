# Jules PR audit — 2026-07-24

## Scope

This audit covers every open Jules-generated pull request from `#107` through
`#142` against the RamShared day-0, SSDV3, hang-safety, and MVP transport
boundaries. A green isolated CI run is supporting evidence, not proof that a
patch preserves cross-component lifecycle invariants.

Classification:

- **ACCEPT**: correct and included in the consolidated MVP branch.
- **REWORKED**: valid concern, but the submitted implementation was incomplete,
  duplicated, or unsafe; the consolidated branch contains the owning-layer fix.
- **REJECT**: wrong threat model, flaky test, unsafe lifecycle change, generated
  junk, or unjustified MVP scope expansion.
- **DEFER**: potentially useful outside the MVP, but requires its own SPEC and
  live lifecycle evidence.

## Results

| PR | Classification | Audit result |
| --- | --- | --- |
| #107 | REWORKED | The English comment translation is included. Generated root `test*.js` files and repeated commits are rejected. |
| #108 | ACCEPT | Adds deterministic demote-status persistence coverage. |
| #109 | REWORKED | `Path::is_file()` follows links and the patch silently retries forever. The consolidated stop consumer uses `symlink_metadata`, accepts only a non-symlink regular file, requires successful removal before setting the stop flag, and records refusal diagnostics. |
| #110 | REJECT | Parallel `swapoff` during agent cleanup weakens bounded teardown ordering and adds unbounded thread creation without measured benefit. |
| #111 | ACCEPT | Adds missing `vram_outros` clamp and attribution-independent telemetry coverage. |
| #112 | ACCEPT | Adds swap argument and failure-path unit coverage without changing lifecycle. |
| #113 | REWORKED | Secure test-file creation is valid and consolidated with `create_new`; this single-purpose duplicate PR is superseded. |
| #114 | ACCEPT | Translates existing Vulkan comments to the project language. |
| #115 | REJECT | Substring matching for `..` rejects valid names and is not path-component validation. Superseded by the consolidated component parser. |
| #116 | REJECT | Linux CI missed the Windows-only use of `HostGates.target` in `verify_unique`. The cross-build fails with `E0609` after the field is removed; the patch is not dead-code cleanup. |
| #117 | ACCEPT | Removes stale completed-fix commentary from the B1 drill without changing behavior. |
| #118 | ACCEPT | Adds safe unit coverage for ublk argument and validation logic; it does not promote ublk to product transport. |
| #119 | REWORKED | Component-based containment is the right direction and is consolidated with positive nested-path and traversal-refusal tests. |
| #120 | REWORKED | Making sysfs `reset` mandatory can break an otherwise valid fallback. The consolidated path preserves fallback semantics, reports runtime-file cleanup failures, and verifies `/dev/zram0` is a block device before `mkswap`. |
| #121 | ACCEPT | PID parsing is unsigned, process identity is checked through `/proc/<pid>/comm`, and `kill` receives an option terminator. |
| #122 | REWORKED | Combines a weak substring traversal check with valid `create_new` test setup. Only the valid mechanisms are consolidated. |
| #123 | REJECT | Duplicate weak substring traversal check. |
| #124 | REWORKED | Valid component-validation intent, superseded by one canonical implementation and regression matrix. |
| #125 | REJECT | Canonicalization followed by a later command is still TOCTOU, changes the recorded device path, and contains a lossy fallback after `to_str`. |
| #126 | REJECT | Cosmetic RAII field rename plus an unrelated root `pr_desc.md`; no MVP value. |
| #127 | REJECT | Applies naive traversal checks to fixed internal `/proc` paths while failing to validate the kernel-derived cgroup component that matters. |
| #128 | REWORKED | Best of the duplicate cgroup proposals; consolidated with stricter component handling and positive-path coverage. |
| #129 | ACCEPT | Adds the conventional option terminator to `zcat`; the input path is constant, so this is defense-in-depth rather than a command-injection fix. |
| #130 | REJECT | Test-only deletion wrappers do not fix a production vulnerability and substring traversal remains incomplete. |
| #131 | REWORKED | `create_new` is valid for the forensic marker, but deleting first recreates a race. Consolidation uses exclusive creation and reports existing markers without following them. |
| #132 | REJECT | Parallel NBD disconnect changes teardown ordering without a bounded drain proof or benchmark. |
| #133 | REJECT | Duplicates a short safety-critical merge loop for an unmeasured micro-optimization and increases review surface. |
| #134 | REWORKED | Cleanup errors should be observable. Consolidation uses a runtime-file-specific helper while preserving idempotent `NotFound` behavior. |
| #135 | REWORKED | Adds useful control encoding and boundary tests. Consolidation excludes the unrelated root `fix.rs`, uses exclusive temporary-file creation, and replaces generated speculative commentary with the actual regular-file refusal contract. |
| #136 | REJECT | A small argument-allocation rewrite adds a large benchmark dependency/lockfile delta and does not justify release risk. |
| #137 | REJECT | Parallel cascade `swapoff` violates the sequential swapoff-first safety frontier and has no exhaustion/hang evidence. |
| #138 | REJECT | Complex test cleanup wrappers silently skip missing paths and do not address production behavior. |
| #139 | DEFER | `OwnedFd` ownership can be a sound cleanup, but ublk is outside the MVP transport and the change needs dedicated FD lifecycle, abort, and crash/drain evidence. |
| #140 | REJECT | The descriptor tests are useful, but the fake-file io_uring test assumes a kernel-specific immediate `-EOPNOTSUPP` completion and is flaky across kernels. Rework behind an injected ring boundary before inclusion. |
| #141 | REWORKED | Duplicates the forensic-marker concern; superseded by the exclusive-create implementation. |
| #142 | ACCEPT | Refuses `mkswap` unless `/dev/zram0` resolves to a block device. |

## Consolidated invariants

The MVP branch applies one solution per owning layer:

1. cgroup paths are assembled only from root and normal components; parent,
   current-directory, and prefix components fail closed;
2. stop requests are consumed only after a non-link regular file is removed;
3. forensic markers use exclusive creation and are never overwritten through a
   link;
4. runtime cleanup treats absence as idempotent and reports every other error;
5. zram fallback verifies the block-device type before destructive formatting;
6. PID-directed termination verifies the expected daemon identity;
7. swapoff and NBD disconnect remain sequential;
8. NBD remains the MVP transport and ublk ownership refactors remain deferred.

## Closure rule

The individual PRs must not be merged after the consolidated MVP PR lands.
They are superseded by this audit and should be closed with a link to the
consolidated PR. No rejected or deferred change is release evidence.
