# AUDIT 2.5 — Campaign evidence lifecycle and custody

## Verdict

GO for the static governance implementation only.

## Findings resolved before implementation

1. A browser screenshot workflow was rejected because RamShared evidence is
   Linux/WSL2/Windows/kernel campaign proof, not a web surface.
2. A generic campaign executor was rejected because it would hide platform
   safety boundaries and has no two native consumers.
3. Automatic retention deletion was rejected: VHD, VM, swap, active service,
   volume, and protected evidence removal require an explicit owner and a
   separate authorized operation.
4. Historical evidence will be catalogued as immutable/unqualified rather than
   relabelled as current PASS.
5. A manifest self-hash was rejected. The checker instead requires an exact
   inventory set, allowing the manifest to describe and verify every sibling
   artifact without recursive hashing.

## Preconditions for implementation

- Tests must be RED before the lifecycle checker exists.
- The checker must be read-only and use only Node standard library APIs.
- New generated files must be deterministic and `docs/INDEX.md` must be
  regenerated after this SPEC directory is added.

## Re-audit trigger

Re-open this audit if a producer integration would change a Windows/WSL2
campaign execution path, retention proposes deletion, or a new manifest field
would carry private host identity or a binary/driver contract.
