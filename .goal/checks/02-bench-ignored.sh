#!/usr/bin/env bash
# The benchmark is an ignored test: unit-style runs list it as ignored, never run it.
set -u
out=$(cargo test --features test-hooks --test bench_find 2>&1) || { echo "MISSING: tests/bench_find.rs does not build or run: $(tail -5 <<<"$out")"; exit 1; }
grep -q "1 ignored" <<<"$out" || { echo "MISSING: bench_find does not report '1 ignored'"; exit 1; }
list=$(cargo test --features test-hooks --test bench_find -- --list --ignored 2>&1)
grep -q "bench_find_latency" <<<"$list" || { echo "MISSING: bench_find_latency is not an ignored test"; exit 1; }
