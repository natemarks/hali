#!/usr/bin/env bash
# shellcheck disable=SC2016  # the backticks in the phrases are literal markdown
set -euo pipefail

# Row 19 (#27): docs/spec.md 9.6, 11.1 and 11.3 say what #27 builds.
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

spec=docs/spec.md
missing=0

# section <heading prefix>: the text of one ### section, up to the next heading.
section() {
    awk -v h="$1" 'index($0, h) == 1 { on = 1; next } on && /^##/ { exit } on { print }' "$spec"
}

need() {
    local heading="$1" phrase="$2"
    if ! section "$heading" | grep -Fq -- "$phrase"; then
        echo "27-spec: '$heading' lacks: $phrase" >&2
        missing=1
    fi
}

need '### 11.3 ' 'prints `docs/agent-usage.md` verbatim'
need '### 11.3 ' 'hali:snippet:start'
need '### 11.3 ' 'hali:snippet:end'
need '### 11.1 ' 'start with an `Examples:` block'
need '### 11.1 ' '`hali --dry-run`'
need '### 9.6 ' 'except `schema`, `help` and `generate`'

if [ "$missing" -ne 0 ]; then
    exit 1
fi
echo "27-spec: ok"
