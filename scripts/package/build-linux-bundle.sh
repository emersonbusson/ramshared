#!/usr/bin/env bash
# Build a local Linux/WSL2 installable bundle for RamShared.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT_ROOT="${RAMSHARED_PACKAGE_OUT:-$ROOT/artifacts/packages}"
VERSION="${RAMSHARED_PACKAGE_VERSION:-$(git -C "$ROOT" describe --always --dirty --tags 2>/dev/null || date -u +%Y%m%d%H%M%S)}"
TARGET_DIR="$ROOT/target/release"
STAGE="$OUT_ROOT/ramshared-linux-$VERSION"
ARCHIVE="$OUT_ROOT/ramshared-linux-$VERSION.tar.gz"

usage() {
  cat <<'EOF'
Usage: scripts/package/build-linux-bundle.sh [--skip-build]

Builds release binaries and writes:
  artifacts/packages/ramshared-linux-<version>/
  artifacts/packages/ramshared-linux-<version>.tar.gz

The bundle contains Linux/WSL2 binaries, safety install scripts, systemd unit
templates, examples, and a SHA-256 manifest. It does not include secrets,
local VM notes, build caches, target directories, or Windows lab driver output.
EOF
}

SKIP_BUILD=0
for arg in "$@"; do
  case "$arg" in
    --skip-build) SKIP_BUILD=1 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "unsupported argument: $arg" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ "$SKIP_BUILD" -eq 0 ]]; then
  cargo build \
    -p ramshared-cli \
    -p ramshared-wsl2d \
    -p ramshared-agent \
    --release
fi

for bin in ramshared ramsharedd ramshared-agent ramshared-host-agent; do
  if [[ ! -x "$TARGET_DIR/$bin" ]]; then
    echo "missing release binary: $TARGET_DIR/$bin" >&2
    exit 1
  fi
done

rm -rf "$STAGE" "$ARCHIVE"
install -d -m 0755 "$STAGE/bin" "$STAGE/scripts/safety/systemd" "$STAGE/scripts/p0" \
  "$STAGE/scripts/package" "$STAGE/systemd" "$STAGE/docs"

for bin in ramshared ramsharedd ramshared-agent ramshared-host-agent; do
  install -m 0755 "$TARGET_DIR/$bin" "$STAGE/bin/$bin"
done

install -m 0755 "$ROOT/scripts/safety/install-cascade-boot.sh" "$STAGE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/uninstall-cascade-boot.sh" "$STAGE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/cascade-app.sh" "$STAGE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/cascade-preflight.sh" "$STAGE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/cascade-up.sh" "$STAGE/scripts/safety/"
install -m 0755 "$ROOT/scripts/safety/cascade-down.sh" "$STAGE/scripts/safety/"
install -m 0644 "$ROOT/scripts/safety/cascade.conf.example" "$STAGE/scripts/safety/"
install -m 0644 "$ROOT/scripts/safety/systemd/ramshared-cascade.service" "$STAGE/systemd/"
install -m 0644 "$ROOT/scripts/safety/systemd/ramshared-cascade.service" "$STAGE/scripts/safety/systemd/"

install -m 0644 "$ROOT/scripts/p0/measure-gpu-workload-vram.ps1" "$STAGE/scripts/p0/"
install -m 0644 "$ROOT/scripts/p0/Invoke-GpuWorkloadGate.ps1" "$STAGE/scripts/p0/"
install -m 0644 "$ROOT/scripts/p0/Start-CudaVramWorkload.ps1" "$STAGE/scripts/p0/"
install -m 0755 "$ROOT/scripts/package/build-linux-bundle.sh" "$STAGE/scripts/package/"

install -m 0644 "$ROOT/README.md" "$STAGE/docs/"
install -m 0644 "$ROOT/docs/FAQ.md" "$STAGE/docs/"
install -m 0644 "$ROOT/docs/packaging/INSTALLABLES.md" "$STAGE/docs/"
install -m 0644 "$ROOT/HYPERV-VM-ACCESS.md" "$STAGE/docs/" 2>/dev/null || true
install -m 0644 "$ROOT/docs/labs/HYPERV-VM-ACCESS.md" "$STAGE/docs/" 2>/dev/null || true

cat > "$STAGE/INSTALL.txt" <<EOF
RamShared Linux/WSL2 bundle: $VERSION

Quick smoke:
  ./bin/ramshared check
  ./bin/ramshared doctor

Opt-in boot install from an unpacked repo remains the safest install path:
  sudo RAMSHARED_BIN_DIR=\$PWD/bin bash scripts/safety/install-cascade-boot.sh

This bundle is app-agnostic. Use scripts/p0/Invoke-GpuWorkloadGate.ps1 from
Windows to measure any external GPU workload; it does not claim process attribution.
EOF

(
  cd "$STAGE"
  find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS
)

tar -C "$OUT_ROOT" -czf "$ARCHIVE" "ramshared-linux-$VERSION"
echo "bundle: $STAGE"
echo "archive: $ARCHIVE"
