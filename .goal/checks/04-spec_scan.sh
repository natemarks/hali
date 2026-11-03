#!/usr/bin/env bash
# The patterns hold literal backticks, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 23 (#22): docs/spec.md states the scan contract: a **Scan:** paragraph in §9.1, the
# Refresh paragraph's "Nothing but `add`, `refresh` and `scan` ever hashes a file", and §7 saying
# the filters gate new registrations only.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

para=$(grep -E '^\*\*Scan:\*\*' "$spec" || true)
if [ -z "$para" ]; then
    echo "MISSING: $spec has no **Scan:** paragraph in §9.1"
    status=1
else
    for what in '--include-hidden' '--no-ignore' '--exclude' '--max-size' '--media-type' '--yes' \
        'root' 'counts' 'media_types' 'truncated' 'missing' 'skipped' 'dry_run' 'changes' \
        'too_large' 'unreadable' 'not_regular' 'non_utf8' 'outside_root' 'bad_ignore_file' \
        'NON_UTF8_PATH' 'FILE_UNREADABLE' 'SHACL_VIOLATION' 'CONFIG_INVALID' \
        'data dir' 'refresh' '100' 'stderr' 'never asks' 'usage error' 'one mutation'; do
        if ! printf '%s\n' "$para" | grep -qF -- "$what"; then
            echo "MISSING: the **Scan:** paragraph does not mention: $what"
            status=1
        fi
    done
fi

if ! grep -qF 'Nothing but `add`, `refresh` and `scan` ever hashes a file' "$spec"; then
    echo "MISSING: the §9.1 Refresh paragraph in $spec does not say: Nothing but \`add\`, \`refresh\` and \`scan\` ever hashes a file"
    status=1
fi

gate=$(grep -E 'gate only new registrations' "$spec" || true)
if [ -z "$gate" ]; then
    echo "MISSING: §7 in $spec has no bullet saying filters gate only new registrations"
    status=1
else
    for what in 'hidden' 'max_size' '--media-type' 'never un-cataloged'; do
        if ! printf '%s\n' "$gate" | grep -qF -- "$what"; then
            echo "MISSING: the §7 bullet on what gates only new registrations does not mention: $what"
            status=1
        fi
    done
fi
exit "$status"
