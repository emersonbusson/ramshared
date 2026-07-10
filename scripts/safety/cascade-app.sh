#!/usr/bin/env bash
# cascade-app.sh — simple control surface for the Day-1 memory cushion.
# SPEC: docs/specs/no-milestone/cascade-desktop-app/SPEC.md
#
# GUI (WSLg/Linux):  ./cascade-app.sh --gui
# CLI:               ./cascade-app.sh start|stop|status|check|enable-boot|disable-boot
set -euo pipefail

REPO="${RAMSHARED_REPO:-$(cd "$(dirname "$0")/../.." && pwd)}"
SCRIPTS="$REPO/scripts/safety"
BIN_DIR="${RAMSHARED_BIN_DIR:-}"
if [[ -z "$BIN_DIR" ]]; then
  if [[ -x "$REPO/target/release/ramshared" ]]; then
    BIN_DIR="$REPO/target/release"
  elif [[ -x "$REPO/target/debug/ramshared" ]]; then
    BIN_DIR="$REPO/target/debug"
  else
    BIN_DIR="$REPO/target/release"
  fi
fi
CLI="${RAMSHARED_CLI:-$BIN_DIR/ramshared}"
export RAMSHARED_REPO="$REPO"
export RAMSHARED_BIN_DIR="$BIN_DIR"
export PATH="/usr/lib/wsl/lib:${PATH:-}"

MODE=""
CMD=""
for arg in "$@"; do
  case "$arg" in
    --gui) MODE=gui ;;
    --cli) MODE=cli ;;
    -h|--help)
      cat <<EOF
RamShared control app (cascade cushion)

  $0 [--gui|--cli] [command]

Commands:
  menu          interactive (default with --gui)
  start         turn cushion on  (needs root)
  stop          turn cushion off (needs root; always swapoff-first)
  status        show swap lines
  check         preflight machine
  enable-boot   opt-in systemd auto-start (needs root)
  disable-boot  remove boot unit (needs root)

Examples:
  $0 status
  $0 --gui
  sudo $0 start
EOF
      exit 0
      ;;
    *)
      if [[ -z "$CMD" ]]; then CMD="$arg"; fi
      ;;
  esac
done

have_gui() {
  [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]] && command -v zenity >/dev/null 2>&1
}

notify() {
  local title="$1" body="$2"
  if command -v notify-send >/dev/null 2>&1; then
    notify-send --app-name=RamShared "$title" "$body" 2>/dev/null || true
  fi
  echo "[$title] $body"
}

die_gui() {
  local msg="$1"
  if have_gui && [[ "$MODE" == "gui" || -z "${CMD:-}" ]]; then
    zenity --error --title="RamShared" --width=420 --text="$msg" 2>/dev/null || true
  fi
  echo "error: $msg" >&2
  return 1
}

info_gui() {
  local msg="$1"
  if have_gui && [[ "${MODE:-}" == "gui" ]]; then
    zenity --info --title="RamShared" --width=420 --text="$msg" 2>/dev/null || true
  fi
}

need_bins() {
  if [[ ! -x "$CLI" ]]; then
    die_gui "ramshared binary not found at:
$CLI

Build first:
  cargo build -p ramshared-cli -p ramshared-wsl2d --release"
    return 1
  fi
}

# Re-exec under pkexec/sudo for privileged actions when not root.
ensure_root() {
  if [[ "$(id -u)" -eq 0 ]]; then
    return 0
  fi
  if command -v pkexec >/dev/null 2>&1; then
    exec pkexec env \
      RAMSHARED_REPO="$REPO" \
      RAMSHARED_BIN_DIR="$BIN_DIR" \
      DISPLAY="${DISPLAY:-}" \
      WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}" \
      XAUTHORITY="${XAUTHORITY:-}" \
      bash "$0" --cli "$1"
  fi
  die_gui "This action needs root.

Try:
  sudo $0 $1

Or install policykit (pkexec)."
  return 1
}

cmd_status() {
  need_bins
  local out ghosts=0
  out="$(swapon --show 2>/dev/null || true)"
  if [[ -z "$out" ]]; then
    out="(no swap lines — unusual)"
  fi
  if grep -qiE 'deleted|\\\\040' /proc/swaps 2>/dev/null; then
    ghosts=1
  fi
  local summary
  summary="Swap right now:

$out

"
  if [[ "$ghosts" -eq 1 ]]; then
    summary+="WARNING: ghost swap detected (deleted device).
Do not kill processes by hand. On Windows run: wsl --shutdown
Then reopen the distro and: sudo $0 stop"
  elif echo "$out" | grep -qiE 'nbd|zram|ublk'; then
    summary+="Looks like the cushion is (at least partly) on."
  else
    summary+="Only disk swap seen — cushion is off (or never started)."
  fi
  echo "$summary"
  if have_gui && [[ "${MODE:-}" == "gui" ]]; then
    zenity --info --title="RamShared status" --width=520 --text="$summary" 2>/dev/null || true
  fi
}

