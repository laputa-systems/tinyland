# CI test failures on 2026-11-20

At base revision `35bd8809ed341cf40da8f0a513a38d16fdea1c9e`, the latest 15 main-branch runs included three failures:

- [44747942428](https://github.com/laputa-systems/tinyland/actions/runs/44747942428): `admit::tests::refuse_policy_keeps_stale_socket` failed at `seatd/src/admit.rs:368` because `probe_live` returned true after the listener was dropped.
- [44641731680](https://github.com/laputa-systems/tinyland/actions/runs/44641731680): `shutdown_reconnect_storm_leaks_no_fds` failed at `seatd/harness/adversarial.rs:1014`, observing six daemon descriptors against a baseline of five.
- [44640532171](https://github.com/laputa-systems/tinyland/actions/runs/44640532171): image construction failed before tests. `cargo fetch --locked` received repeated HTTP 502 responses from `crates.17166969.xyz` and exited 101. The Docker build and `make test` then failed. The registry replacement is configured in `.cargo/config.toml:7-14` and the fetch runs in `Dockerfile.test:30`. The logs do not establish why the registry returned 502.

The Alpine test image matched the checked-out `seatd/src/admit.rs` and `seatd/harness/adversarial.rs` by SHA-256. With four CPU burners and the full seatd library test suite, the exact stale-socket assertion reproduced on iteration 74. A prior run failed the same assertion in `stale_socket_is_reclaimed` on iteration 16. The listener uses `SOCK_CLOEXEC`, and a concurrent library test launches a child process. Inheritance between process creation and exec could keep the listener alive briefly, but the experiment did not observe the descriptor in the child and does not prove that cause.

The reconnect test passed 50 iterations under the same load after creating its required `seatd/.work/tmp` directory inside the container. One earlier attempt failed at the daemon socket startup assertion because that directory was absent. The original descriptor-count failure remains unexplained. `wait_for_fds` and the subsequent assertion sample the daemon separately, so a pending connection accepted between samples is a possible race, not an established cause.

No test or code fix was made. Both failing tests are under `seatd/`, outside the assigned writable paths. Real devices and VT behavior were not tested.
