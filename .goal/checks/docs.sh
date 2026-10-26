#!/usr/bin/env bash
# Goal #7, criterion 19: the docs record the shacl feature choice and its cost.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

fail() {
    echo "docs: $*" >&2
    exit 1
}

# The shacl bullet of spec §16: from its first line to the next bullet or blank line.
bullet=$(awk '/^- \*\*rudof `shacl`/ {on=1; print; next} on && (/^- / || /^$/ || /^#/) {exit} on {print}' docs/spec.md)
for term in 'sparql' 'http-client' 'rdf-12'; do
    echo "$bullet" | grep -q -- "$term" || fail "spec §16 does not record the shacl features"
done

adr=$(find docs/adr -name '0001-*.md' | head -1)
[ -n "$adr" ] || fail "ADR-0001 not found"
consequences=$(sed -n '/^## Consequences/,$p' "$adr")
echo "$consequences" | grep -qi 'shacl\|rudof' || fail "ADR-0001 consequences do not mention the shacl dependency"
echo "$consequences" | grep -q 'http-client' || fail "ADR-0001 consequences do not mention the forced oxigraph features"
echo "docs: ok"
