#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
mode=run tier=change names= areas= skips= full=no
while [ "$#" -gt 0 ]; do
    case "$1" in
        --list) mode=list;; --check) mode=check;; --dry-run) mode=dry;;
        --full) full=yes;;
        --tier|--name|--area|--skip)
            key=$1; shift; [ "$#" -gt 0 ] || { echo "missing value for $key" >&2; exit 2; }
            case "$key" in
                --tier) tier=$1;; --name) names="$names $1";; --area) areas="$areas $1";; --skip) skips="$skips $1";;
            esac;;
        *) echo "unknown option: $1" >&2; exit 2;;
    esac
    shift
done
case "$tier" in edit) ceiling=1;; change) ceiling=2;; scenario) ceiling=3;; full) ceiling=4;; *) echo "unknown tier: $tier" >&2; exit 2;; esac
if [ "$mode" = list ]; then
    echo 'driver-selftest | Edit | driver | selection and preflight | estimated <1s | image shell'
    echo 'seatd-switch | Change | seatd | 60 session switches, RSS and FD recovery | estimated <1 min plus build | Linux image, /proc'
    echo 'seatd-idle | Scenario | seatd | idle CPU and wakeups | manual external host gate; see seatd/doc/validation.md'
    echo 'Full | manual only | external device and downstream workloads; no automated Full workload'
    exit 0
fi
for n in $names $skips; do case "$n" in driver-selftest|seatd-switch|seatd-idle) :;; *) echo "unknown workload: $n" >&2; exit 2;; esac; done
for a in $areas; do case "$a" in driver|seatd) :;; *) echo "unknown area: $a" >&2; exit 2;; esac; done
selected= exclusions=
for row in 'driver-selftest:driver:1' 'seatd-switch:seatd:2' 'seatd-idle:seatd:3'; do
    name=${row%%:*}; rest=${row#*:}; area=${rest%%:*}; level=${rest##*:}; reason=
    [ "$level" -le "$ceiling" ] || reason='above tier ceiling'
    [ -z "$names" ] || case " $names " in *" $name "*) :;; *) reason='name filter';; esac
    [ -z "$areas" ] || case " $areas " in *" $area "*) :;; *) reason='area filter';; esac
    case " $skips " in *" $name "*) reason='skipped';; esac
    if [ "$name" = seatd-idle ] && [ -z "$reason" ]; then reason='external perf-gate-ready required; run documented manual command'; fi
    if [ -n "$reason" ]; then echo "excluded $name: $reason"; exclusions="${exclusions}excluded $name: $reason
"; else selected="$selected $name"; fi
done
[ -n "$selected" ] || { echo 'selection is empty' >&2; exit 2; }
if [ "$tier" = full ] && [ "$full" != yes ]; then echo 'Full requires explicit --full' >&2; exit 2; fi
command -v docker >/dev/null 2>&1 || { echo "docker missing; install/start Docker for:$selected" >&2; exit 1; }
docker info >/dev/null 2>&1 || { echo "Docker unavailable; start Docker for:$selected" >&2; exit 1; }
docker image inspect tinyland-tests:local >/dev/null 2>&1 || { echo "image missing; run make test-image for:$selected" >&2; exit 1; }
command -v git >/dev/null 2>&1 || { echo "git missing; install git for:$selected" >&2; exit 1; }
command -v df >/dev/null 2>&1 || { echo "df missing; install df for:$selected" >&2; exit 1; }
avail=$(df -Pk . | awk 'NR==2 {print $4}')
[ "${avail:-0}" -ge 1048576 ] || { echo "less than 1 GiB free; free disk space for:$selected" >&2; exit 1; }
echo 'Host readiness: not gated; timing is inconclusive. Image dependencies come from the pinned Dockerfile; rebuild with make test-image after dependency changes.'
[ "$mode" != check ] || exit 0
for name in $selected; do
    case "$name" in
        driver-selftest) command='sh scripts/bench-selftest.sh';;
        seatd-switch) command='cargo test --offline --locked -p seatd --test storm storm_switch_storm_no_growth -- --exact --nocapture';;
    esac
    echo "$name: docker run --rm --init --cpus 4 --memory 8g --memory-swap 8g -v <checkout>:/src -v <cache>:/root/.cache/kache -v /src/seatd/.work/tmp tinyland-tests:local sh -c '$command'"
done
[ "$mode" != dry ] || exit 0
mkdir -p .work/bench .work/kache-cache
run=$(mktemp -d ".work/bench/$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX")
summary=$run/summary.txt
{ echo "commit: $(git rev-parse HEAD)"; echo "dirty: $(git status --porcelain | wc -l | tr -d ' ') paths"; echo "image: $(docker image inspect --format '{{.Id}}' tinyland-tests:local)"; echo "host: $(uname -a)"; echo "selection:$selected"; printf "%s" "$exclusions"; echo 'timing: inconclusive; no host gate or baseline; completion is not a performance verdict'; } > "$summary"
status=0
for name in $selected; do
    case "$name" in driver-selftest) command='sh scripts/bench-selftest.sh';; seatd-switch) command='cargo test --offline --locked -p seatd --test storm storm_switch_storm_no_growth -- --exact --nocapture';; esac
    start=$(date +%s)
    if docker run --rm --init --cpus 4 --memory 8g --memory-swap 8g -v "$(pwd):/src" -v "$(pwd)/.work/kache-cache:/root/.cache/kache" -v /src/seatd/.work/tmp tinyland-tests:local sh -c "$command" > "$run/$name.log" 2>&1; then rc=0; else rc=$?; status=1; fi
    echo "$name: exit $rc, $(( $(date +%s) - start ))s; timing inconclusive; log $run/$name.log" | tee -a "$summary"
done
echo "summary: $summary"
exit "$status"
