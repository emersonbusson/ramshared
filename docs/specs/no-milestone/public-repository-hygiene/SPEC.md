# SPEC — Public repository candidate integrity

> SSDV3 Step 2 · implements [`PRD.md`](PRD.md).

## Closed scope

This slice changes only the repository hygiene checker, its public-binary
digest contract, the Rust coverage planner's global ownership validation,
their tests/CI wiring, and public script defaults containing
developer-specific paths. It does not
execute any changed operator script and cannot mutate Windows, WSL, a VM,
driver state, storage, memory pressure, or boot state.

## Decisions

| ID | Decision | Reason |
| --- | --- | --- |
| DT-1 | At run start the checker resolves symbolic `HEAD` exactly once with `rev-parse --verify HEAD^{commit}` and thereafter uses only that immutable commit/tree OID. `--candidate` enumerates the captured tracked set plus non-ignored untracked paths and reads working-tree entries without following the final symlink. With dirty state it selects the commit-OID-to-worktree delta plus untracked files. In a clean checkout it selects the immutable commit against its first parent; a root commit uses Git's empty-tree object. | Local checks represent the next worktree publication, clean CI proves newly committed content, and a concurrent symbolic-HEAD move cannot change authority mid-run. First-parent merge semantics match the captured merge candidate and avoid rescanning unrelated history. |
| DT-2 | One bounded `git ls-files --stage -z` result captures every index path, mode, and blob OID. `--staged` derives its changed set by comparing those immutable entries with the captured HEAD tree and reads blobs with `git cat-file blob <oid>`; it never reselects through a live `:<path>`. A byte-exact final index query must match the snapshot or the run refuses with `git-index-snapshot-changed`. | Staged truth cannot be hidden by an unstaged copy or changed through an index TOCTOU; a concurrent index move either cannot affect selected blobs or fails closed. |
| DT-3 | `--tracked` is retained for a clean checkout and reads tracked working-tree files. The default is `--candidate`; `--check` maps exactly to candidate mode. | Local safety should be strongest while CI and existing callers retain a safe compatibility spelling. |
| DT-4 | Limits are 20,000 paths, 512 KiB per text file, 8 MiB per declared public binary, 64 MiB per decoded PNG, 64 KiB per decoded PNG text field, 256 KiB total decoded PNG text, 64 million JPEG pixels, 2,097,152 JPEG coefficient blocks, and 2 MiB per planner trust input. Limit, decode, Git, and usage failures fail closed. | Silent truncation and parser resource exhaustion are false greens. |
| DT-5 | Every changed public artifact is strict UTF-8 text unless `.png`, `.jpg`, or `.jpeg` bytes satisfy the explicit bounded contract. PNG requires exact signature; legal non-interlaced IHDR depth/color/method fields; first/single IHDR, legal PLTE, consecutive IDAT, and terminal/single IEND ordering; bounded chunk types/length/CRC; all IDAT bytes concatenated in order into exactly one fully consumed RFC 1950 zlib stream; exact packed scanline length; and filter bytes 0–4. Adam7 and a second concatenated zlib stream are refused. The ancillary allowlist is `cHRM`, color-compatible `bKGD`, and parsed `tEXt`/`zTXt`/`iTXt`; indexed `bKGD` must be less than the PLTE entry count, iTXt language bytes must be exact ASCII `[A-Za-z0-9-]*` before decoding, and textual fields have exact separators/encodings/compression methods, bounded inflation and aggregate size, and the same private-marker scan as public text. Every other ancillary type refuses. JPEG is permitted only for a sorted, unique, NFC/case-collision-free canonical path, size, and SHA-256 in the strict reviewed manifest/schema, and only when the captured index says the path is a regular tracked file. Candidate and staged modes bind manifest, schema, JPEG path set, and JPEG bytes to the immutable committed authority OID (captured commit for dirty/staged, first parent for clean candidate), so changing an asset and manifest together cannot redefine the baseline. JPEG must be standalone single-scan baseline/JFIF with bounded DQT/DHT, 8-bit SOF0 dimensions/components/sampling, matching SOS tables/components, decodable Huffman DC/AC blocks, valid stuffing/restart cadence, padding, and exact terminal EOI. APPn/COM is refused unless it is the first exact JFIF APP0 or one exact C2PA APP11 profile: `JP` header/sequence, size-bounded JUMBF boxes, exact `c2pa`→claim-store→signature/claim/assertions→hash/actions tree, content-type UUID tails, labels, and nonempty CBOR leaves. Latin-1 plus both alignments of bounded UTF-16LE/BE metadata are privacy-scanned; only raw UUIDs within a structurally valid content-credential label/CBOR field are exempt. Malformed/unknown APP11 and every other unsupported APPn/COM profile refuse, while private content wins the stable sensitive reason. Progressive, arithmetic, external-table, abbreviated, unlisted, changed-digest, mutable-manifest, duplicate-path/digest, or unsupported-metadata JPEGs refuse. SVG, TXT, JSONL, YAML, logs, and unknown extensions remain text. Public text rejects every Cc control except normalized CR/LF/tab and every Cf/bidi format character; `TextDecoder(ignoreBOM: true)` preserves leading U+FEFF for refusal. Diagnostics encode code points as ASCII `u+XXXX`. | Invalid UTF-8, a BOM, extension confusion, compressed/UTF-16 private metadata, decompression bombs, arbitrary entropy, broad C2PA substring exemptions, or bytes merely bookended by image magic cannot become a bypass, while reviewed image assets remain deterministic and bounded without a dependency. |
| DT-6 | Rules report only `{path,line,rule,reason}`. Fixtures build sensitive strings from fragments so the gate tests itself without committing those values verbatim. | The scanner and its tests must not become exposures. |
| DT-7 | Vendor/project contact e-mail is allowed only through an explicit repository-relative allowlist entry with owner, reason, and expiry; private profile paths and raw addresses cannot be allowlisted. | Public maintainership is legitimate, personal workstation provenance is not. |
| DT-8 | Portable Windows roots are `SANITIZED_PORTABLE_WINDOWS_SOURCE_ROOT` and `SANITIZED_PORTABLE_WINDOWS_ARTIFACT_ROOT`; user caches derive from `$env:USERPROFILE`; Linux drill output defaults under `${TMPDIR:-/tmp}` and binaries resolve from repo/PATH. | Defaults remain usable without one developer's account layout. |
| DT-9 | Candidate and tracked symlinks use `lstat` plus `readlink`; staged symlinks use the index blob and mode `120000`. The final target is never opened. Absolute or lexically repository-escaping public targets report `PUBLIC_SYMLINK_ESCAPE`. | The bytes Git publishes are the link text, not the current target content. This also prevents a repository-internal link from becoming a read primitive for a private target. |
| DT-10 | Strict UTF-8, Cc/Cf, binary structure, and symlink checks cover the whole changed artifact. Identity and activation findings are limited to added lines for an existing artifact; untracked and root-commit artifacts use all lines. Rename detection is disabled so unsafe source and target names are independently visible, while deletions are read-free. | A historical identity baseline must not hide new content, and line scoping must never weaken whole-artifact encoding or structural safety. |
| DT-11 | The docs workflow runs `node tools/ci/check-public-hygiene.mjs --candidate` as a direct named step before the aggregate docs script. The CI contract requires that exact reachable command. | The committed-content proof remains explicit even if the aggregate script is later refactored. |
| DT-12 | Workflow-dispatch recovery for the historical beta preserves the current manifest writer, integrity checker, artifact helper, and SBOM merger before checking out the historical tag. Push-tag execution keeps using the checked-out source tools. Both paths remain read-only and nonpublishing. | The historical writer cannot parse the current immutable Rust provenance field; preserving reviewed current parsers closes version skew without changing the exact tag/SHA or adding publication authority. |
| DT-13 | `validateCoverageMap` performs a global ownership pass before diff selection. Every production source across all active `rust-line-coverage` entries has zero or one pure line owner. A source referenced by `rust-ignored-test-relocation` may additionally have exactly one proof owner; any other special owner, duplicate integration source/target, or duplicate integration source/target/test-name refuses. Static CLI `--all` cannot print READY for an invalid global map. | Diff-local or relocation-only validation cannot expose a latent duplicate pure owner elsewhere in the map. |
| DT-14 | Planner paths are decoded with fatal BOM-preserving UTF-8. No record is trimmed into another path. Map/changed-list files and every referenced SPEC, production source, integration test, Windows static/live test source, and wrapper must resolve to a bounded regular non-symlink file at the same canonical repository path. BOM, Cc, Cf, backslash, absolute, drive, dot-segment, non-NFC, leading option, and repository-escape paths refuse before reads or selection. | A symlinked configuration/source or normalized control-bearing record must not redirect policy evidence or turn an unsafe name into an authorized source. |

