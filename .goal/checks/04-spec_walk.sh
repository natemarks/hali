#!/usr/bin/env bash
# shellcheck disable=SC2016
# (the backticks in the quoted strings are literal Markdown, not command substitution)
set -uo pipefail
# Criterion 9 (#131): spec §7 lists the built-in excludes (with `.git` and the data dir), the
# ignore-file sources, the symlink and outside-root rule and the skip reasons; §15 pins `ignore`
# 0.4.34. The goal commit writes this text, so the check passes by design: it guards against an
# implementer editing the spec away.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

section7=$(awk '/^## 7\. Scan/ { on = 1; next } /^## / { on = 0 } on' "$spec")
section15=$(awk '/^## 15\. Dependencies/ { on = 1; next } /^## / { on = 0 } on' "$spec")

need7() {
    printf '%s\n' "$section7" | grep -qF -- "$1" || {
        echo "MISSING: §7 text: $1"
        status=1
    }
}

need7 'directories named `.git`, `node_modules`, `target`, `__pycache__` and `venv`, at any depth, and hali'"'"'s data dir (§4) are never entered, even with `--include-hidden` or `--no-ignore`'
need7 '`.gitignore` only inside a git repository (a directory holding `.git`), `.git/info/exclude`, the global gitignore (git'"'"'s `core.excludesFile`, else `$XDG_CONFIG_HOME/git/ignore`), `.ignore` and **`.haliignore`** (gitignore syntax) in any directory, including the root'"'"'s parents'
need7 '`--no-ignore` turns off exactly these five, and nothing else'
need7 'reported as skipped (`bad_ignore_file`)'
need7 'Does not follow symlinked directories (nothing under one is scanned, and it is not reported)'
need7 'outside the root it is skipped (`outside_root`); a broken one is skipped (`not_regular`)'
need7 'Skip reasons: `too_large` (over `max_size`), `unreadable`, `not_regular`'
need7 '`non_utf8` (a directory with such a name is reported once and not entered), `outside_root` and `bad_ignore_file`'
need7 'a pattern with no `/` matches a name at any depth, one with a `/` is anchored at the root; an invalid glob, or one starting with `!`, is rejected'

printf '%s\n' "$section15" | grep -qF '| Directory walk | `ignore` | 0.4.34 |' || {
    echo 'MISSING: §15 pins `ignore` at 0.4.34 (| Directory walk | `ignore` | 0.4.34 |)'
    status=1
}
exit "$status"