cmd_check() {
  need_bins
  local out rc=0
  set +e
  out="$("$CLI" check 2>&1)"
  rc=$?
  set -e
  echo "$out"
  if [[ "$rc" -eq 0 ]]; then
    notify "RamShared" "Check OK — machine looks ready."
    info_gui "Check OK.

Machine looks ready for the cushion."
  else
    notify "RamShared" "Check blocked — see doctor."
    set +e
    local doc
    doc="$("$CLI" doctor 2>&1)"
    set -e
    die_gui "Check blocked.

$out

Doctor:
$doc" || true
    return "$rc"
  fi
}

cmd_start() {
  ensure_root start
  need_bins
  if [[ -x "$SCRIPTS/cascade-preflight.sh" ]]; then
    if ! "$SCRIPTS/cascade-preflight.sh"; then
      die_gui "Preflight refused to start (safe).
See terminal / journal for details.
Common fixes: free some GPU memory, or lower VRAM_MIB in /etc/ramshared/cascade.conf"
      return 1
    fi
  fi
  if "$CLI" up; then
    notify "RamShared" "Cushion is on."
    info_gui "Cushion is on.

Run Status anytime to see zram / GPU / disk lines."
    cmd_status || true
  else
    die_gui "Failed to start cushion.
Try: sudo $CLI doctor"
    return 1
  fi
}

cmd_stop() {
  ensure_root stop
  need_bins
  if "$CLI" down; then
    notify "RamShared" "Cushion is off (clean shutdown)."
    info_gui "Cushion is off.

Shutdown used the safe path (swap off before stopping the daemon)."
  else
    die_gui "Stop failed.
If you see ghost swap, on Windows: wsl --shutdown"
    return 1
  fi
}

cmd_enable_boot() {
  ensure_root enable-boot
  if [[ ! -x "$SCRIPTS/install-cascade-boot.sh" ]]; then
    die_gui "install-cascade-boot.sh missing"
    return 1
  fi
  if bash "$SCRIPTS/install-cascade-boot.sh" --enable; then
    notify "RamShared" "Boot auto-start enabled."
    info_gui "Boot auto-start enabled.

Needs systemd in the distro. Edit sizes in:
/etc/ramshared/cascade.conf"
  else
    die_gui "Could not enable boot auto-start."
    return 1
  fi
}

cmd_disable_boot() {
  ensure_root disable-boot
  if [[ ! -x "$SCRIPTS/uninstall-cascade-boot.sh" ]]; then
    die_gui "uninstall-cascade-boot.sh missing"
    return 1
  fi
  if bash "$SCRIPTS/uninstall-cascade-boot.sh"; then
    notify "RamShared" "Boot auto-start removed."
    info_gui "Boot auto-start removed (cascade conf left in place)."
  else
    die_gui "Could not disable boot unit."
    return 1
  fi
}

cmd_menu() {
  if ! have_gui; then
    echo "No zenity/DISPLAY — use CLI commands. Try: $0 status"
    cmd_status || true
    return 0
  fi
  MODE=gui
  while true; do
    local choice
    choice="$(
      zenity --list --title="RamShared Cushion" --width=440 --height=360 \
        --text="Idle GPU memory as a spare cushion when RAM is tight.
Give it back when a game needs the card.

Needs sudo/pkexec for start/stop/boot." \
        --column="Action" --column="What it does" \
        start "Turn cushion ON" \
        stop "Turn cushion OFF (safe)" \
        status "Show swap right now" \
        check "Is this machine ready?" \
        enable-boot "Auto-start on WSL boot" \
        disable-boot "Remove auto-start" \
        quit "Close" 2>/dev/null
    )" || choice="quit"
    case "$choice" in
      start) cmd_start || true ;;
      stop) cmd_stop || true ;;
      status) cmd_status || true ;;
      check) cmd_check || true ;;
      enable-boot) cmd_enable_boot || true ;;
      disable-boot) cmd_disable_boot || true ;;
      quit|"") break ;;
      *) break ;;
    esac
  done
}

# Default mode
if [[ -z "$MODE" ]]; then
  if have_gui && [[ -z "$CMD" || "$CMD" == "menu" ]]; then
    MODE=gui
  else
    MODE=cli
  fi
fi

if [[ -z "$CMD" ]]; then
  if [[ "$MODE" == "gui" ]]; then
    CMD=menu
  else
    CMD=status
  fi
fi

case "$CMD" in
  menu) cmd_menu ;;
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  check) cmd_check ;;
  enable-boot) cmd_enable_boot ;;
  disable-boot) cmd_disable_boot ;;
  *)
    echo "unknown command: $CMD" >&2
    exit 2
    ;;
esac