## Interface

```text
node tools/ci/check-public-hygiene.mjs [--candidate|--tracked|--staged|--check]
```

Output is sorted and ends in either `PUBLIC_HYGIENE_STATUS=PASS` or
`PUBLIC_HYGIENE_STATUS=NO-GO`. Clean output includes `MODE` and `FILES`.

## Files and test matrix

| Path | Change | Named tests / validation |
| --- | --- | --- |
| `tools/ci/check-public-hygiene.mjs` | Export bounded enumeration, extension-independent strict public-text checks, decoder-level PNG validation, manifest-bound standalone baseline JPEG validation, filename identity scanning, text detection, and CLI; implement the three canonical modes plus the safe alias. | all tests below; per-file Node cover ≥80% |
| `tools/ci/check-public-hygiene.test.mjs` | Temporary Git repositories and pure fixtures. | Existing candidate/topology/text/image tests plus `candidate_binds_one_head_oid_when_symbolic_head_moves_mid_run`; `staged_index_snapshot_refuses_mid_run_index_move`; `png_two_zlib_stream_fixture_is_explicit_and_refused`; `png_indexed_background_requires_existing_palette_entry`; `png_itxt_language_requires_exact_raw_ascii_bytes`; `jpeg_metadata_profiles_refuse_utf16_and_malformed_c2pa_app11`; `repository_candidate_is_clean` |
| `docs/governance/public-binary-digests.json`, `docs/governance/public-binary-digests.schema.json` | Exact reviewed JPEG path/size/SHA registry and strict schema. | whole-file UTF-8/JSON, schema, canonical path, sort, duplicate path/digest, tracked regular-file, current-mode digest, symlink, and asset-structure refusal fixtures |
| `tools/ci/plan-rust-slice-coverage.mjs` | Validate all pure line/relocation ownership globally and confine every trust input before static or selected planning. | per-file Node cover ≥80%; invalid current map `--all` must be BLOCKED |
| `tools/ci/plan-rust-slice-coverage.test.mjs` | Hermetic ownership, trust-path, and raw changed-path fixtures plus direct CLI output capture. | `global_line_coverage_ownership_refuses_duplicate_pure_owners_including_cli_all`; `planner_trust_inputs_require_confined_non_symlink_regular_files`; `planner_cli_rejects_bom_or_control_changed_paths_without_trimming`; existing relocation-owner cases |
| `docs/governance/public-hygiene-allowlist.json` | Strict, empty-by-default contact allowlist with `{pattern,scope,owner_role,reason,expires}` entries. | `contact_allowlist_is_scoped_owned_and_expiring`; private-path/secret/address rules remain non-allowlistable |
| `scripts/docs-check.sh` | Invoke `--candidate`. | `docs_check_uses_candidate_public_hygiene` plus live docs-check |
| `.github/workflows/ci.yml` | Add the exact committed-candidate step plus public-hygiene test/coverage gate. | `clean_checkout_ci_enforces_committed_candidate_public_hygiene`; CI-equivalent command |
| `.github/workflows/release-integrity.yml` | Preserve current parser-compatible recovery helpers before the historical checkout. | `release_integrity_recovery_is_exact_tag_sha_read_only`; `release_integrity_recovery_current_parser_accepts_v0_9_0_beta_1_without_publication` |
| `docs/governance/redaction-ledger.json`, `validation.md` | Preserve the historical validation prefix byte-for-byte and bind append-only sanitization corrections by digest. | validation schema `--diff HEAD` and `--all`; ledger digest recomputation |
| `scripts/windows/Install-WinDriveVm.ps1` | Portable source/certificate/log roots. | static no-private-path scan |
| `scripts/windows/build-winsvc.bat` | Portable source root/environment override. | static no-private-path scan |
| `scripts/windows/Build-Drivers.ps1` | Portable repository root. | static no-private-path scan |
| `scripts/windows/Sign-Drivers.ps1` | Portable repository/certificate roots. | static no-private-path scan |
| `scripts/windows/Run-StorportCudaPartial.ps1` | Portable artifact root parameter. | static no-private-path scan |
| `scripts/windows/Invoke-Guest.ps1` | Portable example path. | static no-private-path scan |
| `scripts/windows/Install-InfAndBackend.ps1` | Portable repository root. | static no-private-path scan |
| `scripts/windows/Invoke-SharedWslPressureCampaign.ps1` | Derive WSL repository from an explicit parameter or current directory. | static no-private-path scan |
| `scripts/windows/Measure-CDrivePressure.ps1` | Derive profile paths from `$env:USERPROFILE`. | static no-private-path scan |
| `scripts/windows/Invoke-DisciplinedCampaign.ps1` | Portable source/results defaults. | static no-private-path scan |
| `scripts/p0/measure-cascade-demote.sh` | Repo/PATH binary lookup and temporary artifact default. | `bash -n`; static no-private-path scan |
| existing Node test fixtures | Build sensitive examples from fragments. | candidate gate clean |

