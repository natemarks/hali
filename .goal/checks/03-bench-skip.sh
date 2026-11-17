#!/usr/bin/env bash
# Without HALI_BENCH (what make test-external does) the benchmark skips quickly and passes.
set -u
start=$SECONDS
out=$(env -u HALI_BENCH cargo test --features test-hooks --test bench_find -- --ignored --nocapture 2>&1)
rc=$?
elapsed=$((SECONDS - start))
[ "$rc" -eq 0 ] || { echo "MISSING: skip run failed (rc=$rc): $(tail -5 <<<"$out")"; exit 1; }
grep -q "skipped: run with make bench" <<<"$out" || { echo "MISSING: no 'skipped: run with make bench' message"; exit 1; }
# Allow for the compile; the skip itself must not run the benchmark.
[ "$elapsed" -lt 600 ] || { echo "BAD: skip took ${elapsed}s"; exit 1; }
