# Validate the daemon

Run from the workspace root with the pinned nightly. Local tests exercise
protocol, descriptor ownership, admission, transitions, shutdown, and resource
bounds. They do not qualify real devices or VT behavior.

```sh
cargo test --offline --locked -p seatd
cargo test --offline --locked -p seatd-protocol
cargo clippy --offline --locked -p seatd -p seatd-protocol --all-targets -- -D warnings
```

On Linux, `make test` runs all workspace tests with the pinned Alpine toolchain
and XKB data. The current workspace passes 173 tests with two deliberate
ignores. Component test success does not establish device or kernel behavior.

## Wire protocol

The frozen [golden transcript](../harness/protocol.golden) pins framing,
opcodes, errors, and the handoff sequence. Run it with:

```sh
cargo test --offline --locked -p seatd --test differential
```

The transcript was compared with the C reference daemon. Raw socketpair tests
exercise SCM_RIGHTS transfer, unexpected descriptors, truncation, retries,
EOF, and exactly-once descriptor ownership. Compatibility with every external
client is not implied by the finite transcript.

## Device qualification

Run the [VM harness](../harness/VM.md) with the release binary under test.
Transition mode checks VT release and acquire, device revocation and reopening,
DRM master handoff, and restoration after seat close. Shutdown mode checks
restoration after SIGTERM while a client remains connected. Compositor mode
checks pixels and keyboard input through a prepared compositor and client.

Earlier virtual-device results do not qualify the merged workspace, other
kernels, physical devices, hidraw revocation, or ARM hardware. An older test
run reported an unidentified failure that remains unclassified.

## Miri

Miri covers pure buffer and codec logic in `seatd-protocol`. Run its selected
pure tests with the pinned toolchain after installing the Miri component.
Miri does not validate sendmsg, recvmsg, SCM_RIGHTS transfer, kernel EOF,
or kernel descriptor ownership. Those require real socket and guest tests.

## Idle timing

The normal suite ignores `storm_idle_stays_quiet`. Run it only when
`perf-gate-ready` succeeds before the measurement and with `-1` afterward:

```sh
perf-gate-ready
cargo test --offline --locked --release -p seatd --test storm storm_idle_stays_quiet -- --ignored --exact --nocapture
perf-gate-ready -1
```

Discard a sample when either readiness check fails. An idle result establishes
no throughput, startup latency, or VT behavior. Descriptor, byte-buffer, and
memory bounds remain separate functional checks.
