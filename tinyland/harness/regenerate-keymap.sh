#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
: "${XKB_CONFIG_ROOT:?set XKB_CONFIG_ROOT to the pinned xkeyboard-config 2.43-r0 XKB data}"
mkdir -p .work/tmp
export TINYLAND_KEYMAP_OUT="$PWD/.work/tmp/us.xkb"
cargo test --locked -p tinyland --lib regenerate_embedded_keymap -- --ignored --exact input::tests::regenerate_embedded_keymap
cp "$TINYLAND_KEYMAP_OUT" data/us.xkb
