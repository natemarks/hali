#!/usr/bin/env bash
set -uo pipefail
# Criterion 7 (#123): nothing reads the program name, and `usage_text` and `parse` are unchanged
# from main. A guard, not a red.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0

found=$(git grep -nE 'args_os|current_exe|\barg0\b|env::args|/proc/self' -- src || true)
if [ -n "$found" ]; then
    while IFS= read -r line; do
        echo "FOUND: program-name read in $line"
    done <<<"$found"
    status=1
fi

# The body of `fn <name>` from stdin: from its signature to the closing brace at column 0.
body() {
    awk -v name="$1" '
        $0 ~ "^(pub )?fn " name "\\(" { on = 1 }
        on { print }
        on && /^}/ { exit }
    '
}

base=$(git merge-base HEAD origin/main 2>/dev/null || true)
if [ -z "$base" ]; then
    echo "MISSING: no merge base with origin/main (shallow clone or no origin/main?); run 'git fetch origin main' so usage_text and parse can be compared"
    exit 1
fi

for name in usage_text parse; do
    now=$(body "$name" <src/cli/mod.rs)
    was=$(git show "$base:src/cli/mod.rs" | body "$name")
    if [ -z "$was" ] || [ -z "$now" ]; then
        echo "MISSING: fn $name not found in src/cli/mod.rs (now or at $base)"
        status=1
    elif [ "$now" != "$was" ]; then
        echo "CHANGED: fn $name in src/cli/mod.rs differs from $base"
        status=1
    fi
done
exit "$status"
