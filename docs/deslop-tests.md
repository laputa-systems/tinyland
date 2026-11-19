# Tests pass: critical contracts

The Tests pass at base 8af1c2f6 mapped the following contracts to the named
checks. These checks do not qualify real devices or actual VT transitions.

| Contract | Protecting tests |
| --- | --- |
| Wayland shm size, file and geometry | `shm::tests::{rejects_zero_size_pool,rejects_negative_offset,rejects_past_end,rejects_short_stride,rejects_huge_dimensions}`; `shm::lifetime_tests::{pool_rejects_a_backing_file_shorter_than_its_declared_size,grown_pool_accepts_buffers_in_the_extended_range}`; `server::tests::{invalid_stride_disconnects_client_instead_of_creating_buffer,committing_a_buffer_whose_file_was_truncated_reports_invalid_fd_on_the_buffer}` |
| Client resource limits | `server::tests::{region_object_limit_rejects_the_next_creation,region_rectangle_limit_rejects_add_and_subtract,per_client_memory_budget_rejects_without_harming_healthy_client,callback_bound_disconnects_offender_only}`; uncapped positioners and pointers remain DSLP-6. |
| Damage clipping | `damage::tests::output_rects_clip_unite_overlaps_and_fall_back_to_a_bound`; `server::tests::damage::{pixels_outside_posted_damage_keep_their_previous_contents,random_damage_moves_and_resizes_always_match_a_full_composition}`. |
| Seat and VT decisions | `session::tests::{session_vtbound_second_client_ebusy,session_open_device_needs_active,session_vt_release_and_acquire,session_revoke_failure_across_full_switch}`; `vt::tests::vt_setup_failure_restores_previous_modes`. |
| Seatd wire parsing and FD ownership | `adversarial::{wire_fragmented_header_reassembles,wire_coalesced_frames_parse_in_order,wire_fd_round_trip_exactly_once,device_overlong_path_is_protocol_error}`; `session::tests::session_ping_pong_and_bad_size`; `differential::transcripts_match_golden`. |
| Remote decoder bounds | `tests::{frames_require_exact_size_and_direction,damage_carries_only_its_rectangles_and_rejects_bad_geometry,rejects_invalid_lengths_values_and_utf8}` in remote-protocol. Aggregate damage remains DSLP-8. |
| Shutdown and restore | `adversarial::{shutdown_graceful_restart_serves,shutdown_preserves_replacement_socket,shutdown_reconnect_storm_leaks_no_fds}`; `shutdown::tests::termination_signal_reaches_shutdown_owner`; `seat::tests::shutdown_cancels_an_unanswered_request`; `vt::tests::vt_setup_failure_restores_previous_modes`. |

The test pyramid uses named Rust tests for Edit, the pinned image workspace
suite and strict Clippy for Change, the daemon adversarial and finite golden
transcript harnesses as scenarios within that suite, and separate manual device,
VT, VM, oracle and downstream qualification. The baseline suite reported 267
passing tests and two ignored; the candidate adds three tests. No test was cut.
The existing ground observation was 17.41s warm; this pass observed 22.54s
candidate and 18.07s lint on a busy host. `perf-gate-ready` returned 75, so these
are not valid performance-gate measurements. The `region_` selection executed
8 tests in 0.13s, excluding compilation.

An ephemeral cargo-llvm-cov 0.9.1 run on Alpine ARM64 reported, including harness
files, baseline 77.61% lines and 58.37% branches, candidate 77.76% lines and
58.56% branches. Coverage is a report, not an assertion-strength measure. The
instrumented results and tests on real hardware are separate. The crate mirror
returned 404 for both cargo-llvm-cov and cargo-mutants. A persistent coverage or
mutation recipe would need an unmirrored tool dependency; no CI lane is added.

Cargo-mutants 27.1.0 ran the region-object count guard at
`tinyland/src/server/dispatch_impl.rs:62`, with `--baseline run --gitignore true
--timeout 30 --build-timeout 180 --jobs 1 -p tinyland -f
tinyland/src/server/dispatch_impl.rs --re 'dispatch_impl.rs:62:' -- --lib` in
the pinned Alpine image. Before and after each took 16s: one caught, zero
missed, unviable or timed out, 32s aggregate runner time. The only mutation
changed `>=` to `<`; the earlier suite failed because ordinary region creation
was rejected, and the new boundary test also failed. There are no survivors in
this narrow scope; no whole-package mutation claim is made.

Three independent manual fault controls were run in the pinned image: change
`regions_for_client >= MAX_REGIONS_PER_CLIENT` to `>`; change both rectangle
`>= MAX_REGION_RECTS` guards to `>`; and permit a backing file one byte too
short. Each corresponding new test failed on its own assertion (exit 101), and
the unchanged candidate suite passed. The faults were mounted as disposable
copies and never applied to production files. No production behavior changed.
DSLP-4 remains open pending a qualified mirrored coverage recipe and a valid
warm cost gate; DSLP-6 through DSLP-8 are unchanged.
