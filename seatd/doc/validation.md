# Validate the Rust daemon

Run local checks with the repository's pinned nightly and locked offline
dependencies. These commands do not qualify real devices or VT behavior.

## Run the normal checks

Build the oracle before the first test run with `sh harness/build_oracle.sh`.
The helper requires the original `bc0e96f` revision in local Git history.
It does not fetch missing history. Then run the tests and strict Clippy:

```sh
cargo test --offline --locked
cargo clippy --offline --locked --all-targets -- -D warnings
```

Build both supported musl targets when their Rust targets and the Linux
musl linker are installed:

```sh
export RUSTC_WRAPPER=kache
export RUSTFLAGS='-Clinker=mold -Clinker-flavor=ld'
cargo build --offline --locked --release --target x86_64-unknown-linux-musl
cargo build --offline --locked --release --target aarch64-unknown-linux-musl
```

Rust compilation uses the `kache` rustc wrapper. Linux musl links use `mold`.
The required target installations are external prerequisites; a command that
cannot find a target or linker does not establish a successful target build.

## Compare with the C oracle

The retained C sources under `oracle/` come from upstream revision `bc0e96f`.
They provide an unmodified C libseat seatd client for compatibility checks.
The reference daemon is rebuilt from the original revision in Git history.
These sources are test inputs, not runtime dependencies of the Rust daemon.

Build the oracle and run the client differential:

```sh
sh harness/build_oracle.sh
sh harness/libseat_diff.sh
```

The build helper archives the reference source under `.work/tmp/reference`,
then builds the reference daemon as `.work/tmp/oracle/seatd` and the client
scenario as `.work/tmp/client-oracle/simpletest`. The differential runs the
same client scenario against both daemons and compares normalized transcripts.
It checks client-visible behavior for that scenario. It does not qualify
device access, VT switching, or compositor integration.

The raw protocol corpus and its expected transcript are in
[`harness/protocol.golden`](../harness/protocol.golden). Corpus parity checks
framing and request behavior represented by those cases. It does not establish
kernel behavior for DRM, evdev, or hidraw.

## Run the idle timing gate

The idle parity case is ignored in the normal test run. Run it only when
`perf-gate-ready` reports an idle host. Check readiness immediately before and
after the measurement:

```sh
perf-gate-ready
cargo test --offline --locked --release --test storm storm_idle_parity_with_oracle -- --ignored --exact --nocapture
perf-gate-ready -1
```

If either readiness check reports a busy host, discard the measurement. The
gate is deferred until both checks pass. A deferred gate is not a pass, and
VT-disabled timing does not establish VT behavior. Do not report performance
parity until the valid measurement is available.

## Miri scope

Miri validates the pure buffer and codec logic only. Run it with the
pinned toolchain after installing the component once
(`rustup component add --toolchain nightly-2026-09-15 miri`):

```sh
cargo +nightly-2026-09-15 miri test --locked --offline --lib -- --exact \
  conn::tests::ring_wraparound_preserves_order \
  conn::tests::ring_counters_wrap_without_loss \
  conn::tests::ring_restore_recovers_frame_header \
  wire::tests::wire_opcode_values_match_c_header \
  wire::tests::wire_header_round_trip_native_endian \
  wire::tests::wire_header_explicit_bytes_not_struct_cast
```

All six pass under Miri. Everything else in `conn.rs` is kernel
behavior Miri cannot validate: `recvmsg`/`sendmsg` are foreign calls
Miri refuses to execute, SCM_RIGHTS transfer, `EINTR`/`EAGAIN` retry,
`MSG_CTRUNC` handling, EOF semantics, and FD ownership across the
kernel boundary are proven only by the real socketpair tests in the
normal suite and the disposable-guest qualification below.

## Re-qualification after the socket and lifecycle repairs

On 2026-10-28 at `62da737`, the repaired release binaries re-passed
the guest and client proofs. Build recipe: pinned nightly, locked
offline deps, `RUSTFLAGS="-Clinker=mold -Clinker-flavor=ld"` on top of
the container's `~/.cargo` kache wrapper (local deviation from the CI
recipe, which sets only the env flags; hashes below are for the local
recipe and are recorded as tested, not as canonical release hashes):

| Target | Size | SHA-256 |
|---|---:|---|
| x86_64-unknown-linux-musl | 692376 | `339e726d9e4607fc00c8acf974a706bb2d19e2938a98b57b4de13a844c88cfae` |
| aarch64-unknown-linux-musl | 671952 | `810e72a4670450355898428bd0965503224ebfab6ba31c7dad1ecf10d4826c00` |

