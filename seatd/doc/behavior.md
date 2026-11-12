# Behavior rules

These rules supplement the [runtime contract](runtime.md). Device and VT
claims require qualification against the actual kernel and binary.

- Opened device identity: a path whose device and inode differ from the
  opened descriptor fails with `EACCES`. The check runs after opening
  with `O_NOFOLLOW`. It compares the pathname's device and inode with the
  opened descriptor. The checks are separate filesystem operations.
- Opened device type: a descriptor that is not a character device
  fails with `ENODEV`. Device class comes from the canonical path prefix;
  descriptor identity and character-device checks do not establish the
  kernel subsystem. The device namespace must remain administrator-controlled.
- Symlinks: resolve the path, classify its canonical prefix, then open with
  `O_NOFOLLOW` and validate the descriptor. See `src/policy.rs` and
  `src/device.rs` for the accepted classes and checks.
- Shutdown socket cleanup: remove the path only if its device and inode
  match the socket captured at startup, so a replacement daemon's
  socket survives. The directory must remain administrator-controlled.
- VT setup failure: capture prior mode values and attempt rollback on
  setup failure, then disconnect the opening client. Rollback is best
  effort and does not prove restoration of kernel VT ownership. Use VM
  qualification to check successful switching and graceful restoration.
- Logging: Rust includes level, source file, and line. Logs are not
  wire-protocol goldens.

The protocol corpus and transport checks are described in
[validation](validation.md). They do not establish kernel revocation,
real VT operation, or compositor behavior.
