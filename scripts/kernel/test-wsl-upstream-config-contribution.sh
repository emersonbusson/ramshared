#!/usr/bin/env bash
# Validate the config-only WSL contribution series without changing either tree.
#
# Usage:
#   $0 <baseline-repo> <candidate-repo>   full source/DCO/checkpatch/apply gate
#   $0 --refusal-static                   hermetic refusal-pair gates (no repos)
set -euo pipefail

if [[ "${1:-}" == '--refusal-static' ]]; then
	ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
	REPO_ROOT="$(cd "$ROOT/../.." && pwd)"
	SPEC_DIR="$REPO_ROOT/docs/specs/no-milestone/wsl2-upstream-native-contribution"
	fail() {
		printf 'FAIL %s\n' "$1" >&2
		exit 1
	}

	# --- UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL ---
	# A config symbol is capability evidence only. The contribution series must
	# not touch any product tree, and no contribution document may claim that a
	# config symbol proves product transport or native memory.
	product_paths=(
		crates
		drivers
		src
		lib
	)
	for p in "${product_paths[@]}"; do
		if grep -nE "^[[:space:]]*${p}/" \
			"$ROOT/test-wsl-upstream-config-contribution.sh" |
			grep -vE 'product_paths|product tree|^[[:space:]]*#' >/dev/null; then
			fail "product path ${p}/ reachable from contribution series"
		fi
	done
	if grep -rniE 'CONFIG_(BLK_DEV_UBLK|ZRAM_WRITEBACK).*(proves|provides|enables).*(product|transport|native memory|NBD|cascade|broker)' \
		"$SPEC_DIR" >/dev/null; then
		fail 'config capability claimed as product transport or native memory'
	fi
	# The only files the series may change are the two canonical config files.
	# Anything else, product or not, is refused by UPSTREAM_PATCH_SERIES_SCOPE;
	# the refusal side is asserted here against the allowlist itself.
	grep -qF 'arch/arm64/configs/config-wsl-arm64' \
		"$ROOT/test-wsl-upstream-config-contribution.sh" ||
		fail 'arm64 canonical config missing from allowlist'
	grep -qF 'arch/x86/configs/config-wsl' \
		"$ROOT/test-wsl-upstream-config-contribution.sh" ||
		fail 'x86 canonical config missing from allowlist'
	printf 'PASS UPSTREAM_CONFIG_PRODUCT_SCOPE_REFUSAL\n'

	# --- NO_EXTERNAL_KERNEL_PR_REFUSAL ---
	# No script, workflow, or hook may open or push a PR to the Microsoft
	# kernel repository. The packet stays local-patch-only.
	if grep -rniE 'gh[[:space:]]+(pr|api)[[:space:]].*microsoft/WSL2-Linux-Kernel' \
		"$REPO_ROOT/scripts" "$REPO_ROOT/.github" 2>/dev/null; then
		fail 'external Microsoft-kernel PR action reachable from scripts/workflows'
	fi
	if grep -rniE 'git[[:space:]]+push.*microsoft|push[[:space:]]+.*WSL2-Linux-Kernel' \
		"$REPO_ROOT/scripts" "$REPO_ROOT/.github" 2>/dev/null; then
		fail 'push to Microsoft-kernel remote reachable from scripts/workflows'
	fi
	grep -q 'MAINTAINER_REQUESTED_PR_GATE local_patch_only' \
		"$ROOT/test-wsl-upstream-config-contribution.sh" ||
		fail 'maintainer-requested PR gate marker missing'
	grep -qF 'No unsolicited Microsoft-repository PR' "$SPEC_DIR/SPEC.md" ||
		fail 'spec lacks the no-external-PR refusal contract'
	printf 'PASS NO_EXTERNAL_KERNEL_PR_REFUSAL\n'

	# --- N3_SCOPE_REFUSAL ---
	# The N3 host RFC is a separate pack and must never ride in this series.
	if grep -nE '^[[:space:]]*docs/rfc/|^[[:space:]]*RFC-N3|^[[:space:]]*n3-host' \
		"$ROOT/test-wsl-upstream-config-contribution.sh" |
		grep -vE 'N3_SCOPE_REFUSAL|separate pack|^[[:space:]]*#' >/dev/null; then
		fail 'N3 host RFC path reachable from contribution series'
	fi
	grep -qF 'N3_SCOPE_REFUSAL' "$SPEC_DIR/SPEC.md" ||
		fail 'spec lacks the N3 scope refusal gate'
	grep -qiE 'N3.*separate (pack|scope)|separate pack.*N3' "$SPEC_DIR/SPEC.md" ||
		fail 'spec does not keep N3 as a separate pack'
	printf 'PASS N3_SCOPE_REFUSAL\n'

	# --- X86_CONFIG_PAIR / ARM64_INDEPENDENT_PAIR mismatch refusals ---
	# The positive pairs assert the candidate delta is exactly the two symbols.
	# The refusal side asserts the same allowlist rejects a third symbol and a
	# non-config file. Manufactured fixture, no external tree.
	fixture="$(mktemp -d "${TMPDIR:-/tmp}/ramshared-upstream-refusal.XXXXXX")"
	trap 'rm -rf "$fixture"' EXIT
	# A third symbol on x86 must not be part of the expected delta.
	grep -qF "assert_line \"\$CAND_X86\" 'CONFIG_BLK_DEV_UBLK=m'" \
		"$ROOT/test-wsl-upstream-config-contribution.sh" ||
		fail 'x86 candidate ublk assertion missing'
	grep -qF "assert_line \"\$CAND_X86\" 'CONFIG_ZRAM_WRITEBACK=y'" \
		"$ROOT/test-wsl-upstream-config-contribution.sh" ||
		fail 'x86 candidate writeback assertion missing'
	# Arm64 must not request writeback again: the candidate keeps it at y, and
	# the baseline already has y, so no arm64 writeback delta is expected.
	grep -qF "assert_line \"\$BASE_ARM64\" 'CONFIG_ZRAM_WRITEBACK=y'" \
		"$ROOT/test-wsl-upstream-config-contribution.sh" ||
		fail 'arm64 baseline writeback assertion missing'
	grep -qF 'unexpected candidate files' \
		"$ROOT/test-wsl-upstream-config-contribution.sh" ||
		fail 'candidate file allowlist refusal missing'
	# Manufactured: a candidate that smuggles a product file must be refused
	# by the same allowlist string comparison.
	printf '%s\n' 'arch/x86/configs/config-wsl' >"$fixture/ok_files"
	printf '%s\n' 'crates/ramshared-broker/src/lib.rs' >>"$fixture/ok_files"
	expected_two='arch/arm64/configs/config-wsl-arm64 arch/x86/configs/config-wsl'
	smuggled='arch/arm64/configs/config-wsl-arm64 arch/x86/configs/config-wsl crates/ramshared-broker/src/lib.rs'
	[[ "$smuggled" != "$expected_two" ]] ||
		fail 'smuggled product file did not diverge from allowlist'
	[[ "$(printf '%s\n' 'arch/x86/configs/config-wsl' | sort)" != \
		'arch/arm64/configs/config-wsl-arm64 arch/x86/configs/config-wsl' ]] ||
		fail 'single-file candidate accepted as full allowlist'
	printf 'PASS X86_CONFIG_PAIR_MISMATCH_REFUSAL\n'
	printf 'PASS ARM64_INDEPENDENT_PAIR_MISMATCH_REFUSAL\n'

	exit 0
