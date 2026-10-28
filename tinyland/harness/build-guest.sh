#!/bin/bash
# Build pinned Alpine guest rootfs + disk image + kernel for QEMU proofs.
set -euo pipefail
cd "$(dirname "$0")/.."

ALPINE_VERSION="${ALPINE_VERSION:-3.20.3}"
ALPINE_MIRROR="${ALPINE_MIRROR:-https://dl-cdn.alpinelinux.org/alpine}"
ARCH="x86_64"
DL=".work/dl"
ROOT=".work/guest-root"
IMG=".work/guest.img"

TARBALL="alpine-minirootfs-${ALPINE_VERSION}-${ARCH}.tar.gz"
URL="${ALPINE_MIRROR}/v3.20/releases/${ARCH}/${TARBALL}"
SHA256="d4e6fd67dcf75e40c451560ac7265166c2b72a0f38ddc9aae756a7de3d1efa0c"

mkdir -p "$DL" .work
if [ ! -f "$DL/$TARBALL" ]; then
  echo "downloading $URL"
  wget -O "$DL/$TARBALL" "$URL"
fi
echo "$SHA256  $DL/$TARBALL" | sha256sum -c -

if [ ! -f "$ROOT/.tinyland-seed" ]; then
  rm -rf "$ROOT"
  mkdir -p "$ROOT"
  tar -xzf "$DL/$TARBALL" -C "$ROOT"
  touch "$ROOT/.tinyland-seed"
fi

cat > "$ROOT/etc/apk/repositories" <<EOF
${ALPINE_MIRROR}/v3.20/main
${ALPINE_MIRROR}/v3.20/community
EOF

echo "installing guest packages"
if [ ! -f "$ROOT/.tinyland-pkgs-3.20.3" ]; then
  apk --root "$ROOT" --update-cache --initdb --arch "$ARCH" --allow-untrusted --usermode \
    add busybox linux-virt foot libxkbcommon font-terminus 2>&1 | tail -n 5
  touch "$ROOT/.tinyland-pkgs-3.20.3"
else
  echo "(packages cached)"
fi

echo "building tinyland (musl)"
cargo build --locked --target x86_64-unknown-linux-musl 2>&1 | tail -n 3
BIN="target/x86_64-unknown-linux-musl/debug/tinyland"
mkdir -p "$ROOT/usr/local/bin"
cp "$BIN" "$ROOT/usr/local/bin/tinyland"

cat > "$ROOT/init" <<'INIT_EOF'
#!/bin/sh
# initramfs already provides /proc /sys /dev (devtmpfs). Do not overmount /dev.
mount -o remount,rw / 2>/dev/null || true
mkdir -p /dev/pts /dev/shm /tmp
mount -t devpts none /dev/pts 2>/dev/null || true
export PATH=/usr/local/bin:/usr/bin:/bin:/sbin
export XDG_RUNTIME_DIR=/tmp
MODE="${TINYLAND_MODE:-drm-proof}"
echo "TINYLAND_BOOT mode=$MODE" > /dev/console
if [ "$MODE" = "drm-proof" ]; then
  /usr/local/bin/tinyland --drm-proof --width 1024 --height 768 > /tmp/drm.log 2>&1 &
  TPID=$!
  sleep 4
  echo "TINYLAND_READY" > /dev/console
  cat /tmp/drm.log > /dev/console
  mount -t debugfs none /sys/kernel/debug 2>/dev/null || true
  echo "=== /proc/fb ===" > /dev/console
  cat /proc/fb > /dev/console 2>&1 || true
  echo "=== dri state ===" > /dev/console
  cat /sys/kernel/debug/dri/0/state > /dev/console 2>&1 | head -n 60 || true
  echo "=== dri framebuffer ===" > /dev/console
  cat /sys/kernel/debug/dri/0/framebuffer > /dev/console 2>&1 | head -n 20 || true
  # Hold scanout for the host screendump; host powers us off via QMP.
  sleep 60
  wait $TPID
  echo "TINYLAND_DONE rc=$?" > /dev/console
elif [ "$MODE" = "wayland-pixels" ] || [ "$MODE" = "smoke" ]; then
  ( /usr/local/bin/tinyland --"$MODE" --width 1024 --height 768; echo "TINYLAND_EXIT rc=$?" ) > /dev/console 2>&1 &
  CPID=$!
  sleep 120
  wait "$CPID"
fi
echo "TINYLAND_POWERING_OFF" > /dev/console
sleep 1
poweroff -f
INIT_EOF
chmod +x "$ROOT/init"

cat > "$ROOT/usr/local/bin/guest-test" <<'GUEST_EOF'
#!/bin/sh
# Guest-side test program launched inside foot's terminal.
MODE="${1:-smoke}"
if [ "$MODE" = "wayland-pixels" ]; then
  printf '\033[48;5;201m WAYLAND_PIXELS_MARKER \033[0m\n'
  echo "WAYLAND_PIXELS_MAPPED" > /dev/console
  sleep 20
  exit 0
fi
printf '\033[48;5;201m LAPUTA_MARKER \033[0m\n'
echo "GUEST_PROMPT_READY" > /dev/console
if read -r line; then
  echo "GUEST_GOT_LINE: $line" > /dev/console
  if [ "$line" = "laputa" ]; then
    echo "GUEST_TEXT_OK" > /dev/console
    sleep 20
    exit 0
  else
    echo "GUEST_TEXT_MISMATCH" > /dev/console
    exit 1
  fi
else
  echo "GUEST_READ_FAILED" > /dev/console
  exit 1
fi
GUEST_EOF
chmod +x "$ROOT/usr/local/bin/guest-test"

mkdir -p "$ROOT/etc" "$ROOT/boot"
if [ -f "$ROOT/boot/vmlinuz-virt" ]; then
  cp "$ROOT/boot/vmlinuz-virt" .work/vmlinuz-virt
fi
if [ -f "$ROOT/boot/initramfs-virt" ]; then
  cp "$ROOT"/boot/initramfs-virt .work/initramfs-virt
fi

echo "recording installed versions"
if [ -f "$ROOT/lib/apk/db/installed" ]; then
  rg -a "^(P:|V:)" "$ROOT/lib/apk/db/installed" | sort > .work/guest-packages.txt || true
  rg -a "^P:" "$ROOT/lib/apk/db/installed" | sort >> .work/guest-packages.txt || true
fi
FOOT_VER="$(rg -a -A1 '^P:foot$' "$ROOT/lib/apk/db/installed" | rg -a '^V:' | head -n 1 || true)"
echo "guest foot: $FOOT_VER"

echo "building disk image"
rm -f "$IMG"
truncate -s 1G "$IMG"
mkfs.ext4 -q -d "$ROOT" "$IMG"
echo "BUILD_GUEST_OK"
