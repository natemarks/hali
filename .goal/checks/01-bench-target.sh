#!/usr/bin/env bash
# make bench exists with the right command line, and make help lists it.
set -u
fail=0
dry=$(env -u MAKEFLAGS make --no-print-directory -n bench 2>&1) || { echo "MISSING: 'make bench' does not exist: $dry"; exit 1; }
for want in "cargo test --release" "HALI_BENCH=1" "--ignored" "bench_find"; do
  grep -qF -- "$want" <<<"$dry" || { echo "MISSING: 'make -n bench' lacks: $want"; fail=1; }
done
if grep -qF "test-hooks" <<<"$dry"; then echo "BAD: 'make bench' must not enable test-hooks"; fail=1; fi
env -u MAKEFLAGS make --no-print-directory help 2>/dev/null | grep -Eq '(^|[[:space:]])bench:?[[:space:]]' || { echo "MISSING: 'make help' does not list bench"; fail=1; }
if grep -Eq "^[[:space:]]*(export[[:space:]]+)?MAKEFLAGS[[:space:]]*[:+?]?=" Makefile; then echo "BAD: Makefile sets MAKEFLAGS globally; remove it (checks pass --no-print-directory themselves)"; fail=1; fi
exit "$fail"
