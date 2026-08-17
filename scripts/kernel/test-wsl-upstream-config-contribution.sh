#!/usr/bin/env bash
# Validate the config-only WSL contribution series without changing either tree.
set -euo pipefail

BASELINE_REPO="${1:?usage: $0 <baseline-repo> <candidate-repo>}"
CANDIDATE_REPO="${2:?usage: $0 <baseline-repo> <candidate-repo>}"
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
