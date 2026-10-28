# Differences from the C daemon

The reference is fork revision `bc0e96f`. The Linux Rust daemon preserves
its native-endian socket protocol. The raw corpus compares both daemons
and the checked-in `harness/protocol.golden`. The libseat differential
uses the reference's unmodified seatd backend, without logind or builtin support.

| Surface | Rust behavior | Reference behavior and reason for the difference |
|---|---|---|
| Socket location | `SEATD_SOCK` overrides `/run/seatd.sock`. | The C daemon uses its compiled socket path. The override supports isolated daemon instances. C libseat already uses this environment variable. |
| Opened device identity | A path whose device and inode differ from the opened descriptor fails with `EACCES`. | C resolves the path and opens with `O_NOFOLLOW`, without comparing the opened identity. Rust adds a post-open check against replacement races. |
| Opened device type | A descriptor that is not a character device fails with `ENODEV`. | C classifies by canonical path prefix. Rust also checks the opened descriptor type. |
| Platform | Linux only. | C also includes FreeBSD and NetBSD support. Those platforms are outside this port's scope. |
| VT qualification | VT-bound mode is experimental. | Local tests prove the transition model and ioctl adapter failures. Real VT switching and restoration still require a disposable VM. |
| Distribution | The foreground Rust daemon replaces C seatd. | The C library and its reference inputs remain test-only. There is no Rust seatd-launch or embedded daemon. |
| Logging | Rust includes level, source file, and line. | C includes its own source and timing format. Logs are not wire-protocol goldens. |

Symlinks remain compatible with the reference: resolve the path first, classify
the canonical prefix, then open with `O_NOFOLLOW`. The extra identity and type
checks reject only paths whose opened descriptors fail those checks. Tests use
ordinary scratch files and `/dev/null`; real DRM, evdev, and hidraw qualification
remains N2 in `plan.md`.

The corpus covers ping, open and close, invalid device requests, stale IDs,
invalid switches, disable acknowledgement, switch event ordering, malformed
frames, and repeated opens. Transport tests cover fragmented and coalesced
frames and descriptor ownership. Successful device transfer, kernel revocation,
real VT operation, and compositor behavior require N1–N3. Corpus parity does
not establish those hardware contracts.

The R4 commit records an unidentified single-test failure during contaminated
reruns. It remains unclassified; no particular fix is claimed for that report.
The reproduced multi-client disconnect crash was fixed separately in R7.
