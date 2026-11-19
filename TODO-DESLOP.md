Prefix: DSLP
Next: 12

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier B). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-5, work the open P1 and P2 findings, then fill in `## Report` below.

## Report

The DSLP-1 base is `109c6781ec88f92d6405d58ee15934d4e74b3cdc`.
Ground through caps landed in #7–#13. The Report review covers that base
through `1f2770a`. All open tickets are P3.

| Measure | Before | After and limits |
| --- | --- | --- |
| Rust source and test lines | 12,840 source, 3,816 test | 12,964 source, 3,814 test. Tracked `.rs` lines; test and harness directories count as test, inline tests as source. |
| Warm `make test` | 17.41 s | 22.54 s at Tests. Both observations were on a busy host, not valid performance gates. |
| Coverage | 77.61% lines, 58.37% branches | 77.76% lines, 58.56% branches. Ephemeral Alpine ARM64 run includes harnesses. |
| Mutation | Region-object guard: 1 caught, 0 missed, 0 timeout, 0 unviable | Same result. One guard, 32 runner-seconds total; no wider survivor claim. Three boundary fault controls protected region object count, rectangle count and shm backing size. |
| Escape hatches | unsafe 60, allow 6, unwrap/expect 44, casts 276 | 60, 6, 44, 272 respectively. Production lexical approximation, including possible comments and aliases. |
| Dead code | Unread daemon test field and runtime keyboard lookup | Field removed and lookup restricted to tests; two net harness lines removed, no dependencies removed. 109 private-module declarations narrowed. |
| Findings | Diagnose filed one P2 and two P3 | DSLP-6 P2 fixed with positioner, pointer and keyboard caps and reuse tests. DSLP-7 and DSLP-8 remain P3. Bench filed DSLP-9 and DSLP-10 P3. Report filed DSLP-11 P3. |
| Performance | No qualified comparison | Deferred: host busy. #12 supplies a workload entrypoint, not a throughput gate. |
| Escaped regressions | Not measured | No demonstrated regression in this review. DSLP-11 records retained seat state and an unproven cap bypass. Device, VT, VM, oracle and downstream behavior remain unqualified. |

The campaign fixed one reproduced resource-exhaustion finding. Tests added
three fault-proven boundary behaviors and cover three input-object caps,
capacity reuse and disconnect. The mutation result is not a count of defects.

| Step | Finding and cost |
| --- | --- |
| Ground #7 | Pinned Alpine test and strict lint entrypoints, Linux kache and mold. Cold test 49.69 s, warm 17.41 s, lint 54.64 s including image rebuild. Host busy; CI minutes not retained. |
| Diagnose #8 | Reproduced DSLP-6, DSLP-7 and DSLP-8 across untrusted input and admission paths. Wall time and CI minutes not retained. |
| Types #9 | No strict candidate met the noise and witness rule. Four clipping casts became checked conversions with a boundary witness. Wall time and CI minutes not retained. |
| Tests #10 | Three boundary tests and fault controls; coverage and bounded mutation results above. Candidate warm test 22.54 s, lint 18.07 s on a busy host; CI minutes not retained. [Contract map and method](docs/deslop-tests.md). |
| Dead code #11 | Rust dead-code checks and cargo-machete found no remaining actionable dependency removal. Default and all features, all targets checked. Wall time and CI minutes not retained. |
| Bench #12 | Added Edit and Change driver workloads. Frame throughput, per-client memory, seatd response and startup lack a qualified baseline; DSLP-9 and DSLP-10 track them. [Workloads and limits](docs/bench.md). Wall time and CI minutes not retained. |
| Caps #13 | Fixed DSLP-6 with limits and destroy/reuse checks. Wall time and CI minutes not retained. |
| Report | Reviewed the cumulative diff, emphasizing caps, destruction, clipping and image/CI entrypoints. Filed DSLP-11; no concrete failing check justified a code fix. Review wall time and CI minutes not retained. |

Method feedback: test resource bounds across parent destruction and rebinding,
not just child destruction and disconnect. The narrow mutation trial found no
survivor and cannot assess seat lifetime. Strict lint candidates found no
setting with both a useful witness and acceptable noise. The bench driver
records inconclusive timing because it cannot establish host readiness or image
freshness. No owner walk-back was identified.

Shared-file gaps: the crate mirror lacks `cargo-mutants` and `cargo-llvm-cov`,
so no retained coverage or mutation lane was qualified. Mirror both tools and
qualify a bounded recipe with negative controls before adding a lane. The
region trial and coverage report used ephemeral tools, not a CI guarantee.

## Tickets









### DSLP-7 · P3 · seatd spins and refuses new clients when accept fails

