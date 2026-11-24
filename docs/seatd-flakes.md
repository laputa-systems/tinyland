# seatd intermittent test failures

All runs below use the repository's `tinyland-tests:local` Alpine image (aarch64, musl) with the worktree mounted at `/src`, `--cpus 4`, and four `yes > /dev/null` burners inside the container. Each iteration runs the whole test binary once.

## Findings

No production socket or descriptor lacks close-on-exec. Every `socket`, `accept4`, `open`, `openat`, `signalfd`, `pipe2` and `socketpair` in `seatd/src` already passes `SOCK_CLOEXEC`, `O_CLOEXEC` or `SFD_CLOEXEC`. The seatd daemon never spawns a process. The three defects are in the tests.

### 1. `stale_socket_is_reclaimed` and `refuse_policy_keeps_stale_socket` (`admit.rs`)

Cause: the unit tests share one process. `device::tests::device_teardown_reports_revocation_failure` re-executes the test binary with `Command::output()`. On musl, `fork()` copies every open descriptor, including a listener another test thread currently owns, and the copy lives until the child's `exec`. Close-on-exec does not help before `exec`. If a stale-socket test drops its listener while the child sits between `fork` and `exec` (long under CPU load), `probe_live` connects to the child's copy and the `!probe_live` assertion fails. Test socket paths are unique per process and tag, so there was no path sharing.

Evidence:

- Base, four burners, `--lib`: 1 failure in 30 iterations (`refuse_policy_keeps_stale_socket`, `admit.rs:368`) and 8 failures in 400 iterations (`stale_socket_is_reclaimed`, `admit.rs:353`). Both are the same `assert!(!probe_live(..))` shape.
- Deterministic reproduction, `test_gate::tests::child_launch_never_overlaps_a_listener_that_is_being_dropped`: a child held in `pre_exec` while a listener is dropped. With the gate bypassed it fails 3 of 3 runs (`a child launched during the scenario held the dropped listener`, exit 101). With the gate it passes 3 of 3.
- I did not capture the holding process in a real failing iteration (the `/proc` scan I added ran only in the `refuse` test and caught nothing in 400 iterations). The causal link between the deterministic case and the flake rests on the failure rate going from 8 in 400 to 0 in 1600 with the gate, not on a captured holder.

Fix (`4b5eac0`): `seatd/src/test_gate.rs` holds a read/write lock. The three tests that assert a dropped listener is dead hold it shared; the child launch holds it exclusively. No assertion was weakened and no timeout changed.

Result with the fix: `--lib`, four burners, 1600 iterations, 0 failures, exit 0 (400 base iterations had 8 failures, so 4N = 1600).

### 2. Adversarial tests failing with `ConnectionRefused` (not in the original report)

Cause: `spawn_with_env` waited for the socket path to exist, but `bind()` creates the path before `listen()`. Under load a client connects in between and gets `ECONNREFUSED`.

Evidence: base, `--test adversarial`, four burners: 11 failures in 60 iterations and 6 in 40, all `connect: Os { code: 111, kind: ConnectionRefused }` at `adversarial.rs:745`, spread over five tests.

Fix (`9a5c0ac`): wait for the readiness descriptor (`-n`), which the daemon writes after `listen()`.

### 3. `shutdown_reconnect_storm_leaks_no_fds`, 6 descriptors against a baseline of 5

Probable cause: the test closes its last clients and calls `wait_for_fds`, which returns as soon as the daemon's descriptor count is at or below the baseline. Connections still queued in the listen backlog (up to 16) are not descriptors yet, so the count already equals the baseline. The daemon then accepts them and the count transiently rises by one before it sees EOF, so the following `assert_eq!` samples 6. A slower daemon (CPU load) leaves more connections queued. This is a race in the test's sampling, not a daemon leak, and it is unrelated to close-on-exec: the count is of the daemon's own descriptors.

Evidence: the failure did not reproduce on base (20 iterations of the test alone and 60 of the whole suite, under load, passed it). The mechanism is shown deterministically instead: `shutdown_fd_count_settles_only_after_backlogged_connections_are_accepted` stops the daemon with `SIGSTOP`, queues 8 connections, and asserts that the settle helper does not return until the daemon resumes. With the helper reduced to the old `wait_for_fds` it fails 3 of 3 (`fd count settled while connections were still queued unaccepted`). With the fix it passes 3 of 3.

Fix (`9a5c0ac`): `settle_fds` round-trips a ping before waiting. Backlog order means every earlier connection has been accepted by the time the reply arrives, so the count can only fall afterwards.

This cause is established as a race the old test could lose, not as the cause of the one CI failure.

## Status

- Stale-socket failures: fixed and verified (1600 loaded iterations, 0 failures).
- Descriptor-count race: fixed, with a deterministic regression test. The base failure was never reproduced, so there is no loaded before/after count.
- `ConnectionRefused` failures: fixed, with 3 clean runs of the regression test and the base failure counts above. The 4N loaded run of the whole adversarial suite (160 iterations) was not completed: the campaign was wound down first, and the one attempt used the wrong binary (the loop script's last-executable heuristic picked `target/debug/seatd`). It is unproven under load.
- No `.github/` change was needed. `shutdown_reconnect_storm_leaks_no_fds` relies on `seatd/.work/tmp` existing; a missing directory makes the daemon-startup assertion fail (see the earlier report).

## Commands

```sh
export TMPDIR=$PWD/.work/tmp CARGO_BUILD_JOBS=4
docker run --rm --init --cpus 4 --memory 8g \
  -v $PWD:/src -v seatd-flake-target:/target tinyland-tests:local sh -c '
    cd /src && export CARGO_TARGET_DIR=/target
    bin=$(cargo test --offline --locked -p seatd --lib --no-run --message-format=json 2>/dev/null |
      sed -n "s/.*\"executable\":\"\([^\"]*\)\".*/\1/p" | tail -1)
    for i in 1 2 3 4; do yes > /dev/null & done
    n=0; f=0; while [ $n -lt 1600 ]; do n=$((n+1)); $bin >/dev/null 2>&1 || f=$((f+1)); done
    echo iterations=$n failures=$f; [ $f -eq 0 ]'
```

For the adversarial suite use `--test adversarial` and run the binary from `/src/seatd`. Select the `adversarial-*` executable explicitly.
