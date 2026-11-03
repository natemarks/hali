#!/usr/bin/env bash
# shellcheck disable=SC2016
# (the backticks in the quoted strings are literal Markdown, not command substitution)
set -uo pipefail
# Criterion 9 (#131): spec §7 lists the built-in excludes (with `.git` and the data dir), the
# ignore-file sources and which apply only inside a repository, the fail-closed ignore-file rule,
# the symlink rule (inside-root links silent), that the walker opens no regular file other than
# ignore-file candidates, the 256 KiB cap and the 100-level nesting bound, the build-failure rule,
# the `.git` directory / file / commondir coverage, the accepted race, the parents and `!` lines,
# the global-config gap (a FIFO at a config file, silent loss of global gitignore errors), the live
# ignore-rule budget (512 KiB along the current path, per directory that reads a file, the files of
# one directory adding up), the CPU cost in deep trees, the caller notes, the silent loss of rules when a file changes
# after the check, the root-is-the-data-dir note, the skip reasons, the one-level-listing note, and
# the refused empty, whitespace-only and over-nested excludes; §15
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
need7 'An invalid glob line in one of the per-directory sources (`.gitignore`, `.ignore`, `.haliignore`, `.git/info/exclude`) still lets the file'"'"'s valid lines apply, and the file is reported as skipped (`bad_ignore_file`); the global gitignore is not covered'
need7 '**Ignore files fail closed:**'
need7 'with `lstat` and a bounded, non-blocking read'
need7 '`.git/info/exclude` when `.git` is a directory (or a symlink to one)'
need7 'for a `.git` file (which names the repository'"'"'s `info/exclude`), the file itself, the `commondir` it leads to and `<common>/info/exclude`'
need7 'is not a regular file, is over 256 KiB (262,144 bytes), has `{` nesting deeper than 100 levels, is not valid UTF-8 or cannot be read is reported as skipped (`bad_ignore_file`, at that file) and its directory is not walked; for the root or a parent of the root, nothing is walked'
need7 'A `.git` that is a FIFO, a symlink to a file, or dangling is rejected the same way.'
need7 'A matcher the `ignore` crate fails to build for a file that passed those checks'
need7 'also fails closed, reported at the candidate file.'
need7 'A FIFO or a device named as an ignore file is therefore never opened.'
need7 '**Live ignore-rule budget:**'
need7 'keeps the compiled matcher of every ancestor directory alive while the walk is under it, so memory grows with depth'
need7 'a running total of the bytes of ignore files accepted along the current path (the root'"'"'s parents included; a file is counted once for each directory that reads it, so an `info/exclude` shared by several worktrees is counted once per directory, which matches the crate; the files of one directory add up against the budget)'
need7 'over 512 KiB (524,288 bytes) as skipped (`bad_ignore_file`, at that file); its directory is not walked, and for the root or a parent of the root, nothing is walked.'
need7 'A file counts only while the walk is under its directory, so siblings are not charged for each other'"'"'s files.'
need7 '**CPU in deep trees:** the `ignore` crate matches every entry against the matchers of every ancestor directory, so time grows with depth × files per level × ignore files'
need7 'There is no depth cap. This is accepted: `hali scan` (#22) must include a time bound in its tests, and the follow-up that reads ignore files with hali'"'"'s own matcher, with a depth or live-matcher cap, is the fix.'
need7 '**Caller notes:** `walk()` checks the ignore files of the root and its parents when it is called, but the `ignore` crate reads them at the first `next()`, so callers must iterate at once.'
need7 '`WalkOptions::new` and `walk()` must not be called on a thread with a tiny custom stack (a debug build aborts on a 100-level pattern on a 64 KiB stack; 256 KiB is fine).'
need7 'The budget assumes the sequential depth-first walk and would silently break under a parallel walk.'
need7 'a world-writable parent such as `/tmp` that holds large ignore files can use the whole budget for every root below it.'
need7 'a race remains, and it is not narrow: whoever controls the tree controls how wide it is'
need7 'Those are denials of service.'
need7 'If a file changes after the check, the crate also silently throws away I/O errors from ignore files, so a directory can be walked with none, or only the first lines, of its rules, and nothing is reported: rules can be lost silently. This is accepted.'
need7 'closes it and is the first task after `hali scan` (#22); `hali scan` leaves the store unchanged if the process dies mid-scan.'
need7 'ignore files in the root'"'"'s parents are honoured (git semantics), including world-writable ones such as `/tmp`'
need7 'A `!` line in an ignore file (in the root, a subdirectory or a parent) can override the hidden rule; user excludes, the built-in excludes and the data dir cannot be overridden.'
need7 'not git'"'"'s own configuration (`~/.gitconfig`, `$XDG_CONFIG_HOME/git/config`, `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`, `/etc/gitconfig`) or the file `core.excludesFile` names.'
need7 'A FIFO at one of the git config files blocks the walk (`walk()` does not return); the file `core.excludesFile` names is checked with `is_file()` first, so a FIFO there is never opened.'
need7 'The `ignore` crate drops errors in the global gitignore silently: an invalid line, or a file it cannot build, is never reported, and its rules vanish.'
need7 'the `ignore` crate never applies the built-in and data-dir filter to the root itself, so a root that is the data dir, or inside it, is walked; `hali scan` refuses such a root.'
need7 'Does not follow symlinked directories (nothing under one is scanned, and it is not reported)'
need7 'inside the root it is silent, because the real file is walked directly and is subject to every filter'
need7 'outside the root it is skipped (`outside_root`); a broken or looping one, or one that leads to something that is not a regular file, is skipped (`not_regular`)'
need7 'A skipped symlink is reported at its own path.'
need7 'Skip reasons: `too_large` (over `max_size`), `unreadable` (a directory the walk cannot read, a symlink whose target cannot be reached for lack of permission, or an entry whose `stat` fails), `not_regular`'
need7 '`non_utf8` (a directory with such a name is reported once and not entered), `outside_root` and `bad_ignore_file`'
need7 'The walker does not open regular files other than ignore-file candidates (opened by the check above and by the `ignore` crate): whether one can be read is left to `inspect`'
need7 'Hidden, ignored, built-in and pruned directories are listed one level by the `ignore` crate before being skipped, but nothing under them is yielded or entered.'
need7 'The walk is built from the canonical root.'
need7 'a pattern with no `/` matches a name at any depth, one with a `/` is anchored at the root; an invalid glob, an empty or whitespace-only one, one starting with `!`, or one with `{` nested deeper than 100 levels (the same bound as for ignore files), is rejected'

printf '%s\n' "$section15" | grep -qF '| Directory walk | `ignore` | 0.4.34 |' || {
    echo 'MISSING: §15 pins `ignore` at 0.4.34 (| Directory walk | `ignore` | 0.4.34 |)'
    status=1
}
exit "$status"
