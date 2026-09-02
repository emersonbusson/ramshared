#!/usr/bin/env bash
# Build a sealed, NBD-only Linux/WSL2 release payload. Installation is a
# separate explicit transaction into /opt/ramshared/releases/<version>.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
OUT_ROOT=${RAMSHARED_PACKAGE_OUT:-$ROOT/artifacts/packages}
VERSION=${RAMSHARED_PACKAGE_VERSION:-$(git -C "$ROOT" describe --always --dirty --tags 2>/dev/null || date -u +%Y%m%d%H%M%S)}
SOURCE_COMMIT=$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || true)
SOURCE_BRANCH=$(git -C "$ROOT" symbolic-ref --short -q HEAD 2>/dev/null || printf 'detached')
SOURCE_TREE_STATE=$(if git -C "$ROOT" diff --quiet --ignore-submodules HEAD -- 2>/dev/null && \
  [[ -z $(git -C "$ROOT" ls-files --others --exclude-standard 2>/dev/null) ]]; then printf 'clean'; else printf 'dirty'; fi)
TARGET_DIR="$ROOT/target/release"
STAGE="$OUT_ROOT/ramshared-linux-$VERSION"
STAGE_RELEASE="$STAGE/release"
ARCHIVE="$OUT_ROOT/ramshared-linux-$VERSION.tar.gz"
STAGE_DEBUG="$OUT_ROOT/ramshared-linux-$VERSION-debug"
STAGE_DEBUG_RELEASE="$STAGE_DEBUG/release"
ARCHIVE_DEBUG="$OUT_ROOT/ramshared-linux-$VERSION-debug.tar.gz"

usage() {
  cat <<'EOF'
Usage: scripts/package/build-linux-bundle.sh [--skip-build]

Builds an NBD-only release payload under:
  artifacts/packages/ramshared-linux-<version>/release/

The payload is copied by its sealed installer only to
/opt/ramshared/releases/<version>, then selected atomically. This builder
never performs that installation or starts a lifecycle action.

The archive is universal and contains no host lower-sink binding. The attended
installer derives one installed release with its lower-sink binding after the
operator has supplied the exact install approval.
EOF
}

SKIP_BUILD=0
while [[ $# -gt 0 ]]; do
  case $1 in
    --skip-build) SKIP_BUILD=1 ;;
    --help|-h) usage; exit 0 ;;
    *) printf 'unsupported argument: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

