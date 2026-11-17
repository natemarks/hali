#!/usr/bin/env bash
set -uo pipefail
# Criterion 19 (#9): the quick-xml justification in .cargo/audit.toml is re-checked for the
# query evaluator: it gains a line saying `hali query` builds its evaluator without the default
# HTTP service handler (so SERVICE cannot reach the network), names the source guard, and the
# two advisories are still the only ignores.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
file=.cargo/audit.toml
if [ ! -f "$file" ]; then
    echo "MISSING: $file does not exist"
    exit 1
fi
comment=$(grep -E '^[[:space:]]*#' "$file")
for what in 'without_default_http_service_handler' 'SERVICE' 'tests/sparql_guard.rs'; do
    if ! printf '%s\n' "$comment" | grep -qF -- "$what"; then
        echo "MISSING: the justification in $file does not mention: $what"
        status=1
    fi
done
if ! printf '%s\n' "$comment" | grep -qiE 'hali query.*(evaluator|SERVICE)|(evaluator|SERVICE).*hali query'; then
    echo "MISSING: the justification in $file has no line tying the query evaluator to hali query"
    status=1
fi
# F8: the paragraph about the query evaluator sits after the quick-xml reasoning, which ends
# with "cannot be bumped from here.", not in the middle of it.
bumped=$(grep -n 'cannot be bumped from here\.' "$file" | head -1 | cut -d: -f1)
paragraph=$(grep -n 'without_default_http_service_handler' "$file" | head -1 | cut -d: -f1)
if [ -z "$bumped" ]; then
    echo "MISSING: $file no longer ends the quick-xml reasoning with \"cannot be bumped from here.\""
    status=1
elif [ -n "$paragraph" ] && [ "$paragraph" -le "$bumped" ]; then
    echo "MISPLACED: the hali query paragraph (line $paragraph) must come after the quick-xml reasoning that ends at line $bumped (\"cannot be bumped from here.\")"
    status=1
fi
for id in RUSTSEC-2026-0194 RUSTSEC-2026-0195; do
    if ! grep -qE "^[[:space:]]*\"$id\"," "$file"; then
        echo "MISSING: $file no longer ignores $id (the ignore list must stay as it was)"
        status=1
    fi
done
extra=$(grep -E '^[[:space:]]*"RUSTSEC-' "$file" | grep -vE 'RUSTSEC-2026-019[45]' || true)
if [ -n "$extra" ]; then
    echo "CHANGED: $file ignores more than the two quick-xml advisories:"
    printf '%s\n' "$extra"
    status=1
fi
exit "$status"
