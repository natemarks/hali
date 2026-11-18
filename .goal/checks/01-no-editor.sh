#!/usr/bin/env bash
set -uo pipefail
# Criterion 13 (#25, spec §8): `hali new` never launches an editor, never reads EDITOR or VISUAL
# and never spawns a process, so nothing under src/ may name them. Red until `new` exists (a
# grep that finds nothing proves nothing about a command that is not there); then it guards the
# source: `git grep` over src/ for EDITOR, VISUAL or process::Command must be empty.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
if ! git grep -qE 'Command::New\b' -- src/cli/mod.rs; then
    echo "MISSING: src/cli/mod.rs has no Command::New: hali new is not implemented yet"
    status=1
fi
found=$(git grep -nE 'EDITOR|VISUAL|process::Command' -- src/ || true)
if [ -n "$found" ]; then
    echo "FOUND: src/ must never name an editor or spawn a process:"
    printf '%s\n' "$found"
    status=1
fi
exit "$status"
