# Evidence index — autonomous Windows broker service

All PASS claims below map to committed machine-readable artifacts. Rejected
physical attempts remain available as negative evidence and are not counted as
successful runs.

| Surface | Named proof | Artifact |
| --- | --- | --- |
| Final broker binary | `BROKER_BINARY_MATCH`, `broker_event_log_transition`, SCM dependency/SIDs | `vm-final/broker-final-matrices.json` |
| Peer and status ACL | legitimate service SID, administrator/unrelated service refusal, read-only status pipe | `vm-final/broker-final-matrices.json` |
| Frame and cancellation boundary | oversized/partial/deny-only refusals, blocked read/accept cancellation, no mutation | `vm-final/broker-final-matrices.json` |
| Retry policy | retryable error filter, deadline, three SCM restarts then NONE, deterministic failure remains stopped | `vm-final/broker-final-matrices.json` |
| Online broker loss | safe stop without reconnect or residue | `vm-final/072311-before.json`, `vm-final/072311-results.json` |
| Earlier raw broker matrices | peer, retry, boundary | `vm/broker-20260725-073316-results.json`, `vm/broker-20260725-073330-results.json`, `vm/broker-20260725-074647-results.json` |
| VM package transactions | FreshInstall, Repair, ManufacturedRollback, UninstallRefusal, CleanUninstall | `package-final/` |
| VM lifecycle | three cold boots, SHA rounds, BINARY_MATCH, consumer-first stop, release and zero residue | `vm-final/` |
| Physical lifecycle | three cold boots with one manifest, SHA rounds, watchdog, final cleanup | `physical-final/` |
| Refused physical attempts | occupied `R:` collision; incomplete stop-command evidence | `physical-failed-r-collision/`, `physical-failed-stop-cmdlet/` |

The final broker matrices were rerun after adding the broker Event Log
transition. Their native executable SHA-256 is
`EE7C102F620B5F21947321EE93F16E9C6D174A406E7426165EA64B9A0D746911`.