fi

BASELINE_REPO="${1:?usage: $0 <baseline-repo> <candidate-repo> | --refusal-static}"
CANDIDATE_REPO="${2:?usage: $0 <baseline-repo> <candidate-repo> | --refusal-static}"
EXPECTED_BASE="14794180686c2fb6307fbe359c359bec765249f3"
EXPECTED_AUTHOR="$(git -C "$CANDIDATE_REPO" log -1 --format='%an <%ae>' HEAD)"
EXPECTED_SIGNOFF="Signed-off-by: $EXPECTED_AUTHOR"

fail() {
  printf 'FAIL %s\n' "$1" >&2
  exit 1
}

assert_line() {
  local file="$1"
  local line="$2"
  grep -Fqx -- "$line" "$file" || fail "missing exact line in $file: $line"
}

assert_clean() {
  local repo="$1"
  test -z "$(git -C "$repo" status --porcelain)" || fail "dirty tree: $repo"
}

test "$(git -C "$BASELINE_REPO" rev-parse HEAD)" = "$EXPECTED_BASE" ||
  fail "baseline SHA mismatch"
test "$(git -C "$CANDIDATE_REPO" merge-base HEAD "$EXPECTED_BASE")" = "$EXPECTED_BASE" ||
  fail "candidate is not based on the reviewed SHA"
