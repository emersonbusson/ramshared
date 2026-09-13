## Summary
Replaced generic `throw "..."` statements in `scripts/windows/Invoke-WindowsStorageMatrix.ps1` with semantic errors using `Write-Error -ErrorId "StorageMatrixFailure"`, while still carrying the original context/message and terminating the script.

## Issue
Fixes #005 - semantic error returns with Write-Error -ErrorId on storage test failures.

## Commits
- feat(scripts): semantic error returns with Write-Error -ErrorId on storage test failures

## Validacao
- Manually verified via regex and `bash -n` checks for valid PowerShell syntax.
- Pre-commit manual parsing checks applied.

## Rollback trigger
Rollback trigger: If the new `Write-Error` insertions cause unforeseen errors in PowerShell execution environments that strictly require single-line `if/throw` constructs or misinterpret `Write-Error` output formats.
