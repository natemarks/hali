#!/usr/bin/env bash
# The patterns hold literal backticks, angle brackets and quotes, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 21 (#24): docs/spec.md states the move and prune contract of `scan`: the synopsis of
# §9.1 gains `[--prune [--force]]`, the **Scan:** paragraph describes moves, `--prune`, the new
# output and text, and no longer says scan never asks or that Resources outside the root are not
# looked at; §9.8 gives the prompt, `"scan"` as a `details.command` and the retry.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

para=$(grep -E '^\*\*Scan:\*\*' "$spec" || true)
if [ -z "$para" ]; then
    echo "MISSING: $spec has no **Scan:** paragraph in §9.1"
    exit 1
fi
for what in '--prune' '--force' '--yes' 'relinked' 'ambiguous' 'pruned' 'candidates' 'from' \
    'one-to-one' 'CONFIRMATION_REQUIRED' 'inbound relations' 'SHACL_VIOLATION' \
    'forgot|would forget resource' 'relinked|would relink resource' 'candidate <path>' \
    '--force is for --prune' '100' '10'; do
    if ! printf '%s\n' "$para" | grep -qF -- "$what"; then
        echo "MISSING: the **Scan:** paragraph does not mention: $what"
        status=1
    fi
done
if printf '%s\n' "$para" | grep -qF '(#24)'; then
    echo "WRONG: the **Scan:** paragraph still says (#24): relinking and --prune are there now"
    status=1
fi
if printf '%s\n' "$para" | grep -qF 'Resources outside the root are not looked at'; then
    echo "WRONG: the **Scan:** paragraph still says Resources outside the root are not looked at"
    status=1
fi
if printf '%s\n' "$para" | grep -qF '`scan` is not destructive (§9.8): it never asks, never reads stdin and ignores `--yes`.'; then
    echo "WRONG: the **Scan:** paragraph still says scan never asks; --prune asks (§9.8)"
    status=1
fi

if ! grep -qF '[--media-type M]… [--prune [--force]] [--list] [--forget-root <root>]' "$spec"; then
    echo "MISSING: the scan synopsis of §9.1 in $spec does not contain [--prune [--force]]"
    status=1
fi

section=$(awk '/^### 9\.8 /{on=1; next} /^### 9\.9 /{on=0} on' "$spec")
if [ -z "$section" ]; then
    echo "MISSING: $spec has no §9.8"
    exit 1
fi
if ! printf '%s\n' "$section" | grep -qF 'forget <n> missing resources under <root>? [y/N] '; then
    echo "MISSING: §9.8 does not give the prompt: forget <n> missing resources under <root>? [y/N] "
    status=1
fi
if ! printf '%s\n' "$section" | grep -qF '"scan"'; then
    echo "MISSING: §9.8 does not name \"scan\" among the details.command values"
    status=1
fi
if ! printf '%s\n' "$section" | grep -qF -- '--prune --yes'; then
    echo "MISSING: §9.8 does not give the retry (hali scan <root> … --prune --yes)"
    status=1
fi
exit "$status"
