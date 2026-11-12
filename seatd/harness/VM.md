# Run daemon VM qualification

Use an x86_64 Linux host with KVM, QEMU, a static C toolchain, mkfs.ext4,
and Python 3. The harness uses virtual devices only. Run commands from the
workspace root and pass absolute input paths.

Build the static musl daemon and prepare the compositor's disposable guest:

```sh
cargo build --locked --release --target x86_64-unknown-linux-musl -p seatd
/bin/bash tinyland/harness/build-guest.sh
workspace=$PWD
```

Run transition and shutdown proofs against that guest:

```sh
for mode in transition shutdown; do
  /bin/bash seatd/harness/vm-qualification.sh "$mode" \
    "$workspace/tinyland/.work/guest-root" \
    "$workspace/tinyland/.work/vmlinuz-virt" \
    "$workspace/tinyland/.work/initramfs-virt" \
    "$workspace/target/x86_64-unknown-linux-musl/release/seatd"
done
```

Inspect `seatd/.work/vm-qualification/<mode>/console.log`. Transition mode
checks release, acquire, revocation, reopening, DRM master handoff, and
restoration after seat close. Shutdown mode checks restoration after SIGTERM
with a connected client. The harness compiles its raw-protocol probe and
reaps the owned QEMU process.

The optional `compositor` mode requires a prepared guest with compatible dwl,
wlroots, foot, eudev, and their runtime libraries. Supply that guest root with
the same absolute-path interface. It checks a rendered terminal marker and
keyboard input. Inspect its `screen.ppm` as well as the console result.

For Tinyland's unprivileged compositor and broker proof, run
`make -C tinyland proof`. Each result qualifies only the exact daemon, kernel,
client, and guest configuration used by that run.
