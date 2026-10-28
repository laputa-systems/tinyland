#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p .work
rm -f .work/wayland-pixels-console.log .work/wayland-pixels.png .work/qemu-wayland-pixels.log .work/qmp-wayland.sock

bash harness/build-guest.sh 2>&1 | tail -n 5

TINYLAND_MODE=wayland-pixels
export TINYLAND_MODE

QEMU=qemu-system-x86_64
QMP_SOCK="$PWD/.work/qmp-wayland.sock"
CONSOLE="$PWD/.work/wayland-pixels-console.log"
rm -f "$QMP_SOCK"

cleanup() {
  if [ -n "${QEMU_PID:-}" ] && kill -0 "$QEMU_PID" 2>/dev/null; then
    python3 harness/qmp.py "$QMP_SOCK" quit >/dev/null 2>&1 || true
    sleep 2
    kill -9 "$QEMU_PID" 2>/dev/null || true
  fi
  if pgrep -f "qemu-system.*qmp-wayland.sock" >/dev/null 2>&1; then
    pkill -9 -f "qemu-system.*qmp-wayland.sock" || true
  fi
}
trap cleanup EXIT

$QEMU -M q35 -m 1024 -cpu host -enable-kvm \
  -vga none -device virtio-gpu-pci \
  -kernel .work/vmlinuz-virt -initrd .work/initramfs-virt \
  -append "console=tty0 console=ttyS0 root=/dev/vda rootfstype=ext4 rw init=/init TINYLAND_MODE=wayland-pixels" \
  -drive file=.work/guest.img,format=raw,if=virtio \
  -display none \
  -serial file:"$CONSOLE" \
  -qmp unix:"$QMP_SOCK",server=on,wait=off \
  -no-reboot > .work/qemu-wayland-pixels.log 2>&1 &
QEMU_PID=$!
echo "qemu pid $QEMU_PID"

READY=0
for i in $(seq 1 90); do
  if rg -q "TINYLAND_SURFACE_MAPPED" "$CONSOLE" 2>/dev/null; then READY=1; break; fi
  if ! kill -0 "$QEMU_PID" 2>/dev/null; then echo "qemu exited early"; cat "$CONSOLE" 2>/dev/null | tail -n 30; exit 1; fi
  sleep 1
done
if [ "$READY" != "1" ]; then echo "guest never became ready"; tail -n 30 "$CONSOLE" 2>/dev/null || true; exit 1; fi
sleep 2
if rg -i "protocol error|foot spawn failed|TINYLAND_CHILD_EXIT" "$CONSOLE"; then exit 1; fi
python3 harness/qmp.py "$QMP_SOCK" screendump "$PWD/.work/wayland-pixels.png"
python3 harness/check-screenshot.py .work/wayland-pixels.png 0.05 0.0001
python3 harness/qmp.py "$QMP_SOCK" quit >/dev/null 2>&1 || true
for i in $(seq 1 15); do
  kill -0 "$QEMU_PID" 2>/dev/null || break
  sleep 1
done
if kill -0 "$QEMU_PID" 2>/dev/null; then echo "qemu did not exit; killing"; kill -9 "$QEMU_PID"; exit 1; fi
trap - EXIT
echo "WAYLAND_PIXELS_OK"