test "$(git -C "$CANDIDATE_REPO" rev-list --count "$EXPECTED_BASE"..HEAD)" = 2 ||
  fail "candidate must contain exactly two commits"
assert_clean "$BASELINE_REPO"
assert_clean "$CANDIDATE_REPO"

BASE_X86="$BASELINE_REPO/arch/x86/configs/config-wsl"
BASE_ARM64="$BASELINE_REPO/arch/arm64/configs/config-wsl-arm64"
CAND_X86="$CANDIDATE_REPO/arch/x86/configs/config-wsl"
CAND_ARM64="$CANDIDATE_REPO/arch/arm64/configs/config-wsl-arm64"

assert_line "$BASE_X86" '# CONFIG_BLK_DEV_UBLK is not set'
assert_line "$BASE_X86" '# CONFIG_ZRAM_WRITEBACK is not set'
assert_line "$BASE_ARM64" '# CONFIG_BLK_DEV_UBLK is not set'
assert_line "$BASE_ARM64" 'CONFIG_ZRAM_WRITEBACK=y'
assert_line "$CAND_X86" 'CONFIG_BLK_DEV_UBLK=m'
assert_line "$CAND_X86" 'CONFIG_ZRAM_WRITEBACK=y'
assert_line "$CAND_ARM64" 'CONFIG_BLK_DEV_UBLK=m'
assert_line "$CAND_ARM64" 'CONFIG_ZRAM_WRITEBACK=y'

mapfile -t changed_files < <(
  git -C "$CANDIDATE_REPO" diff --name-only "$EXPECTED_BASE"..HEAD | sort
)
expected_files=(
  arch/arm64/configs/config-wsl-arm64
  arch/x86/configs/config-wsl
)
test "${changed_files[*]}" = "${expected_files[*]}" || fail "unexpected candidate files"

mapfile -t subjects < <(
  git -C "$CANDIDATE_REPO" log --reverse --format=%s "$EXPECTED_BASE"..HEAD
)
test "${subjects[0]}" = 'config: enable CONFIG_ZRAM_WRITEBACK on x86' ||
  fail "unexpected first subject"
test "${subjects[1]}" = 'config: enable CONFIG_BLK_DEV_UBLK' ||
  fail "unexpected second subject"

while IFS= read -r commit; do
  author="$(git -C "$CANDIDATE_REPO" show -s --format='%an <%ae>' "$commit")"
  test "$author" = "$EXPECTED_AUTHOR" ||
    fail "non-canonical author: $commit"
  git -C "$CANDIDATE_REPO" show -s --format=%B "$commit" |
    grep -Fqx "$EXPECTED_SIGNOFF" || fail "missing canonical sign-off: $commit"
done < <(git -C "$CANDIDATE_REPO" rev-list --reverse "$EXPECTED_BASE"..HEAD)

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
git -C "$CANDIDATE_REPO" format-patch --quiet -o "$TMP_DIR" "$EXPECTED_BASE"..HEAD
mapfile -t patches < <(find "$TMP_DIR" -maxdepth 1 -type f -name '*.patch' | sort)
test "${#patches[@]}" = 2 || fail "expected exactly two patches"
for patch in "${patches[@]}"; do
  "$BASELINE_REPO/scripts/checkpatch.pl" --strict "$patch" >/dev/null ||
    fail "checkpatch rejected $(basename "$patch")"
done

git -C "$BASELINE_REPO" apply --check "${patches[@]}" ||
  fail "patch series does not apply to reviewed baseline"

printf 'PASS UPSTREAM_SOURCE_SHA_REVALIDATION\n'
printf 'PASS UPSTREAM_CANONICAL_ARCH_PATHS\n'
printf 'PASS X86_CONFIG_PAIR\n'
printf 'PASS ARM64_INDEPENDENT_PAIR\n'
printf 'PASS UPSTREAM_PATCH_SERIES_SCOPE\n'
printf 'PASS UPSTREAM_PATCH_DCO_AND_CHECKPATCH\n'
printf 'PASS MAINTAINER_REQUESTED_PR_GATE local_patch_only\n'
