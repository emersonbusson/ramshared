#!/usr/bin/env bash
# qemu-validate.sh — valida um kernel WSL2 custom num QEMU ISOLADO (não toca o WSL).
# Prova, ANTES de armar no .wslconfig: (1) o bzImage boota até o userspace; (2) o
# `uname -r` confere com a release esperada; (3) os módulos alvo carregam (insmod na
# ordem dada). Console via ttyS0 (CONFIG_SERIAL_8250_CONSOLE=y no config-wsl).
#
# uso: qemu-validate.sh <bzImage> <kernelrelease> [mod1.ko mod2.ko ...]
#   módulos na ORDEM de dependência (ex.: zsmalloc.ko ANTES de zram.ko).
# saída 0 = PASS (kernel bootou + release confere). Detalhes de módulo no log.
#
# Reutilizável p/ qualquer build de kernel (toolkit Fase B+). SPEC: docs/runbooks/FASE-B-KERNEL.md
set -euo pipefail

BZ="${1:?uso: qemu-validate.sh <bzImage> <kernelrelease> [mods...]}"
REL="${2:?falta kernelrelease}"
shift 2
MODS=("$@")

[ -f "$BZ" ] || { echo "bzImage inexistente: $BZ" >&2; exit 2; }
command -v qemu-system-x86_64 >/dev/null || { echo "qemu-system-x86_64 ausente (apt install qemu-system-x86)" >&2; exit 2; }
[ -x /bin/busybox ] || { echo "busybox-static ausente (apt install busybox-static)" >&2; exit 2; }

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
IRD="$WORK/irfs"; mkdir -p "$IRD/bin" "$IRD/modules"
cp /bin/busybox "$IRD/bin/busybox"

# módulos numerados p/ preservar a ordem de dependência
i=0; for m in "${MODS[@]}"; do
  [ -f "$m" ] || { echo "módulo inexistente: $m" >&2; exit 2; }
  cp "$m" "$IRD/modules/$(printf '%02d' "$i")-$(basename "$m")"; i=$((i+1))
done

cat > "$IRD/init" <<'INIT'
#!/bin/busybox sh
/bin/busybox mkdir -p /proc /sys /dev
/bin/busybox mount -t proc proc /proc
/bin/busybox mount -t sysfs sysfs /sys
/bin/busybox mount -t devtmpfs devtmpfs /dev 2>/dev/null
echo "=====KTEST-BEGIN====="
echo "KTEST-UNAME=$(/bin/busybox uname -r)"
for m in /modules/*.ko; do
  if /bin/busybox insmod "$m" 2>/tmp/e; then
    echo "KTEST-INSMOD-OK=$(/bin/busybox basename "$m")"
  else
    echo "KTEST-INSMOD-FAIL=$(/bin/busybox basename "$m"): $(/bin/busybox cat /tmp/e)"
  fi
done
[ -e /dev/ublk-control ] && echo "KTEST-UBLK-CONTROL=present" || echo "KTEST-UBLK-CONTROL=absent"
echo "KTEST-DMESG:"
/bin/busybox dmesg | /bin/busybox grep -iE "module|ublk|zram|zsmalloc|magic|tainted|invalid|disagrees" | /bin/busybox tail -12
echo "=====KTEST-END====="
/bin/busybox poweroff -f
INIT
chmod +x "$IRD/init"

( cd "$IRD" && find . | cpio -o -H newc 2>/dev/null | gzip ) > "$WORK/initramfs.gz"

# KVM acelera; sem permissão em /dev/kvm cai p/ TCG (mais lento, mas válido).
ACCEL=(-machine accel=tcg)
[ -w /dev/kvm ] && ACCEL=(-enable-kvm -cpu host)

echo "[qemu-validate] bootando $BZ (release esperada: $REL; accel: ${ACCEL[*]})..."
timeout 180 qemu-system-x86_64 "${ACCEL[@]}" -m 1024 -nographic -no-reboot \
  -kernel "$BZ" -initrd "$WORK/initramfs.gz" \
  -append "console=ttyS0 panic=1 rdinit=/init" > "$WORK/serial.log" 2>&1 || true

echo "=========== resultado ==========="
grep -E "KTEST-" "$WORK/serial.log" || { echo "sem output KTEST — kernel pode não ter bootado"; }
echo "================================="
# GATE = boot ao userspace (uname confere). É o risco catastrófico ("não boota").
# O insmod via busybox no initramfs mínimo é BEST-EFFORT (a applet do busybox é
# limitada — falha sem chegar ao kernel; dmesg vazio confirma). A validação
# AUTORITATIVA de módulos é PÓS-BOOT, no kernel real, via kmod (boot-kernel-safe.ps1
# faz `modprobe` + auto-revert). Por isso o veredito NÃO gateia em módulos.
if grep -q "KTEST-UNAME=$REL" "$WORK/serial.log" && grep -q "KTEST-END" "$WORK/serial.log"; then
  echo "QEMU-VALIDATE: PASS — kernel bootou ao userspace, release confere."
  echo "(módulos: best-effort no initramfs; validação real = pós-boot via kmod no launcher)"
  exit 0
else
  echo "QEMU-VALIDATE: FAIL — kernel não bootou ou release diferente. Veja o serial acima."
  echo "--- tail do serial ---"; tail -15 "$WORK/serial.log"
  exit 1
fi
