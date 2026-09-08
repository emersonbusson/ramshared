# FINDING_ONLY: Cannot add Hyper-V prerequisite check to ubuntu-latest runner

The task requests adding a Hyper-V prerequisite check to `.github/workflows/windows-lab.yml`.
However, inspecting the workflow file reveals that the job `windows-lab-plan` runs on `ubuntu-latest`:

```yaml
jobs:
  windows-lab-plan:
    name: windows-lab-plan
    runs-on: ubuntu-latest
```

Running Windows-specific PowerShell cmdlets (like `Get-WindowsOptionalFeature -FeatureName Microsoft-Hyper-V`) on an Ubuntu runner will fail because the module and underlying OS feature are not present on Linux.

According to the IMMUTABLE CONTRACT rule 4: "If safe code modification is not possible, produce FINDING_ONLY with evidence in docs/jules/findings/."

Therefore, no modifications to `.github/workflows/windows-lab.yml` have been made to avoid breaking the CI pipeline.
