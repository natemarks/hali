#!/usr/bin/env bash
# Temporary check for #8: docs/spec.md §9.5 states the dry-run contract.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# §9.5 only, markup stripped, line wrapping joined.
section=$(awk '/^### 9\.5 /{on=1; next} /^### /{on=0} on' docs/spec.md \
    | tr -d '`*_' | tr '\n' ' ' | tr -s ' ')

missing() {
    echo "docs: spec §9.5 does not state $1"
    exit 1
}

[ -n "${section// /}" ] || missing "anything (section 9.5 not found)"
grep -qiE 'add[^.]{0,40}before[^.]{0,40}remove' <<<"$section" || missing "the order of changes (add before remove)"
grep -qi 'byte order' <<<"$section" || missing "the order of changes (byte order)"
grep -qi 'read-only' <<<"$section" || missing "that read-only commands ignore --dry-run"
grep -qi 'global' <<<"$section" || missing "that --dry-run is a global flag"
grep -qF 'note: dry run; nothing was written' <<<"$section" || missing "the text-mode note (note: dry run; nothing was written)"
grep -qi 'would add concept' <<<"$section" || missing "the text-mode output (would add concept)"
grep -qiE 'same error|identical' <<<"$section" || missing "that errors are identical to a real run"
echo "docs: spec §9.5 states the dry-run contract"
