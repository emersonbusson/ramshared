# AUDIT — Public repository candidate integrity R5

## Findings

| Sev | SPEC § | Issue | Resolution |
| --- | --- | --- | --- |
| High | DT-1, DT-2 | One run repeatedly reused symbolic `HEAD`, live `:<path>` index resolution, and live staged selection. A concurrent commit or index rewrite could therefore change authority or bytes after selection. | The checker resolves HEAD once to a commit OID, captures the complete bounded stage-0 index path/mode/blob map, derives staged changes against the immutable captured tree, reads captured blob OIDs, and byte-compares the index at exit. Deterministic HEAD-move and index-move fixtures prove stable authority and fail-closed inconsistency. |
| High | DT-1, DT-11 | A clean checkout has no worktree diff, so local-only candidate semantics could report zero files and never inspect a newly committed unsafe artifact. | Clean candidate compares HEAD with its first parent, uses the empty tree for a root commit, and is invoked directly by the clean CI job. Root, detached, merge, rename, deletion, mixed-state, and topology-error fixtures are hermetic. |
| High | DT-5 | Default `TextDecoder` BOM handling could consume a leading U+FEFF in content or the first NUL-delimited Git path. | The fatal decoder preserves BOM code points; leading/interior content BOMs and a BOM-prefixed Git path refuse. |
| High | DT-9 | Reading a candidate path with ordinary file APIs could follow a public symlink into a repository-internal or external target. | Candidate/tracked use `lstat` and `readlink`; staged uses the index blob and mode. The final target is never opened, and an escaping public link refuses. |
| High | DT-5 | The earlier PNG claim stopped at chunk CRC/order: a CRC-valid IDAT containing raw private-path bytes passed without zlib inflation, exact scanline length, or filter validation. Large declared dimensions could also turn decompression into an unbounded resource request, and the evidence did not contain a real two-zlib-stream fixture. | All consecutive IDAT chunks are concatenated, inflated as one fully consumed zlib stream with `maxOutputLength = expected + 1`, and checked against BigInt-derived bounded non-interlaced scanline geometry and filters 0–4. A fixture concatenates two actual `deflateSync` outputs and proves `public-png-zlib-invalid`; illegal IHDR/PLTE/order, trailing zlib data, short/long output, and bombs have stable refusal reasons. |
| High | DT-5 | CRC-valid ancillary PNG chunks were accepted without interpreting their payload. Later parsing still decoded iTXt language bytes with high-bit-masking ASCII semantics and only required an indexed `bKGD` to follow some PLTE, not to reference an existing entry. | Only the canonical structural ancillary set plus strictly parsed textual chunks is accepted. Raw iTXt language bytes are checked byte-by-byte against ASCII alphanumeric/hyphen, indexed background is strictly below the PLTE entry count, text compression/separators/encodings/output are bounded, all decoded fields are privacy-scanned, and unknown ancillary types refuse. |
| High | DT-5 | The earlier JPEG marker walk accepted abbreviated files, missing DQT/DHT, unsupported frames, mismatched SOS components, and arbitrary entropy as long as a frame, scan marker, and terminal EOI existed. | Only exact reviewed tracked JPEG paths/sizes/SHA-256 from the strict manifest are eligible. A dependency-free parser additionally requires JFIF, 8-bit SOF0, bounded legal DQT/DHT, matching one-scan SOS, decodable baseline DC/AC Huffman blocks, stuffing/restart cadence, all-one padding bits, and terminal EOI. Progressive, arithmetic, external-table, unlisted, changed, duplicate, symlinked, or malformed assets refuse. |
| High | DT-5 | A candidate could change tracked JPEG bytes and rewrite the digest manifest in the same diff, turning mutable candidate data into its own authority. APPn/COM scanning missed UTF-16LE/BE private values, and any APP11 containing `JP` plus the substring `c2pa` received a whole-segment raw-UUID exemption without validating JUMBF. | Candidate/staged authority is bound to an immutable committed OID and clean candidate authority to its first parent. JPEG path set, bytes, manifest, and schema cannot be redefined by the candidate. Non-JFIF metadata is refused except one exact size-bounded C2PA JUMBF tree. Latin-1 and both UTF-16 byte alignments are scanned; only raw UUIDs inside structurally validated credential labels/CBOR leaves are exempt. Malformed C2PA and all unsupported APPn/COM profiles refuse. |
| High | DT-13 | Relocation ambiguity was checked globally, but two ordinary `rust-line-coverage` entries could own the same production source when no relocation referenced it. | Every active pure line owner participates in one global source map. More than one owner emits `line-coverage-production-owner-duplicate` and blocks direct selection and CLI `--all`. |
| High | DT-14 | Map/SPEC/source/test inputs could be symlinks to external files, making external bytes satisfy repository policy. | Every loaded planner trust input is bounded and must be a regular, non-symlink file whose canonical path equals its lexical path inside the canonical repository root. External final or ancestor symlinks refuse. |
| High | DT-14 | The changed-path loader trimmed each record, so a leading BOM or whitespace control could be normalized into a valid production path. | Fatal BOM-preserving UTF-8 decoding keeps exact records; only a terminal empty line and CR belonging to CRLF are removed. BOM, Cc, and Cf characters anywhere produce `changed-path-unsafe`. |
| High | DT-10 | Added-line scoping could accidentally suppress invalid UTF-8, Cc/Cf, or malformed image content elsewhere in the same changed artifact. | Encoding, Unicode controls, image structure, and symlink rules inspect the whole changed artifact. Only identity and activation rules use added-line scoping. |
| High | DT-12 | Workflow-dispatch recovery checks out the historical beta after current source, causing its older manifest writer to reject the current immutable Rust provenance argument. | Current reviewed writer/checker/artifact helper/SBOM merger are preserved before the historical checkout and selected only for dispatch recovery. Exact tag/SHA, read-only permissions, and nonpublication remain unchanged. |
| High | evidence governance | In-place sanitization of `validation.md` violated the append-only schema, while retaining raw values in a new correction would repeat the exposure. | Restore the 3,869-line historical prefix byte-for-byte, keep legitimate later facts as appended entries, and bind the sanitized correction by SHA-256 in `docs/governance/redaction-ledger.json` without copying a private value. |

