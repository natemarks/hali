#!/usr/bin/env bash
set -uo pipefail
# Criterion 11 (#20, spec §6): only `add` and `refresh` hash. `core` receives `stat` and `hash`
# as injected functions and never calls `storage::inspect`; in the CLI the one hashing function,
# `storage::inspect`, is reached from `inspect_all` (add) and from the Refresh arm (or a helper
# fn named `refresh*` it calls), nowhere else. `find` and `show` only `stat`.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0

hits=$(grep -rnE 'inspect\(|storage::inspect' src/core || true)
if [ -n "$hits" ]; then
    echo "WRONG: src/core must not call inspect (hashing is injected):"
    printf '%s\n' "$hits"
    status=1
fi

# Each `storage::inspect` line of src/cli/mod.rs, with the fn or `Command::<Arm>` it sits in.
found=$(awk '
    /^(pub )?fn [a-z_0-9]+/ { match($0, /fn [a-z_0-9]+/); region = "fn " substr($0, RSTART + 3, RLENGTH - 3) }
    /^        Command::[A-Za-z]+/ { match($0, /Command::[A-Za-z]+/); region = "arm " substr($0, RSTART + 9, RLENGTH - 9) }
    /storage::inspect/ && $0 !~ /^[[:space:]]*\/\// { print region ":" NR }
' src/cli/mod.rs)

in_refresh=0
while IFS= read -r line; do
    [ -n "$line" ] || continue
    region=${line%%:*}
    case "$region" in
    "fn inspect_all") ;;
    "arm Refresh" | "fn refresh"*) in_refresh=1 ;;
    *)
        echo "WRONG: src/cli/mod.rs calls storage::inspect outside inspect_all and the Refresh arm: $line"
        status=1
        ;;
    esac
done <<<"$found"

if [ "$in_refresh" -eq 0 ]; then
    echo "MISSING: no storage::inspect call in the Refresh arm of src/cli/mod.rs (or a fn named refresh*)"
    status=1
fi
exit "$status"
