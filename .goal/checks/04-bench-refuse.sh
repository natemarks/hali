#!/usr/bin/env bash
# With HALI_BENCH=1, a debug or test-hooks build refuses to run.
set -u
out=$(HALI_BENCH=1 cargo test --features test-hooks --test bench_find -- --ignored --nocapture 2>&1)
rc=$?
[ "$rc" -ne 0 ] || { echo "MISSING: HALI_BENCH=1 with test-hooks exited 0"; exit 1; }
grep -q "make bench needs a release build without test-hooks" <<<"$out" || { echo "MISSING: refusal message 'make bench needs a release build without test-hooks' not printed"; exit 1; }
