#!/usr/bin/env bash
# test-preflight-reserve-floor.sh — ITEM-5 of gpu-reserve-floor-authority.
#
# Named test `preflight_refuses_below_sealed_floor` (DT-8): the preflight gate
# must refuse a reserve-floor override below the sealed authority instead of
# clamping it up. A raise-only override at or above the seal must be accepted.
#
# The gate is exercised only for its reserve-floor decision: no GPU, ublk or
# swap state is touched, and the binary/GPU steps are skipped by pointing the
# gate at a missing binary so the failure is deterministic and early.
set -euo pipefail

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
preflight="$root/scripts/safety/preflight.sh"
[[ -f $preflight && ! -L $preflight ]] || {
  echo "preflight.sh missing or a symlink: $preflight" >&2
  exit 1
}

# The sealed literals the gate enforces must match the Rust authority.
# Values are extracted from both sides and compared numerically so a change to
# either one alone fails this test instead of silently drifting.
rust_policy="$root/crates/ramshared-vram/src/reserve_policy.rs"
[[ -f $rust_policy ]] || {
  echo "sealed authority source missing: $rust_policy" >&2
  exit 1
}
for name in SEALED_RESERVE_MIN_MIB SEALED_RESERVE_PERCENT; do
  shell_value=$(grep -E "^${name}=" "$preflight" | head -1 | cut -d= -f2 | tr -dc '0-9')
  rust_value=$(grep -E "pub const ${name}: u64 =" "$rust_policy" | head -1 | sed -E 's/.*= *([0-9]+).*/\1/')
  [[ -n $shell_value && -n $rust_value ]] || {
    echo "cannot read $name from preflight ('$shell_value') and reserve_policy ('$rust_value')" >&2
    exit 1
  }
  [[ $shell_value == "$rust_value" ]] || {
    echo "$name drifts: preflight=$shell_value reserve_policy=$rust_value" >&2
    exit 1
  }
done

# The clamp-to-a-low-default reader must be gone (DT-8). Match the
# assignment form only — the explanatory comment above is allowed to quote it.
if grep -Eq '^[[:space:]]*MIN_VRAM_FREE_MIB=.*:-256' "$preflight"; then
  echo "preflight still defaults the reserve floor to 256 MiB" >&2
  exit 1
fi
if ! grep -Fq 'raise-only' "$preflight"; then
  echo "preflight must state the raise-only rule (DT-8)" >&2
  exit 1
fi

run_gate() {
  # Unset both override names, then set only what the case supplies.
  env -u RAMSHARED_MIN_VRAM_FREE_MIB -u MIN_VRAM_HEADROOM_MIB \
    RAMSHARED_MIN_VRAM_FREE_MIB="$1" \
    bash "$preflight" "$root/target/debug/__no_such_ramsharedd__" 2>&1 || true
}

# Kahneman #13 — refusal plus legitimate pass, both directions.

# 1. Below the seal: refused with the raise-only diagnostic (DT-8).
below=$(run_gate 128)
grep -Fq 'is below the sealed authority' <<<"$below" || {
  echo "an override below the seal must be refused, got: $below" >&2
  exit 1
}
grep -Fq 'raise-only' <<<"$below" || {
  echo "the refusal must name the raise-only rule, got: $below" >&2
  exit 1
}

# 2. At the seal: the reserve check is reached and not refused for being low.
#    The gate then fails on the missing binary, which is the expected outcome
#    for this fixture and proves the reserve check passed.
at_seal=$(run_gate 2048)
if grep -Fq 'is below the sealed authority' <<<"$at_seal"; then
  echo "an override at the seal must not be refused as too low: $at_seal" >&2
  exit 1
fi

# 3. Above the seal: accepted (raise-only means more conservative is legal).
above=$(run_gate 4096)
if grep -Fq 'is below the sealed authority' <<<"$above"; then
  echo "a raise above the seal must be accepted: $above" >&2
  exit 1
fi

# 4. Non-numeric override: refused, not coerced to zero.
junk=$(run_gate 'abc')
grep -Fq 'is not numeric' <<<"$junk" || {
  echo "a non-numeric override must be refused, got: $junk" >&2
  exit 1
}

# 5. DT-8 raise-only: a value at or above the seal is accepted and is what the
#    gate enforces. The gate then fails on the missing binary; the refusal
#    must NOT be the reserve-floor one, which proves the override was honored.
honored=$(run_gate 3072)
if grep -Fq 'is below the sealed authority' <<<"$honored"; then
  echo "a raise-only override at 3072 MiB must be honored: $honored" >&2
  exit 1
fi
# The alias name is accepted under the same raise-only rule (DT-8).
alias_out=$(env -u RAMSHARED_MIN_VRAM_FREE_MIB -u MIN_VRAM_HEADROOM_MIB \
  MIN_VRAM_HEADROOM_MIB=4096 \
  bash "$preflight" "$root/target/debug/__no_such_ramsharedd__" 2>&1 || true)
if grep -Fq 'is below the sealed authority' <<<"$alias_out"; then
  echo "the MIN_VRAM_HEADROOM_MIB alias must follow raise-only: $alias_out" >&2
  exit 1
fi
# A value below the seal through the alias is refused the same way.
alias_low=$(env -u RAMSHARED_MIN_VRAM_FREE_MIB -u MIN_VRAM_HEADROOM_MIB \
  MIN_VRAM_HEADROOM_MIB=512 \
  bash "$preflight" "$root/target/debug/__no_such_ramsharedd__" 2>&1 || true)
grep -Fq 'is below the sealed authority' <<<"$alias_low" || {
  echo "the alias must refuse a below-seal raise, got: $alias_low" >&2
  exit 1
}

echo "preflight_honors_raise_only_override: ok"
echo "preflight_refuses_below_sealed_floor: ok"
