# seatd

This workspace contains a Linux Rust implementation of the seatd foreground
daemon. It serves the existing libseat seatd protocol over a Unix socket. The
daemon supports one seat, `seat0`.

## Run under a supervisor

Run seatd as a supervised foreground process. For a seat group named `seat`,
the following invocation disables VT binding:

```sh
SEATD_VTBOUND=0 seatd -g seat -l info
```

The daemon listens on `/run/seatd.sock` unless `SEATD_SOCK` names another path.
Socket access is the admission boundary. Every process that can connect can
request seat and supported-device operations. Seatd does not authenticate
users or manage login, power, or supervision policy. Read [runtime operation](doc/runtime.md)
before deployment.

## Protocol compatibility

The daemon supports opening and closing seats and devices, session handoff,
session switching, and ping. Its shared protocol crate owns wire constants,
framing, and descriptor transfer; client and server behavior still require
separate checks. See [runtime operation](doc/runtime.md) for limits and
[validation](doc/validation.md) for builds and compatibility evidence.

Use the [disposable VM harness](harness/VM.md) to qualify VT transitions,
device revocation, and restoration against the kernel and binary under test.

## License

The project retains the MIT license and attribution to Kenny Levinsen (2020).
