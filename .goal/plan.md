# Plan: #131 Walker seam for scan (storage::walk and the ignore crate)

Contract: see issue #131 ("Contract (groomed)"). It is the walker half of #22, split off with the human's approval of the #22 plan (the "Decision 1, option A" split). The full feature is described in #22 and its goal-map comment; this PR is rows 3 to 6, 21 and 22 of that map plus the walker part of row 7. #22 is blocked by this issue.

## Goal map

New file `tests/storage_walk.rs` (goal-tagged, locked): real temp trees under `CARGO_TARGET_TMPDIR`, HOME and XDG_CONFIG_HOME pointed at temp dirs so the global gitignore is controlled; the module is red at compile time under the `goal` feature only (no `storage::walk` yet). Where a `.git` repo is needed, create the directory skeleton by hand (a `.git` dir with `HEAD` is enough for `require_git`); do not run `git`.

| # | Criterion | Checks | Expected failure |
|---|---|---|---|
| 1 | Hidden names are skipped; `include_hidden` includes them; the root itself is always walked even if hidden | `storage_walk::hidden` | `cannot find function walk in module storage` |
| 2 | `.gitignore` is honoured only inside a git repo; `.git/info/exclude`, the global gitignore (temp HOME and XDG_CONFIG_HOME), `.ignore` and `.haliignore` are honoured, including in the root's parents; `no_ignore` turns off exactly these five; a bad ignore line still applies its valid lines and is yielded as `bad_ignore_file` | `storage_walk::{ignore_files, no_ignore, bad_ignore_file}` | same |
| 3 | `.git`, `node_modules`, `target`, `__pycache__`, `venv` and the data dir (`prune`) are never entered, at any depth, even with `include_hidden` and `no_ignore`; user excludes use gitignore syntax relative to the root (no `/` matches at any depth, with `/` anchored); an invalid glob or one starting with `!` is `Err(pattern)` from `WalkOptions::new` | `storage_walk::{builtin_excludes, user_excludes, bad_exclude}` | same |
| 4 | Directory symlinks are not followed and not reported; a file symlink inside the root is yielded once at its real path (de-duplicated against the file walked directly); outside the root it is `outside_root`; a broken symlink is `not_regular` | `storage_walk::symlinks` | same |
| 5 | FIFOs and sockets are never opened (no hang, 30 s timeout) and are `not_regular`; unreadable files and directories are `unreadable` (skipped as root); non-UTF-8 names are `non_utf8`, a directory with one is reported once and not entered; the walk continues in every case; mount points are crossed and hard links are separate paths (not tested beyond a hard link) | `storage_walk::{special_files, unreadable, non_utf8}` | same |
| 6 | The order is deterministic (sorted by file name) and the walk only reads (a tree snapshot with mtimes is identical before and after) | `storage_walk::{determinism, reads_only}` | same |
| 7 | The dependency delta is exactly `ignore = "=0.4.34"` in `Cargo.toml` and exactly `ignore` 0.4.34 and `globset` 0.4.18 added to `Cargo.lock` | `.goal/checks/01-dependency_delta.sh` (diff against `main`: any other change to either file fails) | `MISSING: ignore = "=0.4.34"` |
| 8 | The seams hold: `src/core` has no `std::fs`, `ignore::` or `storage::walk`; `ignore::` appears only in `src/storage/walk.rs` | `.goal/checks/03-seams.sh` | `MISSING: src/storage/walk.rs` |
| 9 | Spec §7 lists the built-ins (with `.git` and the data dir), the symlink and outside-root rule and the skip reasons; §15 pins `ignore` 0.4.34 | `.goal/checks/04-spec_walk.sh` (the test-author writes the spec text into the goal commit, so this passes by design) | `MISSING: §7 …` if absent |
| 10 | Existing goldens are unchanged | `.goal/checks/05-existing_goldens_unchanged.sh` (the #18/#19/#20 pattern) | a guard |

`.goal/locked`: `tests/storage_walk.rs`.

## Approach

Implementer on opus (it touches guarded paths and the dependency; the rest of the feature is #22).

1. `Cargo.toml`: add exactly `ignore = "=0.4.34"` after `clap`, default features. Let cargo update `Cargo.lock` offline if possible (`cargo update -p ignore --precise 0.4.34` or a plain build); the lock must gain exactly `ignore` and `globset`.
2. `src/storage/walk.rs` (declared in `src/storage/mod.rs`): `WalkOptions`, `Walked`, `SkipReason`, `walk(root, &opts)`, built as in the issue's contract (`WalkBuilder` with `follow_links(false)`, the hidden/git/ignore switches, `parents(true)`, `require_git(true)`, `.haliignore`, `sort_by_file_name`, `filter_entry` for built-ins and the data dir, `OverrideBuilder` for user excludes; never `max_filesize`). Symlink handling by `canonicalize` then `metadata` then the inside-root check; non-UTF-8 handling in the same place.
3. Spec §7 and §15.

Nothing else changes: no command, no config, no schema. `src/core` stays untouched.

## Security call

Sensitive: run the security-reviewer (guarded paths `src/storage/`, `Cargo.toml`, `Cargo.lock`). It should probe: the dependency's provenance and licence; its regex size limits against hostile `.gitignore`/`.haliignore` patterns (ReDoS, huge globs, very long lines, many patterns); that `ignore` reads `~/.gitconfig` as text and never runs `git`; that no directory symlink is followed and the root can never be left (outside-root targets refused); races between the walk's `stat` and a later `inspect` (#115 is unchanged); that FIFOs and devices are never opened; deep trees and large directories; terminal escapes in file names are the caller's concern (the walker yields raw paths; #22 escapes them in output).

## Risks

`ignore`'s global-gitignore lookup depends on HOME and XDG_CONFIG_HOME, so tests must control them. A hostile `.gitignore` is read from the scanned tree, which is untrusted input. The crate's behaviour with `require_git(true)` and `parents(true)` across filesystem boundaries needs a test with a temp tree. `ignore` needs Rust 1.88 or newer (the toolchain is 1.99.1).
