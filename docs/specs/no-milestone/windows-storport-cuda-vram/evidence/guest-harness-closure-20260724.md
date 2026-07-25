# Windows Guest Harness Closure — 2026-07-24

## Scope

This evidence closes two harness reliability defects on the isolated
`win11-drill` surface:

- PowerShell Direct connection creation could block before the previous
  `Wait-Job` timeout owned the operation.
- Multi-record job output could be coerced to `System.Object[]`, producing a
  false `UNKNOWN` despite exact `STATUS=PASS` and `EXIT=0` records.

Connection creation now runs inside a bounded local job. Status parsing joins
records before applying the existing exact, single-marker verdict rules.

## Driver and Verifier campaign

Artifact: `C:\ramshared\artifacts\guest-exhaustive-20260724-215817`

- `IOCTL_PASS1=PASS`
- `IOCTL_VERIFIER=PASS`
- Driver Verifier flags: `0x2093B`
- `ramshared.sys`: load 1 / unload 0
- package/running SHA-256:
  `324CC7C95A17BE3C245865F55EFC3E87B443D9CF711249068A4221DD86DEDBFA`
- no new crash dump
- elevated harness exit: 0

## Product Online campaign

Artifact: `C:\ramshared\artifacts\guest-product-online-20260724-221128`

Three fresh CUDA-backed 64 MiB lifecycles passed:

- exact `RAMSHARE VRAMDISK` identity and size;
- write/read SHA match;
- console exit zero without force-kill;
- graceful lock, dismount, unregister, destroy, wipe, and lease release;
- CUDA memory restoration;
- no new dump and safe terminal state.

The campaign summary reports `PASS=true`.

## Boundary

This is supervised-beta evidence from a test-signing-enabled disposable VM.
It is not a public Windows signing or daily-host installation claim.
