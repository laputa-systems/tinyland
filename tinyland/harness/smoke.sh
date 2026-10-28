#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .work
rm -f .work/smoke-console.log .work/smoke.png .work/qemu-smoke.log .work/qmp-smoke.sock

bash harness/build-guest.sh 2>&1 | tail -n 5

TINYLAND_MODE=smoke
export TINYLAND_MODE

QEMU=qemu-system-x86_64
QMP_SOCK="$PWD/.work/qmp-smoke.sock"
CONSOLE="$PWD/.work/smoke-console.log"
COMPOSITOR="$PWD/.work/smoke-compositor.log"
rm -f "$COMPOSITOR"
rm -f "$QMP_SOCK"

cleanup() {
  if [ -n "${QEMU_PID:-}" ] && kill -0 "$QEMU_PID" 2>/dev/null; then
    python3 harness/qmp.py "$QMP_SOCK" quit >/dev/null 2>&1 || true
    sleep 2
    kill -9 "$QEMU_PID" 2>/dev/null || true
  fi
  if pgrep -f "qemu-system.*qmp-smoke.sock" >/dev/null 2>&1; then
    pkill -9 -f "qemu-system.*qmp-smoke.sock" || true
  fi
}
trap cleanup EXIT

$QEMU -M q35 -m 1024 -cpu host -enable-kvm \
  -vga none -device virtio-gpu-pci -device virtio-keyboard-pci \
  -kernel .work/vmlinuz-virt -initrd .work/initramfs-virt \
  -append "console=tty0 console=ttyS0 root=/dev/vda rootfstype=ext4 rw init=/init TINYLAND_MODE=smoke" \
  -drive file=.work/guest.img,format=raw,if=virtio \
  -display none \
  -serial file:"$CONSOLE" -serial file:"$COMPOSITOR" \
  -qmp unix:"$QMP_SOCK",server=on,wait=off \
  -no-reboot > .work/qemu-smoke.log 2>&1 &
QEMU_PID=$!
echo "qemu pid $QEMU_PID"

READY=0
for i in $(seq 1 90); do
  if rg -q "GUEST_PROMPT_READY" "$CONSOLE" 2>/dev/null && rg -q "TINYLAND_SURFACE_MAPPED" "$CONSOLE"; then READY=1; break; fi
  if rg -q "TINYLAND_EXIT|TINYLAND_CHILD_EXIT|GUEST_TEXT_MISMATCH|GUEST_READ_FAILED" "$CONSOLE" 2>/dev/null; then tail -n 30 "$CONSOLE"; exit 1; fi
  if ! kill -0 "$QEMU_PID" 2>/dev/null; then echo "qemu exited early"; cat "$CONSOLE" 2>/dev/null | tail -n 30; exit 1; fi
  sleep 1
done
if [ "$READY" != "1" ]; then echo "guest never became ready"; tail -n 30 "$CONSOLE" 2>/dev/null || true; exit 1; fi
python3 harness/qmp.py "$QMP_SOCK" input
TEXT_OK=0
for i in $(seq 1 20); do
  if rg -q "GUEST_TEXT_OK" "$CONSOLE" 2>/dev/null; then TEXT_OK=1; break; fi
  if rg -q "TINYLAND_EXIT|TINYLAND_CHILD_EXIT|GUEST_TEXT_MISMATCH|GUEST_READ_FAILED" "$CONSOLE" 2>/dev/null; then tail -n 30 "$CONSOLE"; exit 1; fi
  if ! kill -0 "$QEMU_PID" 2>/dev/null; then echo "qemu exited before input proof"; exit 1; fi
  sleep 1
done
if [ "$TEXT_OK" != "1" ]; then echo "guest did not receive laputa"; tail -n 30 "$CONSOLE"; exit 1; fi
sleep 1
if rg -i "protocol error|foot spawn failed|TINYLAND_CHILD_EXIT" "$CONSOLE"; then exit 1; fi
python3 harness/qmp.py "$QMP_SOCK" screendump "$PWD/.work/smoke.png"
python3 harness/check-screenshot.py .work/smoke.png 0.05 0.0001
python3 harness/qmp.py "$QMP_SOCK" quit >/dev/null 2>&1 || true
for i in $(seq 1 15); do
  kill -0 "$QEMU_PID" 2>/dev/null || break
  sleep 1
done
if kill -0 "$QEMU_PID" 2>/dev/null; then echo "qemu did not exit; killing"; kill -9 "$QEMU_PID"; exit 1; fi
trap - EXIT
echo "SMOKE_OK"
