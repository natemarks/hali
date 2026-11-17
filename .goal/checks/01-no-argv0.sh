#!/usr/bin/env bash
set -uo pipefail
# #4 row 5: the generators are given the name "hali" explicitly and never read the program name.
# No source file reads argv[0], the executable path or the process's own files. Red until
# src/cli/generate.rs exists (so the grep is not vacuous).
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

if [ ! -f src/cli/generate.rs ]; then
    echo "MISSING: src/cli/generate.rs (the generate subcommand) does not exist"
    exit 1
fi
if ! grep -q '"hali"' src/cli/generate.rs; then
    echo "MISSING: src/cli/generate.rs never passes the literal \"hali\" to the generators"
    exit 1
fi
hits=$(grep -rnE 'args_os|current_exe|arg0|env::args|/proc/self' src || true)
if [ -n "$hits" ]; then
    echo "FOUND: source reads the program name or argv:"
    printf '%s\n' "$hits"
    exit 1
fi
exit 0
