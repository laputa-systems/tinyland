#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
cat > "$tmp/docker" <<'MOCK'
#!/bin/sh
case "$1" in info|image) exit "${MOCK_FAIL:-0}";; run) echo ran >> "$MOCK_TRACE"; exit "${MOCK_RUN_FAIL:-0}";; esac
MOCK
chmod +x "$tmp/docker"
export PATH="$tmp:$PATH" MOCK_TRACE="$tmp/trace"
script=scripts/bench.sh
sh "$script" --tier edit --dry-run > "$tmp/out"
grep -q 'driver-selftest: docker run' "$tmp/out"
grep -q 'excluded seatd-switch: above tier ceiling' "$tmp/out"
! grep -q '^seatd-switch: docker run' "$tmp/out"
sh "$script" --name seatd-switch --area seatd --dry-run > "$tmp/out"
grep -q '^seatd-switch: docker run' "$tmp/out"
if sh "$script" --name seatd-switch --area driver --dry-run > /dev/null 2>&1; then exit 1; fi
if sh "$script" --name unknown --dry-run > /dev/null 2>&1; then exit 1; fi
if sh "$script" --area unknown --dry-run > /dev/null 2>&1; then exit 1; fi
if sh "$script" --tier edit --skip driver-selftest --dry-run > /dev/null 2>&1; then exit 1; fi
if sh "$script" --tier full --dry-run > /dev/null 2>&1; then exit 1; fi
if MOCK_FAIL=1 sh "$script" --tier edit > /dev/null 2>&1; then exit 1; fi
[ ! -e "$MOCK_TRACE" ]
if MOCK_RUN_FAIL=7 sh "$script" --tier edit > /dev/null 2>&1; then exit 1; fi
[ -s "$MOCK_TRACE" ]
echo 'bench self-test: selection, exclusions, dry run and failed preflight passed'
