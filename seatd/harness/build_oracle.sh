#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
exclude=$(git rev-parse --git-path info/exclude)
if ! rg -q -x -F '.work/' "$exclude"; then
	printf '\n.work/\n' >> "$exclude"
fi
mkdir -p .work/tmp/reference
export TMPDIR="$PWD/.work/tmp"
git archive bc0e96f | tar -x -C .work/tmp/reference
if [ -d .work/tmp/oracle ]; then
	rm -rf .work/tmp/oracle
fi
uvx --offline meson setup -Dlibseat-logind=disabled -Dlibseat-seatd=enabled \
	-Dlibseat-builtin=disabled -Dserver=enabled -Dexamples=disabled \
	-Dman-pages=disabled -Ddefaultpath="$PWD/.work/tmp/oracle.sock" \
	.work/tmp/oracle .work/tmp/reference
ninja -C .work/tmp/oracle seatd
uvx --offline meson setup --wipe .work/tmp/client-oracle oracle
ninja -C .work/tmp/client-oracle
ninja -C .work/tmp/client-oracle test