## Historical open questions (observed 2026-08-24; superseded below)

1. The strict canonical `--all` run now finds duplicate pure owners for
   `crates/ramshared-cli/src/cascade/lifecycle.rs`,
   `crates/ramshared-cli/src/main.rs`,
   `crates/ramshared-winsvc/src/config.rs`,
   `crates/ramshared-winsvc/src/evidence.rs`, and
   `crates/ramshared-winsvc/src/runtime.rs`. The shared coverage map is outside
   this dispatch and remains unchanged for a separately owned reconciliation.
2. The full planner test still requires the out-of-scope Rust named test
   `daemon_worker_shutdown_drains_queued_io_before_stop` in
   `crates/ramshared-wsl2d/src/main.rs`. The source owner must implement or
   reconcile that exact contract; the assertion remains intact.
3. No hosted workflow was dispatched. The source contract and hermetic
   clean-commit fixtures are green; hosted same-revision status remains an
   external observation.

## Historical verdict (2026-08-24)

**GO for the current immutable-Git and binary-parser remediations and the five
earlier owned validator remediations; the prior canonical planner residuals
were not revalidated in this narrowly scoped dispatch.** The repository
claim remains `PARTIAL` until the externally owned residuals are closed. No
host, WSL, VM, device, storage, swap, GPU, driver, service, publication, commit,
or remote-write action is authorized by this verdict.

## Current reconciliation — 2026-09-27

The residual list above records the 2026-08-24 checkout and is not the current
state. Revalidation on the current worktree found:

- `node tools/ci/plan-rust-slice-coverage.mjs --all` exits 0 with
  `RUST_SLICE_COVERAGE_STATUS=READY`; the five historical duplicate owners are
  absent from the current map.
- The documentation inventory, capability observations, campaign evidence
  lifecycle, docs index, and SPEC evidence manifest checks all pass in check
  mode.
- The former required name
  `daemon_worker_shutdown_drains_queued_io_before_stop` is obsolete. The
  current memory-broker DT-50 contract makes the terminal flag win at an
  iteration boundary, preempting queued I/O. Its current named proof is
  `daemon_worker_shutdown_preempts_queued_io_at_iteration_boundary`, with
  `daemon_worker_terminal_flag_wins_over_512_continuous_queue_refills` proving
  shutdown is not starved by refills. An operation already executing may finish
  at its bounded completion barrier. The source and SPEC agree; restoring a
  drain guarantee would contradict DT-50.
- Current public hygiene candidate and Node named-test/per-file gates pass; the
  exact measurements are recorded in `evidence/validation-summary.json` and
  `evidence-manifest.json`.

**Current verdict:** the scoped repository-hygiene and planner checks are green.
The final aggregate `docs-check` passed after the concurrent release
identity/evidence edits stabilized; the RamShared CLI test suite passed 355
unit and 10 integration tests, the `ramshared-wsl2d` test suite passed with
its hardware/root/device-specific tests intentionally ignored, and the three
changed shell scripts passed syntax checks. The localization gate has no hash findings; it remains
`PARTIAL` because the informational translation has no current human review
receipt. Keep this slice `PARTIAL` until hosted required CI reports success
for the final committed source revision; no hosted workflow was dispatched.

## Re-audit trigger

Re-open this audit if HEAD/index snapshot semantics change, candidate topology changes, a binary type, reviewed JPEG,
or dependency is added, PNG decoded/ancillary limits change, the JPEG committed
authority, exact JUMBF profile, or metadata policy is weakened, ownership becomes diff-local again,
a planner trust input or public-artifact symlink target is followed, changed
paths are normalized before validation, or strict whole-artifact checks become
line-scoped.
