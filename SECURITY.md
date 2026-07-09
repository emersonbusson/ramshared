# Security Policy

## Supported Versions

Only the latest release version on the `main` branch is actively supported with security updates.

| Version | Supported |
| ------- | --------- |
| Latest  | Yes       |
| < older | No        |

## Reporting a Vulnerability

Because RamShared operates close to the hardware layer (kernel-mode drivers, memory locking, and swap space interfaces), security vulnerabilities could potentially lead to local privilege escalation (LPE), kernel panics, or host-guest escape.

**Do not report security vulnerabilities via public GitHub issues.**

Instead, please report security vulnerabilities responsibly by contacting the maintainers directly. If you find a vulnerability, email the core development team (or open a draft security advisory on GitHub if the platform supports it). 

Please include:
*   A detailed description of the vulnerability.
*   A proof-of-concept (PoC) script or steps to reproduce the issue.
*   The impact on system memory or hardware registers.

We will acknowledge your report within 48 hours and work with you to coordinate a patch and a public advisory.
