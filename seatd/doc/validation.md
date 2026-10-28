# Validate the Rust daemon

Run local checks with the repository's pinned nightly and locked offline
dependencies. These commands do not qualify real devices or VT behavior.

## Run the normal checks

Run the test suite and strict Clippy check:

```sh
cargo test --offline --locked
cargo clippy --offline --locked --all-targets -- -D warnings
```

Build both supported musl targets when their Rust targets and the Linux
musl linker are installed:

```sh
cargo build --offline --locked --target x86_64-unknown-linux-musl
cargo build --offline --locked --target aarch64-unknown-linux-musl
```

Rust compilation uses the `kache` rustc wrapper. Linux musl links use `mold`.
The required target installations are external prerequisites; a command that
cannot find a target or linker does not establish a successful target build.

## Compare with the C oracle

The retained C sources under `oracle/` come from upstream revision `bc0e96f`.
They provide an unmodified C libseat seatd client for compatibility checks.
The reference daemon is rebuilt from the original revision in Git history. They are test inputs, not a runtime dependency of the
Rust daemon.

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
cargo test --offline --locked --test storm storm_idle_parity_with_oracle -- --ignored --exact --nocapture
perf-gate-ready -1
```

If either readiness check reports a busy host, discard the measurement. The
gate is deferred until both checks pass. A deferred gate is not a pass, and
VT-disabled timing does not establish VT behavior. Do not report performance
parity until the valid measurement is available.

## Qualification limits

The following environment proofs remain pending:

- N1: real VT switching, release and acquire, and console restoration in a
  disposable VM.
- N2: device open, revocation, and reopen against real DRM and input devices
  in a disposable VM.
- N3: compositor startup and operation against the Rust daemon in that VM.

Do not run qualification against host DRM, input, or VT devices. Unit tests,
test doubles, and C differential checks do not substitute for N1–N3. Until
those proofs pass, do not describe the daemon as production ready or claim
actual performance parity.

The recorded R4 history includes one unidentified test failure during
contaminated reruns. It remains unclassified. The multi-client disconnect
crash reproduced during R7 was fixed; that fix does not resolve the unknown
R4 report.
