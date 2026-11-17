#!/usr/bin/env bash
# Docs: AGENTS.md row, spec §14.5 and §14.7.
set -u
fail=0
miss() { echo "MISSING: $1"; fail=1; }
grep -Eq '^\| .make bench. \|' AGENTS.md || miss "AGENTS.md commands table row for make bench"
if grep -E 'arrive with their tickets' AGENTS.md | grep -q 'make bench'; then miss "AGENTS.md 'arrive with their tickets' sentence still names make bench"; fi
s5=$(awk '/^### 14\.5/{f=1;next} /^### /{f=0} f' docs/spec.md)
for want in HALI_BENCH "warm-up" "nearest-rank" "verdict"; do
  grep -qiF -- "$want" <<<"$s5" || miss "spec §14.5 does not mention: $want"
done
s7=$(awk '/^### 14\.7/{f=1;next} /^### /{f=0} f' docs/spec.md)
grep -q 'HALI_BENCH' <<<"$s7" || miss "spec §14.7 does not say the benchmark skips without HALI_BENCH"
exit "$fail"