if [[ -n ${RAMSHARED_PACKAGE_TARGET_DIR:-} ]]; then
  [[ ${RAMSHARED_PACKAGE_TEST_MODE:-} == 1 && $SKIP_BUILD -eq 1 ]] || {
    printf 'custom package target is test-only and requires --skip-build\n' >&2
    exit 2
  }
  [[ $RAMSHARED_PACKAGE_TARGET_DIR == /* && -d $RAMSHARED_PACKAGE_TARGET_DIR \
    && ! -L $RAMSHARED_PACKAGE_TARGET_DIR ]] || {
    printf 'invalid package test target directory\n' >&2
    exit 2
  }
  TARGET_DIR=$RAMSHARED_PACKAGE_TARGET_DIR
fi

[[ $VERSION =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]] || {
  printf 'invalid package version: %s\n' "$VERSION" >&2
  exit 2
}
[[ ! -e $STAGE && ! -e $ARCHIVE ]] || {
  printf 'refuse to replace an existing package artifact: %s\n' "$STAGE" >&2
  exit 1
}
[[ ! -e $STAGE_DEBUG && ! -e $ARCHIVE_DEBUG ]] || {
  printf 'refuse to replace an existing debug package artifact: %s\n' "$STAGE_DEBUG" >&2
  exit 1
}

command -v objcopy >/dev/null || { printf 'missing objcopy\n' >&2; exit 69; }
command -v strip >/dev/null || { printf 'missing strip\n' >&2; exit 69; }

if [[ $SKIP_BUILD -eq 0 ]]; then
  cargo build -p ramshared-cli -p ramshared-wsl2d --release
fi

for binary in ramshared ramsharedd; do
  [[ -x $TARGET_DIR/$binary ]] || {
    printf 'missing release binary: %s\n' "$TARGET_DIR/$binary" >&2
    exit 1
  }
done

install -d -m 0755 "$STAGE_RELEASE/bin" "$STAGE_RELEASE/scripts/safety" "$STAGE_RELEASE/systemd" \
  "$STAGE_RELEASE/systemd/docker.service.d" "$STAGE_RELEASE/systemd/containerd.service.d" \
  "$STAGE_RELEASE/systemd/cron.service.d"
install -d -m 0755 "$STAGE_DEBUG_RELEASE/bin"
for binary in ramshared ramsharedd; do
  install -m 0755 "$TARGET_DIR/$binary" "$STAGE_RELEASE/bin/$binary"
  objcopy --only-keep-debug "$STAGE_RELEASE/bin/$binary" "$STAGE_DEBUG_RELEASE/bin/$binary.debug"
  strip --strip-debug "$STAGE_RELEASE/bin/$binary"
  objcopy --add-gnu-debuglink="$STAGE_DEBUG_RELEASE/bin/$binary.debug" "$STAGE_RELEASE/bin/$binary"
done
install -m 0755 "$ROOT/scripts/safety/install-cascade-boot.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/uninstall-cascade-boot.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/cascade-up.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/cascade-down.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/cascade-controller.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/provision-origin-swap.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/lifecycle-recovery-status.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/cascade-health.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/configure-btop-observability.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/nbd-product-preflight.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/nbd-benchmark-cell.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/nbd-benchmark-cgroup-launch.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0644 "$ROOT/scripts/safety/nbd-benchmark-lib.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/cascade_pressure_integrity_worker.py" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/wsl-relay-health.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/manage-control-plane.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/ramshared-host-gate.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/ramshared-session-launcher.sh" "$STAGE_RELEASE/scripts/safety/"
install -m 0644 "$ROOT/scripts/safety/docker-daemon-ramshared.json" "$STAGE_RELEASE/scripts/safety/"
install -m 0644 "$ROOT/scripts/safety/cascade.conf.example" "$STAGE_RELEASE/scripts/safety/"
install -m 0644 "$ROOT/scripts/safety/systemd/ramshared-cascade.service" "$STAGE_RELEASE/systemd/"
install -m 0644 "$ROOT/scripts/safety/systemd/ramshared-cascade-health.service" "$STAGE_RELEASE/systemd/"
install -m 0644 "$ROOT/scripts/safety/systemd/ramshared-workloads.slice" "$STAGE_RELEASE/systemd/"
for unit in ramshared-control.slice ramshared-workloads-docker.slice ramshared-workloads-cron.slice \
  ramshared-host-gate.service ramshared-supervisor.service ramshared-cron-workload.service.in; do
  install -m 0644 "$ROOT/scripts/safety/systemd/$unit" "$STAGE_RELEASE/systemd/"
done
install -m 0644 "$ROOT/scripts/safety/systemd/docker.service.d/10-ramshared-control.conf" \
  "$STAGE_RELEASE/systemd/docker.service.d/"
install -m 0644 "$ROOT/scripts/safety/systemd/containerd.service.d/10-ramshared-control.conf" \
  "$STAGE_RELEASE/systemd/containerd.service.d/"
install -m 0644 "$ROOT/scripts/safety/systemd/cron.service.d/10-ramshared-control.conf" \
  "$STAGE_RELEASE/systemd/cron.service.d/"
printf '%s\n' "$VERSION" >"$STAGE_RELEASE/RELEASE_VERSION"
printf '%s\n' "$VERSION" >"$STAGE_DEBUG_RELEASE/RELEASE_VERSION"
chmod 0644 "$STAGE_RELEASE/RELEASE_VERSION" "$STAGE_DEBUG_RELEASE/RELEASE_VERSION"
[[ $SOURCE_COMMIT =~ ^[0-9a-f]{40}$ ]] || {
  printf 'source commit is unavailable\n' >&2
  exit 1
}
printf '%s\n' "$SOURCE_COMMIT" >"$STAGE_RELEASE/SOURCE_COMMIT"
printf '%s\n' "$SOURCE_COMMIT" >"$STAGE_DEBUG_RELEASE/SOURCE_COMMIT"
[[ $SOURCE_BRANCH =~ ^[A-Za-z0-9._/-]{1,200}$ ]] || {
  printf 'source branch is invalid\n' >&2
  exit 1
}
printf '%s\n' "$SOURCE_BRANCH" >"$STAGE_RELEASE/SOURCE_BRANCH"
printf '%s\n' "$SOURCE_BRANCH" >"$STAGE_DEBUG_RELEASE/SOURCE_BRANCH"
printf '%s\n' "$SOURCE_TREE_STATE" >"$STAGE_RELEASE/SOURCE_TREE_STATE"
printf '%s\n' "$SOURCE_TREE_STATE" >"$STAGE_DEBUG_RELEASE/SOURCE_TREE_STATE"
chmod 0644 "$STAGE_RELEASE/SOURCE_COMMIT" "$STAGE_RELEASE/SOURCE_BRANCH" "$STAGE_RELEASE/SOURCE_TREE_STATE" \
  "$STAGE_DEBUG_RELEASE/SOURCE_COMMIT" "$STAGE_DEBUG_RELEASE/SOURCE_BRANCH" "$STAGE_DEBUG_RELEASE/SOURCE_TREE_STATE"

(
  cd "$STAGE_RELEASE"
  find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
)
chmod 0644 "$STAGE_RELEASE/SHA256SUMS"
(
  cd "$STAGE_DEBUG_RELEASE"
  find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
)
chmod 0644 "$STAGE_DEBUG_RELEASE/SHA256SUMS"

tar -C "$OUT_ROOT" -czf "$ARCHIVE" "ramshared-linux-$VERSION"
tar -C "$OUT_ROOT" -czf "$ARCHIVE_DEBUG" "ramshared-linux-$VERSION-debug"
printf 'bundle_release=%s\n' "$STAGE_RELEASE"
printf 'bundle_archive=%s\n' "$ARCHIVE"
printf 'bundle_debug_release=%s\n' "$STAGE_DEBUG_RELEASE"
printf 'bundle_debug_archive=%s\n' "$ARCHIVE_DEBUG"
