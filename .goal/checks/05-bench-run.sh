#!/usr/bin/env bash
# make bench really runs: output format only (never the numbers), repo left untouched.
set -u
command -v cargo >/dev/null 2>&1 || { echo "SKIP: cargo is not installed"; exit 0; }
fail=0
grep -q "env_clear" tests/bench_find.rs 2>/dev/null || { echo "MISSING: tests/bench_find.rs does not use env_clear"; fail=1; }
before=$(git status --porcelain)
log=$(mktemp)
trap 'rm -f "$log"' EXIT
# A first release build can be slow; allow it.
if ! timeout 3000 make bench >"$log" 2>&1; then
  echo "MISSING: 'make bench' failed: $(tail -8 "$log")"
  exit 1
fi
after=$(git status --porcelain)
if [ "$before" != "$after" ]; then
  echo "BAD: make bench changed the working tree"
  diff <(echo "$before") <(echo "$after")
  fail=1
fi
tab=$(printf '\t')
num='[0-9]+\.[0-9]+'
grep -Eq '^hali find --json: release binary, 30 fresh processes per size, warm page cache, nearest-rank percentiles$' "$log" || { echo "MISSING: header line"; fail=1; }
grep -Eq "^resources${tab}p50_ms${tab}p95_ms\$" "$log" || { echo "MISSING: column header line"; fail=1; }
for n in 1000 10000; do
  grep -Eq "^${n}${tab}${num}${tab}${num}\$" "$log" || { echo "MISSING: row for $n (tab separated, two numbers)"; fail=1; }
done
last=$(grep -v '^[[:space:]]*$' "$log" | tail -1)
case "$last" in
  "verdict: p95 "*" ms <= 200 ms at 10k Resources: the in-memory store stays (spec §3.2)") ;;
  "verdict: p95 "*" ms > 200 ms at 10k Resources: the on-disk storage fallback is a v0.1 task (spec §3.2); file a follow-up issue") ;;
  *) echo "MISSING: verdict as the last line of output"; fail=1 ;;
esac
exit "$fail"
