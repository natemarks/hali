#!/usr/bin/env bash
# shellcheck disable=SC2016
# (backticks in the grep patterns are literal markdown)
set -uo pipefail
# Criterion 12 (#6): docs/spec.md documents `hali schema` in §9.1 and the read-only
# `--dry-run` rule for it in §9.5.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

spec=docs/spec.md
fail=0
miss() {
    echo "MISSING: $1"
    fail=1
}

section() { awk -v h="$1" 'index($0, "### " h) == 1 {on=1; next} /^### / {on=0} on' "$spec"; }

s91=$(section "9.1")
s95=$(section "9.5")

grep -q 'hali schema concept add' <<<"$s91" || miss "§9.1: example 'hali schema concept add' (subcommand path as separate words)"
grep -q '2020-12' <<<"$s91" || miss "§9.1: schema documents are JSON Schema draft 2020-12"
grep -qi 'exit 2\|usage error' <<<"$s91" || miss "§9.1: an unknown schema name is a usage error (exit 2)"
grep -q '`error`' <<<"$s91" || miss "§9.1: the error envelope's schema is named \`error\`"
grep -qi 'bare' <<<"$s91" || miss "§9.1: 'hali schema <name>' prints the bare schema document, not the envelope"
grep -q '^\s*- \*\*Read-only commands\*\*.*`schema`' <<<"$s95" || miss "§9.5: the read-only commands list that accept and ignore --dry-run includes \`schema\`"

exit "$fail"
