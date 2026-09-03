# FINDING_ONLY

## Target File
`scripts/windows/Win11LabMediaContract.ps1`

## Finding
Safe code modification is not possible. The requested task requires adding guard clauses for disk space and network connectivity before starting large artifact downloads. However, `scripts/windows/Win11LabMediaContract.ps1` is a sealed unattended-media contract helper that only inspects, probes, and stages *existing* ISOs and local VHDs. It does not contain any logic for downloading artifacts (such as `Invoke-WebRequest` or `Start-BitsTransfer`). As the file lacks the requested logic entirely, safe and orthogonal code modification to validate network connectivity and disk space for downloads cannot be performed here.
