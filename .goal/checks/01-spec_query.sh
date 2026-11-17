#!/usr/bin/env bash
# The patterns hold literal backticks, angle brackets and quotes, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 18 (#9): docs/spec.md §9.9 states the whole `hali query` contract (dataset, input,
# every output form, read-only, SERVICE, bounds, flags, store, schema) and §9.4 lists the new
# rule QUERY_FAILED.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

section=$(awk '/^### 9\.9 /{on=1; next} /^### 9\.10 |^---$|^## 10\./{on=0} on' "$spec")
if [ -z "$section" ]; then
    echo "MISSING: $spec has no §9.9"
    exit 1
fi
for what in \
    'default graph' 'merge' 'GRAPH' 'FROM NAMED' 'FROM' 'never fetched' \
    'PREFIX' '§3.4' 'argument' 'stdin' \
    'W3C' 'schema_version' '{"head":{},"boolean"' 'TSV' 'ASK' 'true' 'false' \
    'CONSTRUCT' 'DESCRIBE' 'Turtle' '\u' '§9.3' \
    'READ_ONLY_QUERY' 'SPARQL Update' 'INSERT' 'LOAD' 'comment' 'invalid SPARQL query' \
    'QUERY_FAILED' 'SERVICE' 'SILENT' 'network' 'timeout' 'too_large' \
    '30 s' '100,000' 'LIMIT' '--limit' \
    '--dry-run' '--yes' '--agent' \
    'core graph' 'CONFIG_INVALID' 'STORE_UNPARSEABLE' \
    'hali schema query'; do
    if ! printf '%s\n' "$section" | grep -qF -- "$what"; then
        echo "MISSING: §9.9 does not mention: $what"
        status=1
    fi
done

# Review of #9 (F9, F4/F2, F10): the contract says what the code does.
flat=$(printf '%s\n' "$section" | tr '\n' ' ' | tr -s ' ')
for what in \
    'except LF and TAB' \
    'empty or comment-only' \
    'custom function' \
    'blank node' 'not stable across runs'; do
    if ! printf '%s\n' "$flat" | grep -qF -- "$what"; then
        echo "MISSING: §9.9 does not say: $what"
        status=1
    fi
done
if ! printf '%s\n' "$flat" | grep -qiE 'empty or comment-only[^.]*usage error|usage error[^.]*empty or comment-only'; then
    echo "MISSING: §9.9 does not say that empty or comment-only text is a usage error"
    status=1
fi
if ! printf '%s\n' "$flat" | grep -qiE 'custom function[^.]*usage error|usage error[^.]*custom function'; then
    echo "MISSING: §9.9 does not say that an evaluation error from an unsupported custom function is a usage error"
    status=1
fi
if ! printf '%s\n' "$flat" | grep -qiE 'blank node[^.]*not stable across runs|not stable across runs[^.]*blank node'; then
    echo "MISSING: §9.9 does not say that the blank node labels of CONSTRUCT/DESCRIBE output are not stable across runs"
    status=1
fi

rules=$(grep -F '**Rules** (SCREAMING_SNAKE, minimum set)' "$spec" || true)
if [ -z "$rules" ]; then
    echo "MISSING: $spec has no Rules list in §9.4"
    status=1
elif ! printf '%s\n' "$rules" | grep -qF '`QUERY_FAILED`'; then
    echo "MISSING: the §9.4 list of rules does not include \`QUERY_FAILED\`"
    status=1
fi
exit "$status"
