#!/usr/bin/env bash
# The patterns hold literal backticks, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 13 (#73): docs/spec.md states the escaping (§9.3 definition, §9.4 text bullet). Written
# by the test author in the goal commit: a guard, not a red.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

if ! grep -qF -- '- **Escaped for the terminal** means each control character (C0, DEL, C1), line or paragraph separator (U+2028, U+2029), bidi control (U+061C, U+200E, U+200F, U+202A–U+202E, U+2066–U+2069) and invisible format character (U+00AD, U+180E, U+200B, U+2060–U+2064, U+206A–U+206F, U+FEFF, U+FFF9–U+FFFB, U+E0000–U+E007F) is printed as `\u{<hex>}`; everything else, ZWNJ and ZWJ included, is printed as is. `--json` output is never escaped this way.' "$spec"; then
    echo "MISSING: §9.3 of $spec does not define Escaped for the terminal (control characters, separators, bidi controls, invisible format characters; ZWNJ and ZWJ kept; --json never escaped)"
    status=1
fi

if ! grep -qF -- 'the message, the suggested fix and the suggested command are each escaped for the terminal (§9.3), so outside text can neither add lines nor drive the terminal.' "$spec"; then
    echo "MISSING: §9.4 does not say the message, the suggested fix and the suggested command are each escaped for the terminal"
    status=1
fi
exit "$status"
