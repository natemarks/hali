#!/usr/bin/env bash
set -uo pipefail
# Criterion 8 (#123): docs/spec.md §9 states the fixed program name. Written by the test author in
# the goal commit: a guard, not a red.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

# The sentence holds literal backticks, which must not expand.
# shellcheck disable=SC2016
if ! grep -qF -- 'One `hali` binary (clap 4, derive). Its help, usage errors and version always call it `hali`, whatever name it was started under: the name it was run as (`argv[0]`) is never printed.' docs/spec.md; then
    echo "MISSING: §9 program-name sentence in docs/spec.md"
    exit 1
fi
