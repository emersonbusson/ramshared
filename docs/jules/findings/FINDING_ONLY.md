# Finding Only: Architectural Mismatch for Launcher Executable Validation

## Scope
Task: validate launcher executable paths and digital signatures
Target File: `scripts/windows/Manage-RamSharedLaunchers.ps1`

## Finding
Safe, orthogonal modification of `Manage-RamSharedLaunchers.ps1` to validate the digital signatures of `wsl.exe`, `wt.exe`, and `code` is **not possible** due to an architectural mismatch.

## Evidence
1. **Script Purpose:** `Manage-RamSharedLaunchers.ps1` does not directly execute these tools. It acts as an installer that simply writes fixed string templates into `.cmd` wrapper files (e.g., `@wsl.exe -d "$Distro" -- ramshared session --class interactive` and `@wsl.exe -d "$Distro" -- ramshared run --class interactive -- code .`).
2. **Execution Context:** The actual execution of these strings happens completely out-of-band when a user invokes the created `.cmd` scripts. The installer runs once and lacks the context of the user's runtime environment (PATH modifications, Windows Terminal installation status at run-time vs install-time, WSL target environment).
3. **Environment Boundary:** The executable `code` in the `ramshared-vscode.cmd` wrapper string is actually invoked *inside* the WSL guest environment (`@wsl.exe -d ... -- code .`). A Windows PowerShell script running on the host cannot natively use `Get-AuthenticodeSignature` to validate an ELF binary or a script inside the Linux guest.
4. **Tool Verification:** A standalone validation of `wsl.exe` or `wt.exe` at install time does not guarantee they will remain unaltered or available at execution time. Adding strict validation for `wt.exe` would also incorrectly block installation for users who do not have Windows Terminal installed but only intend to use the `wsl.exe` shell launcher.

## Conclusion
Adding guard clauses and signature validation to `Manage-RamSharedLaunchers.ps1` violates the Immutable Contract Rule 4 (If safe code modification is not possible, produce FINDING_ONLY with evidence). The validation must either occur at runtime within the `.cmd` wrapper logic (if they were PowerShell scripts instead of batch files) or the requirement must be rescoped. No changes were made to `scripts/windows/Manage-RamSharedLaunchers.ps1`.
