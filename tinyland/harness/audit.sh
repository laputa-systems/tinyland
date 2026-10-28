#!/bin/bash
set -euo pipefail
echo "== forbidden dep audit =="
cargo tree -e features 2>/dev/null | grep -Ei 'libwayland|libdrm|mesa|smithay|wlroots|gbm|egl' && { echo "FORBIDDEN DEP PRESENT"; exit 1; } || true
echo "no forbidden crate names in tree"
echo "== dynamic libs =="
if [ -f target/debug/tinyland ]; then
  ldd target/debug/tinyland || true
fi
echo "== unsafe inventory =="
grep -rn "unsafe" src/ || echo "no unsafe blocks"
