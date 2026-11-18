#!/usr/bin/env bash
# The patterns hold literal backticks and a literal tilde, which must not expand.
# shellcheck disable=SC2016,SC2088
set -uo pipefail
# Criterion 17 (#25): docs/spec.md states the `new` contract: the §9.1 synopsis with
# `--path-only`, a **New:** paragraph in §9.1, §9.6 scope naming `new`, and §8 naming the
# FILES_DIR_UNUSABLE rule, the slug cap and the symlink rule. Rows 19-20: the **New:** paragraph
# says the deepest existing prefix of files.dir must be a directory (else `not_a_directory`, in a
# dry run too), and that `..` after a missing component is resolved against the real prefix
# (the real path is recorded, never the lexical one).
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

if ! grep -qF 'hali new "<title>" [--type T]… [--tag C]… [--path-only]' "$spec"; then
    echo "MISSING: the §9.1 synopsis of $spec is not: hali new \"<title>\" [--type T]… [--tag C]… [--path-only]"
    status=1
fi

para=$(grep -E '^\*\*New:\*\*' "$spec" || true)
if [ -z "$para" ]; then
    echo "MISSING: $spec has no **New:** paragraph in §9.1"
    status=1
else
    for what in 'FILES_DIR_UNUSABLE' 'unresolved' 'relative' 'not_a_directory' 'own_dir' \
        'cannot_create' 'NON_UTF8_PATH' 'UNKNOWN_TYPE' 'UNKNOWN_CONCEPT' 'SHACL_VIOLATION' \
        'files.dir' 'user-dirs' '~/Documents/hali' 'text/markdown' 'dcterms:created' \
        'prov:wasAttributedTo' '--path-only' '--dry-run' '--json' '--yes' 'schema' 'O_EXCL' \
        'O_NOFOLLOW' 'umask' '128' 'untitled' 'usage error' 'real path' 'EDITOR' 'stdin' \
        'dry_run' 'changes' 'lstat' 'symlink' 'created' 'would create' \
        'deepest existing' '`..`'; do
        if ! printf '%s\n' "$para" | grep -qF -- "$what"; then
            echo "MISSING: the **New:** paragraph does not mention: $what"
            status=1
        fi
    done
fi

scope=$(grep -E '^- \*\*Scope:\*\*' "$spec" || true)
if ! printf '%s\n' "$scope" | grep -qF '`add`, `scan` and `new`'; then
    echo 'MISSING: the §9.6 Scope bullet of '"$spec"' does not say: `add`, `scan` and `new`'
    status=1
fi

section8=$(awk '/^## 8\. /{on=1; next} /^## 9\. /{on=0} on' "$spec")
for what in 'FILES_DIR_UNUSABLE' '128' 'symlink' '--path-only' 'dcterms:created'; do
    if ! printf '%s\n' "$section8" | grep -qF -- "$what"; then
        echo "MISSING: §8 of $spec does not mention: $what"
        status=1
    fi
done
exit "$status"
