# WSL #40795 sanitized evidence checklist

Preparation only. Nothing in this checklist authorizes an external upload or
comment.

- Preserve the guest JSONL and Windows JSONL rings as independent sources.
- Bound every artifact to the incident start/end timestamps and boot ID.
- Include SHA-256 plus byte size for every retained file.
- Retain `comm`, systemd unit, cgroup, RSS, swap, CPU, and I/O only.
- Remove command lines, environment variables, user names, private paths,
  tokens, cookies, repository remotes, and unrelated process records.
- Keep `guest_pressure_unresponsive`, `guest_oom`,
  `kernel_warning_at_boot`, `kernel_crash`, `host_reboot`, and `wsl_terminate`
  as separate classifications with evidence and counterevidence.
- Record the exact WSL version, kernel identity, `.wslconfig` qualification,
  and whether the reproduction used a disposable VM.
- Review Microsoft's official collection script and its SHA-256 before a
  manual run; do not request a lifecycle restart on the daily host.
- Obtain explicit human approval before posting the draft or uploading any
  archive.