- **Area:** Reliability
- **Where:** seatd/src/loop.rs:137-146 (handle_listen), seatd/src/loop.rs:274-292 (listen revents), seatd/src/admit.rs:300-317 (accept_client)
- **Problem:** accept_client returns None when accept4 fails, after logging; handle_listen then leaves the pending connection queued and sets neither `dirty` nor any backoff. The listen descriptor stays readable, so poll returns immediately and the daemon spends a full core on failing accepts while admitting nobody. Reproduced in the pinned Alpine image with the daemon at a 24-descriptor limit (`ulimit -n 24`), holding 36 client connections: 747 connects were refused over 4 s, daemon CPU was 0.99 CPU-seconds per wall second, and a fresh client's connect was refused; after the held sockets closed, CPU returned to 0.00 and a fresh client received its PONG. At the default log level each iteration also writes one line, so the spin floods the log. The trigger is the daemon's descriptor budget, so the shipped 1024-descriptor default needs about 1020 held connections; any process that can connect, which the README names as the admission boundary, can reach it. Already-accepted clients keep being serviced, so impact is refusal of new sessions plus CPU and log burn until descriptors free.
- **Recommendation:** On accept failure, stop selecting the listen descriptor, or back off to a deadline, until a client slot frees, and bound the error log rate. Verify with a socket test that the daemon does not spin and that admission recovers when descriptors free.
- **Fix with:** error-model
- **Effort:** S


### DSLP-8 · P3 · Remote damage messages can materialize far more pixels than one frame

- **Area:** Data
- **Where:** remote-protocol/src/lib.rs:135-211 (read_compositor tags 3 and 4), remote-protocol/src/lib.rs:314-323 (damage_header), remote-protocol/src/lib.rs:276-312 (write_tiled_damage)
- **Problem:** read_compositor bounds each damage rectangle to the output Size and the rectangle count to MAX_DAMAGE_RECTS (64), but never bounds the sum of the rectangle areas. Overlapping and repeated rectangles are accepted, and a tiled rectangle (tag 4) encodes a solid 64x64 tile in five bytes, so a small message materializes a large Vec per rectangle. Reproduced by feeding hand-built messages to read_compositor for Size 4096x4096: 1,291,269 wire bytes carrying 63 full-output solid-tile rectangles returned 4,227,858,432 damage bytes (4.03 GiB, about 3270x the input) and raised the reader's resident set by 4032 MiB; eight such rectangles needed 163,989 wire bytes for 512 MiB. That contradicts the module's own intent that a damage message is bounded by one output, with excess rectangles collapsed to a bounding box. A hostile or compromised compositor can exhaust the reading host's memory; a reader-side bound also protects other future senders.
- **Recommendation:** Reject a damage message whose rectangle areas sum past size.pixel_len(), or require rectangles to be disjoint, in both tag 3 and tag 4; keep the writers' current invariants. Add tests for overlapping and repeated rectangles in both encodings.
- **Fix with:** os-boundaries
- **Effort:** S


### DSLP-9 · P3 · Establish representative compositor performance evidence

- **Area:** Performance
- **Where:** docs/CAMPAIGN-DAMAGE.md; tinyland/src/server/tests/damage.rs:88
- **Problem:** The 300-step headless damage differential checks pixels and sparse updates but does not measure shipped frame throughput, latency, CPU, allocations or bytes. The headed bctl comparison is historical and lacks a reproducible pinned baseline and retained samples here. No current evidence detects a performance regression or attributes it across compositor, guest and viewer.
- **Recommendation:** Extend the existing headed damage campaign with pinned candidate and reference, completed-frame and pixel checks, interleaved gated samples, stage CPU and byte counters, and separate sampling or tracing for attribution. Qualify actual devices and downstream separately.
- **Fix with:** bench-pyramid
- **Effort:** L


### DSLP-10 · P3 · Measure client density, seatd responsiveness and startup

- **Area:** Performance
- **Where:** seatd/harness/storm.rs:201; seatd/doc/validation.md:55; tinyland/docs/verification.md:92
- **Problem:** The switch storm checks 60 completed transitions and post-cleanup daemon RSS and descriptors, not per-client retained or peak memory, latency or throughput. The ignored idle test samples only daemon CPU ticks and voluntary switches over five seconds. Neither component has a startup measurement or an interleaved baseline; regressions in these outcomes can go unnoticed.
- **Recommendation:** Add representative client-density, request-response and startup journeys with completion checks, per-process and container memory scope, operation counts, gated interleaved samples and an accepted reference. Use separate CPU sampling, allocation and syscall tracing for attribution; document overhead and host validity.
- **Fix with:** bench-pyramid
- **Effort:** M


### DSLP-11 · P3 · Retire destroyed seats without losing input object accounting

- **Area:** Resource lifecycle
- **Where:** tinyland/src/server/dispatch_impl.rs:675-703, :717-740, :772-779, :796-831
- **Problem:** The seat destroyed callback does nothing, so State.seats retains seat records after release or disconnect. On seat release, input children may remain live and their vectors remain attached to the retained seat. The new pointer and keyboard limits are per seat, while seat admission counts only resources whose client lookup succeeds. Rebinding after release may therefore evade the intended per-client resource bound; repeated disconnects also retain seat records. The current disconnect test checks emptied input vectors but not removal of the seat. The rebind impact has not been reproduced.
- **Recommendation:** Exercise release, rebind and disconnect with live pointer and keyboard children, then account for their actual lifetimes and retire seat state without losing live input resources. Assert bounded retained state and continued input delivery.
- **Fix with:** lifecycle-concurrency
- **Effort:** M