Both are static (`DT_NEEDED` count 0), x86_64 static-pie. Evidence run
on the exact binaries above:

- KVM guest `transition` mode: release, acquire, device revocation and
  reopening, DRM master handoff, restoration after closing the seat —
  `VM_TRANSITION_OK`.
- KVM guest `shutdown` mode: restoration after SIGTERM with a client
  connected — `VM_SHUTDOWN_OK`.
- Unmodified C libseat client differential: pass against the x86_64
  binary natively and against the aarch64 binary through
  `qemu-aarch64` user mode — `LIBSEAT-DIFF-OK` both.
- `cargo test --offline --locked` green (57 lib tests including the new
  admission and revoke-transition pins), strict Clippy clean.

Compositor mode was not re-run (needs the Laputa package mirror, absent
here); N3 dwl/foot proof remains the earlier recorded run. Physical
devices, hidraw revoke, and ARM hardware stay out of scope as before.

## Qualification limits

N1–N3 passed on 2026-10-28 in an x86_64 KVM guest running Alpine
Linux 6.6.142-0-virt, using virtual GPU and keyboard devices only:

- N1: VT1→VT2→VT1 release/acquire, console restoration after seat close,
  and SIGTERM restoration while a seat client remained connected.
- N2: evdev returned `ENODEV` after revoke and worked after close/reopen.
  A competing DRM descriptor gained master while the seat was disabled and
  was refused master again after the original seat reacquired it.
- N3: dwl 0.9 with wlroots 0.20 and foot rendered a magenta terminal marker.
  QMP keyboard injection reached foot through libinput and Rust seatd.

The integrated harness reproduced all three modes. The compositor screenshot
was also visually inspected. Commands and cached-guest preparation are in
[the VM recipe](../harness/VM.md). No host DRM, input, or VT device was used.
This proof covers that guest configuration. Physical devices, hidraw revoke,
and ARM hardware remain outside these results. Unit tests and raw protocol
parity alone do not establish kernel device behavior.

The recorded R4 history includes one unidentified test failure during
contaminated reruns. It remains unclassified. The multi-client disconnect
crash reproduced during R7 was fixed; that fix does not resolve the unknown
R4 report.

## Local delivery results

On 2026-10-28, the pinned toolchain and locked dependencies passed 79 tests.
The normal suite ignores the idle timing case. That case subsequently passed
five release-mode samples with valid readiness checks. Strict Clippy passed
with warnings denied. Both retained C unit tests passed after
a fresh oracle rebuild.

Both musl release binaries built with `mold` and reported version `0.9.3`.
The same unmodified C libseat client scenario matched the reference daemon
against each binary. The aarch64 binary ran through `qemu-aarch64` user-mode
emulation. This does not qualify ARM hardware, devices, or real VT behavior.

| Target | Unstripped binary size | Client differential |
|---|---:|---|
| x86_64-unknown-linux-musl | 685080 bytes | Pass, native Linux |
| aarch64-unknown-linux-musl | 665416 bytes | Pass, QEMU user mode |

Pass a daemon executable as the first argument to `harness/libseat_diff.sh`
to check a release build. Without an argument, it checks `target/debug/seatd`.

The [CI workflow](../.github/workflows/ci.yml) runs the functional checks and
both target builds on a disposable Ubuntu runner. Checkout uses a full commit
pin. The compiler wrapper, uv, and Meson versions are pinned. The wrapper
archive is checked against its pinned digest. Dependency fetches use
`Cargo.lock`, and compilation runs offline afterward. CI leaves the idle
measurement deferred until a separate host-ready run.

Workflow YAML, embedded shell syntax, and the explicit musl target
configuration were checked locally. Hosted CI has not run: these changes
have not been pushed.

## Idle gate result

Five release-mode samples passed the default host-readiness check before
each run and the `-1` check immediately afterward. Each sample observed both
daemons over five seconds. Rust and the C reference both recorded zero CPU
ticks and zero voluntary context-switch increments in every sample: median
0, range 0–0 for each counter.

This passes the idle CPU/wakeup regression gate at the counters' resolution.
It establishes no throughput, startup-latency, or busy-workload speedup.
The byte, descriptor, and memory storm checks remain separate functional
resource gates. Earlier samples attempted while readiness checks failed were
discarded and are not part of these results.
