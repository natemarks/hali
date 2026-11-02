#!/usr/bin/env bash
set -uo pipefail
# Criterion 9 (#117): clap usage errors are escaped in one place. src/cli no longer lets clap print
# and exit (`Cli::parse()`, `.exit()`, `process::exit`), the §9.3 list occurs once under src/, and
# `fn usage_text` exists.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0

for pattern in 'Cli::parse()' '.exit()' 'process::exit'; do
    found=$(grep -rnF -- "$pattern" src/cli || true)
    if [ -n "$found" ]; then
        while IFS= read -r line; do
            echo "FOUND: $pattern at $line"
        done <<<"$found"
        status=1
    fi
done

count=$( (grep -rnF -- "'\\u{2028}'" src || true) | wc -l)
if [ "$count" -ne 1 ]; then
    echo "WRONG: the §9.3 list ('\\u{2028}') occurs $count times under src/, expected exactly 1"
    status=1
fi

if ! grep -rqE 'fn usage_text\(' src; then
    echo "MISSING: no fn usage_text under src/"
    status=1
fi
exit "$status"
