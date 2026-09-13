# RamShared threat model

## Scope and decision boundary

This model covers the repository's engineering records, deterministic checkers,
build inputs, and supervised laboratory evidence. It does not assert that a
local checkout, a WSL2 instance, a Windows guest, a GPU, or a kernel driver is
safe to operate. Those are separate execution boundaries, and an
environment-bound result remains `PARTIAL` until its own evidence is current.

The model is intentionally operationally inert: it authorizes no pressure,
swap, driver, VM, service, reboot, disk-reclaim, network, or privilege action.

## Assets and security objectives

| Asset | Objective | Failure if lost |
| --- | --- | --- |
| Source and reviewed configuration | Integrity and provenance | A build or lab action differs from the reviewed state. |
| ADRs, SSDV3 records, task log, and claims | Traceability | A decision or delivery state has no accountable, time-bounded basis. |
| Campaign and benchmark evidence | Custody, bounded retention, and reproducibility | A claimed observation cannot be tied to an exact run and input state. |
| Sanitized logs and artifacts | Confidentiality and safe sharing | Credentials, private paths, or kernel-sensitive data leave the intended boundary. |
| Host and guest safety controls | Fail-closed authority | A documentation or test action is mistaken for permission to operate privileged hardware paths. |

## Trust boundaries

1. A working tree is an input, not evidence. Uncommitted state is recorded as
   such and cannot silently become a release or campaign identity.
2. Pure Node and shell checkers are read-only validators. They must refuse
   symlinks, unsafe relative paths, malformed manifests, and unbounded
   artifacts instead of following or repairing them.
3. WSL2, Windows guests, drivers, services, GPUs, and kernel paths are
   privileged execution domains. A local documentation gate never crosses
   into them; their existing safety harnesses and explicit approvals remain
   authoritative.
4. Repository history preserves prior records but does not retroactively
   qualify them. Historical evidence is labeled observed or unqualified when
   custody, owner, timestamps, source revision, or retention facts are absent.

## Broker-Daemon IPC Threats (Unix Sockets & Named Pipes)

The broker-daemon communication channel is a high-value target because it bridges user-space coordination with privileged hardware enforcement.

| Threat | Channel | Required control | Honest residual risk |
| --- | --- | --- | --- |
| **Message Replay** | Unix Sockets, Named Pipes | Nonces, strictly monotonic sequence numbers, or timestamped signatures on state-mutating requests | High-frequency replay might exhaust broker queue limits before detection. |
| **Spoofed Client Injection** | Named Pipes (Windows), Unix Sockets (Linux) | Strict ACLs (Windows `LocalSystem`/`Administrators` only, Linux `root:root` 0600), `SO_PEERCRED` validation, mutual authentication | A compromised administrator account already owns the machine. |
| **Malformed Payload DoS** | Unix Sockets, Named Pipes | Strict bounds-checked parsing (no `unwrap()`), maximum message size limits, timeout on partial reads | Extremely fast well-formed but semantically complex messages might still induce high CPU usage. |
| **Downgrade Attack** | Protocol handshake | Minimum protocol version enforcement, fail-closed on unknown opcodes | A legitimate but old client is permanently locked out without upgrade. |
| **File Descriptor / Handle Exhaustion** | Unix Sockets, Named Pipes | Connection limits per peer, overall broker connection ceiling, eager reaping of idle sessions | The broker may briefly refuse legitimate new daemons during a storm. |

## Threats and controls

| Threat | Boundary crossed | Required control | Honest residual risk |
| --- | --- | --- | --- |
| Edited evidence is presented as an observed campaign | Working tree → decision record | Versioned manifest, source revision, artifact inventory/hash, owner role, observed and verified times | A valid hash cannot prove the physical experiment occurred. |
| A new record bypasses review through an orphan file or duplicate ADR number | Filesystem → architectural history | Canonical ADR registry, explicit historical collision record, read-only index checker | Legacy document shape remains preserved rather than rewritten. |
| A sensitive value or host-specific detail enters a report | Lab output → tracked documentation | Sanitized evidence, provenance checks, no secrets/private paths/kernel addresses in tracked records | New detection rules can miss an unknown encoding; reviewers remain required. |
| A symlink or traversal path expands the artifact set | Manifest → filesystem | `lstat`-based refusal, relative-path allowlist, artifact count and byte limits | Existing historical directories are not automatically repaired. |
| A stale environmental result is treated as current capability | Time → capability claim | Explicit lifecycle, freshness/review fields, `PARTIAL` for blocked or stale proof | Live hardware availability still limits revalidation. |
| A cleanup note is interpreted as permission for host mutation | Documentation → privileged host | Read-only runbook/checkers, explicit operator authority, no automatic reclaim action | An authorized operator can still make a human error; command-level safeguards remain necessary. |
| A checker passes while its own policy has drifted | Policy → CI outcome | Deterministic fixtures, negative tests, reviewable policy files, no network dependency | A checker cannot independently establish the truth of a physical measurement. |

## Required handling rules

- New governed records carry an accountable owner role, a precise source
  revision or explicit dirty-state marker, and separate observed versus
  verified timestamps when both exist.
- Evidence lifecycle is explicit: `writing`, `complete`, `failed`, or
  `blocked`. Only `complete` evidence can support a promotion, and only when
  its consumer's gate is otherwise satisfied.
- Retention is declared before a new campaign artifact is accepted. A
  temporary artifact is either removed by its owning runner or refused as
  residue; a historical artifact is not silently deleted by a checker.
- All automated safety frontiers fail closed. A malformed input produces
  `NO-GO` or an explicit checker error, never a best-effort repair.
- Capability, decision, and evidence records state their limitation. Passing
  a repository check is not a claim that a driver, VM, GPU, or kernel path ran.

## Review and escalation

Reviewers should ask whether the claimed fact matches its owner, source
revision, time, environment, and retained artifact set. If any answer is
unknown, downgrade the claim to observed, historical, blocked, or `PARTIAL`;
do not manufacture missing proof.

A finding that could cause host mutation, unsafe pressure, driver installation,
storage teardown, or data loss is escalated to the owning safety runbook and
requires explicit operator authority. This document does not provide that
authority.

## Non-goals

- It is not a substitute for kernel, Windows driver, WSL2, or hardware threat
  analysis attached to a concrete SSDV3 slice.
- It does not create a generic security service, telemetry pipeline, or remote
  control plane.
- It does not turn lab evidence into a production-security certification.
