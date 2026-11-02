#!/usr/bin/env bash
# shellcheck disable=SC2016
# (the backticks in the quoted strings are literal Markdown, not command substitution)
set -uo pipefail
# Criterion 9 (#131): spec §7 lists the built-in excludes (with `.git` and the data dir), the
# ignore-file sources and which apply only inside a repository, the fail-closed ignore-file rule,
# the symlink rule (inside-root links silent), that the walker opens no regular file, the skip
# reasons, the one-level-listing note, and the refused empty and whitespace-only excludes; §15
# pins `ignore` 0.4.34. The goal commit writes this text, so the check passes by design: it
# guards against an implementer editing the spec away.
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
need7 '`.gitignore`, `.git/info/exclude` and the global gitignore (git'"'"'s `core.excludesFile`, else `$XDG_CONFIG_HOME/git/ignore`) apply only inside a git repository (a directory holding `.git` or `.jj` counts as one)'
need7 '`.ignore` and **`.haliignore`** (gitignore syntax) apply in any directory, including the root'"'"'s parents'
need7 '`--no-ignore` turns off exactly these five, and nothing else'
need7 'An ignore file with an invalid glob line still applies its valid lines and is reported as skipped (`bad_ignore_file`)'
need7 '**Ignore files fail closed:**'
need7 'with `lstat` and a bounded, non-blocking read'
need7 'is not a regular file, is over 1 MiB, is not valid UTF-8 or cannot be read is reported as skipped (`bad_ignore_file`, at that file) and its directory is not walked; for the root or a parent of the root, nothing is walked'
need7 'a `.git` file (which names the repository'"'"'s `info/exclude`)'
need7 'Does not follow symlinked directories (nothing under one is scanned, and it is not reported)'
need7 'inside the root it is silent, because the real file is walked directly and is subject to every filter'
need7 'outside the root it is skipped (`outside_root`); a broken or looping one, or one that leads to something that is not a regular file, is skipped (`not_regular`)'
need7 'A skipped symlink is reported at its own path.'
need7 'Skip reasons: `too_large` (over `max_size`), `unreadable` (a directory the walk cannot read, or an entry whose `stat` fails), `not_regular`'
need7 '`non_utf8` (a directory with such a name is reported once and not entered), `outside_root` and `bad_ignore_file`'
need7 'The walker does not open regular files: whether one can be read is left to `inspect`'
need7 'Hidden, ignored, built-in and pruned directories are listed one level by the `ignore` crate before being skipped, but nothing under them is yielded or entered.'
need7 'The walk is built from the canonical root.'
need7 'a pattern with no `/` matches a name at any depth, one with a `/` is anchored at the root; an invalid glob, an empty or whitespace-only one, or one starting with `!`, is rejected'

printf '%s\n' "$section15" | grep -qF '| Directory walk | `ignore` | 0.4.34 |' || {
    echo 'MISSING: §15 pins `ignore` at 0.4.34 (| Directory walk | `ignore` | 0.4.34 |)'
    status=1
}
exit "$status"
