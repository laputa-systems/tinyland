# Verify the compositor

Run commands from the workspace root unless a command changes directory.
The pinned nightly is in `rust-toolchain.toml`. Linux musl links use mold;
Rust builds use the kache wrapper.

## Component tests

On Linux, run the workspace tests in the pinned Alpine image:

```sh
make test
```

For local tests, set `XKB_CONFIG_ROOT` to xkeyboard-config 2.43-r0 data:

```sh
cargo test --locked -p tinyland
cargo test --locked -p seatd-protocol
```

The tests exercise protocol errors, shared-memory validation and ownership,
commit transactions, focus restoration, pointer and popup behavior, keyboard
translation, and broker failure. The optional `oracle` feature compares
keyboard behavior with libxkbcommon. That feature requires the oracle library;
it is not a runtime dependency of the compositor.

The merged workspace passes 173 tests on the host and in the pinned Alpine
musl image. The idle timing test and embedded-keymap regeneration are
intentionally ignored. Restoring the broken toplevel retirement ordering
makes both window-close regressions fail. These results cover component
behavior, not a particular kernel or device configuration.

## Disposable QEMU guest

Prepare the pinned Alpine x86_64 guest and build both binaries:

```sh
/bin/bash tinyland/harness/build-guest.sh
cd tinyland
/bin/bash harness/seat-proof.sh
```

The seat proof runs the daemon separately, denies direct device access to the
session user, and checks brokered DRM and input, visible client pixels, a
post-input marker, and confirmed presentation. The harness owns and reaps its
QEMU process. It never treats a timeout or uncertain input outcome as success.

Additional retained harnesses cover smoke pixels, page-flip presentation,
atomic and legacy modeset parity, and deliberate guest failures. Requalify
them on the exact release binaries and guest inputs under test. Earlier VM
results do not qualify the current merged workspace. Use virtual devices for
these proofs rather than host console, GPU, or input devices.

## Wayland conformance

The headless server is available with the `headless` feature. The WLCS adapter
and its pinned test runner are under `tinyland/harness/wlcs/`:

```sh
/bin/sh tinyland/harness/wlcs/run.sh --sweep
```

Inspect each reported result. Passing selected cases does not establish full
protocol conformance. The adapter does not exercise kernel scanout or input.

## Dependency audit

Build the final x86_64 musl release and inspect the linked binary:

```sh
cargo build --locked --release --target x86_64-unknown-linux-musl -p tinyland
make -C tinyland audit
```

The audit rejects forbidden compositor libraries, runtime libxkbcommon,
dynamic dependencies in the static build, and unsafe compositor source.
The release build, device proof, binary size, startup, and idle measurements
remain separate checks. Record their actual inputs and results when run.