## Ritual and validation

1. Add the named failing tests.
2. Implement the checker until unit/refusal tests pass.
3. Apply portable defaults, then run `bash -n` and the existing Windows static
   PowerShell harnesses without executing operational scripts.
4. Run the per-file Node coverage command with 80% lines/branches/functions.
5. Run candidate, staged, and tracked before→action→after fixtures, including
   dirty and clean commits, root/detached/merge topology, rename/deletion and
   mixed index/worktree state; invalid UTF-8, U+202E, C0, and BOM refusals for
   TXT and SVG; internal/external symlink blobs; valid and malformed PNG zlib,
   two-stream zlib, scanline, palette/background, raw iTXt language, ordering,
   and bomb fixtures; reviewed and malformed JPEG
   table/frame/scan/entropy/restart/digest/schema/UTF-16/JUMBF fixtures; one
   symbolic-HEAD-move fixture; one index-move refusal; and at least two
   identity refusals.
6. Run direct and CLI `--all` ownership fixtures, including duplicate pure line
   owners, one legal line owner plus one proof, duplicate relocation owners,
   external symlink inputs, and raw BOM/Cc/Cf changed paths.
7. Run `scripts/docs-check.sh` twice and compare normalized output hashes.
8. Append validation evidence and create `IMPL.md`; no commit is automatic.

## Platform gates

`BINARY_MATCH`, WDK, SCM, VM, physical boot, GPU, storage, and pressure gates
are `N/A — repository-only read-only tooling`. PowerShell syntax/static tests
are applicable to the changed scripts; live execution is deliberately out of
scope because only their path defaults change.

## Rollback

Revert the checker/wiring and portable-default changes together if one staged
blob is read from the worktree, one non-ignored or newly committed candidate is
skipped, one symbolic HEAD or moved index changes authority without refusal,
one topology error reports an empty success, one public text extension,
BOM, or invalid UTF-8 sequence bypasses scanning, one image is accepted without
its size/signature/decoded-structure contract, one JPEG bypasses the exact
tracked immutable digest manifest or strict metadata profile, one unsupported
or sensitive ancillary payload passes, one duplicate pure line or relocation
owner reaches READY, one planner input symlink is followed, one changed path is
trimmed before validation, one symlink target is followed, one sensitive match
is printed, or candidate mode exceeds 10 seconds on the current repository in
three consecutive no-load runs.
