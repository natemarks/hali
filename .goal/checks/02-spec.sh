#!/usr/bin/env bash
set -uo pipefail
# Criteria 6 and 7 (#117): docs/spec.md states the §9.3 residue and the §9.4 usage-error rendering.
# Written by the test author in the goal commit: a guard, not a red.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

if ! grep -qF -- 'The list is explicit: other invisible format characters (such as the Arabic number signs U+0600–U+0605, U+06DD, U+070F, and script-specific format controls), variation selectors, the combining grapheme joiner, the Mongolian free variation selectors and the Hangul fillers are deliberately not escaped, because they are part of real text in some scripts. They can make two different ids look the same, but they cannot move the cursor, set the title or add a line.' "$spec"; then
    echo "MISSING: §9.3 residue sentence in $spec"
    status=1
fi

# The sentence holds literal backticks, which must not expand.
# shellcheck disable=SC2016
if ! grep -qF -- 'A usage error that clap reports itself (an unknown subcommand or flag, a missing or invalid argument, an unknown `schema` name) keeps clap'"'"'s layout without colour. Every argument it repeats is escaped for the terminal, and so is the rest of its text apart from its line breaks.' "$spec"; then
    echo "MISSING: §9.4 usage-error wording in $spec"
    status=1
fi
exit "$status"
