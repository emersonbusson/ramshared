# SPEC — Public repository candidate integrity

> SSDV3 Step 2 · implements [`PRD.md`](PRD.md).

## Closed scope

This slice changes only the repository hygiene checker, its tests/CI wiring,
and public script defaults containing developer-specific paths. It does not
execute any changed operator script and cannot mutate Windows, WSL, a VM,
driver state, storage, memory pressure, or boot state.

## Decisions

| ID | Decision | Reason |
| --- | --- | --- |
| DT-1 | `--candidate` enumerates `git ls-files -co --exclude-standard -z` and reads the working tree. | It represents the next local public candidate, including new files. |
| DT-2 | `--staged` enumerates `git ls-files --cached -z` and reads each `:<path>` index blob with `git show`. | Staged truth cannot be hidden by an unstaged clean copy. |
| DT-3 | `--tracked` is retained for a clean checkout and reads tracked working-tree files. The default is `--candidate`. | Local safety should be strongest while CI remains simple. |
| DT-4 | Limits are 20,000 paths and 512 KiB per text file. Limit, decode, Git, and usage failures return exit 2. | Silent truncation is a false green. |
| DT-5 | Binary detection rejects NUL-containing buffers. Extensionless files are scanned when they are known repository control files or start with a text shebang. | Coverage without interpreting binary artifacts as text. |
| DT-6 | Rules report only `{path,line,rule,reason}`. Fixtures build sensitive strings from fragments so the gate tests itself without committing those values verbatim. | The scanner and its tests must not become exposures. |
| DT-7 | Vendor/project contact e-mail is allowed only through an explicit repository-relative allowlist entry with owner, reason, and expiry; private profile paths and raw addresses cannot be allowlisted. | Public maintainership is legitimate, personal workstation provenance is not. |
| DT-8 | Portable Windows roots are `C:\ramshared\src` and `C:\ramshared\artifacts`; user caches derive from `$env:USERPROFILE`; Linux drill output defaults under `${TMPDIR:-/tmp}` and binaries resolve from repo/PATH. | Defaults remain usable without one developer's account layout. |

## Interface

```text
node tools/ci/check-public-hygiene.mjs [--candidate|--tracked|--staged]
```

Output is sorted and ends in either `PUBLIC_HYGIENE_STATUS=PASS` or
`PUBLIC_HYGIENE_STATUS=NO-GO`. Clean output includes `MODE` and `FILES`.

## Files and test matrix

| Path | Change | Named tests / validation |
| --- | --- | --- |
| `tools/ci/check-public-hygiene.mjs` | Export bounded enumeration, text detection, scanning, and CLI; implement the three content modes. | all tests below; per-file Node cover ≥80% |
| `tools/ci/check-public-hygiene.test.mjs` | Temporary Git repositories and pure fixtures. | `candidate_scans_nonignored_untracked_file`; `staged_reads_index_blob_not_worktree`; `ignored_file_is_not_a_candidate`; `extensionless_shebang_is_scanned`; `binary_blob_is_skipped`; `rejects_private_profile_email_token_key_and_kernel_address`; `diagnostic_never_contains_sensitive_match`; `invalid_mode_and_git_failure_exit_two`; `identical_candidate_runs_are_deterministic`; `full_repository_candidate_is_clean` |
| `docs/governance/public-hygiene-allowlist.json` | Strict, empty-by-default contact allowlist with `{pattern,scope,owner_role,reason,expires}` entries. | `contact_allowlist_is_scoped_owned_and_expiring`; private-path/secret/address rules remain non-allowlistable |
| `scripts/docs-check.sh` | Invoke `--candidate`. | `docs_check_uses_candidate_public_hygiene` plus live docs-check |
| `.github/workflows/ci.yml` | Add the exact public-hygiene test/coverage gate. | workflow static assertion and CI-equivalent command |
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
5. Run candidate and staged before→action→after fixtures, including one
   legitimate clean candidate and at least two refusals.
6. Run `scripts/docs-check.sh` twice and compare normalized output hashes.
7. Append validation evidence and create `IMPL.md`; no commit is automatic.

## Platform gates

`BINARY_MATCH`, WDK, SCM, VM, physical boot, GPU, storage, and pressure gates
are `N/A — repository-only read-only tooling`. PowerShell syntax/static tests
are applicable to the changed scripts; live execution is deliberately out of
scope because only their path defaults change.

## Rollback

Revert the checker/wiring and portable-default changes together if one staged
blob is read from the worktree, one non-ignored candidate is skipped, one
sensitive match is printed, or candidate mode exceeds 10 seconds on the
current repository in three consecutive no-load runs.
