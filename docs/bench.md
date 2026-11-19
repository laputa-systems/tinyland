# Performance workload driver

Build the pinned Alpine image with `make test-image` after changing dependencies
or the Dockerfile. The driver mounts the current checkout and builds offline
with the image's nightly and kache. It uses an anonymous Linux scratch volume
for sockets and Docker's `--rm --init` for cleanup. It checks Docker, the image,
git and 1 GiB of disk before any workload; it cannot verify image freshness,
so rebuild after changes to image inputs. The image ID and checkout state are
recorded. Its 4 CPU / 8 GiB container limit does not reserve a quiet host.

```
make bench-list
make bench-check BENCH_ARGS='--tier change --area seatd'
make bench BENCH_ARGS='--tier change --name seatd-switch --dry-run'
make bench BENCH_ARGS='--tier edit'
make bench BENCH_ARGS='--name seatd-switch'
```

`--tier edit|change|scenario|full` sets a ceiling (default change); repeat
`--name`, `--area`, or `--skip` to select workload names, areas, or exclusions.
Name and area filters intersect; unknown and empty selections fail. Every
exclusion is shown. `--tier full` requires `--full`; no automated Full workload
exists, and Full is manual-only. The Scenario idle workload is excluded from
this driver because it needs the host gate before and after the measurement;
follow [seatd's idle procedure](../seatd/doc/validation.md#idle-timing) on a
qualified host. Real devices, VM and downstream workloads remain manual.

Edit runs the driver's self-test in the image (estimated under one second after
container startup). Change runs one seatd switch storm, estimated under one
minute plus an offline debug build. It asserts 60 ordered transitions, FD
recovery and no more than 1 MiB daemon RSS growth after cleanup. It is not a
throughput or latency benchmark. Each run has `.work/bench/<UTC>-*/summary.txt`
and per-workload logs; durations include setup and build. No baseline or host
gate exists for these runs: timing is inconclusive and completion is not a
performance verdict. Instrumented profiling must be separate from timing.
