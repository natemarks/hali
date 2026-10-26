#!/usr/bin/env bash
# Goal #7, criterion 18: the SHACL dependencies are pinned, add no duplicate oxigraph crates,
# and hali never hands Turtle text to rudof's parsers.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

fail() {
    echo "shacl-dependency: $*" >&2
    exit 1
}

for crate in shacl rudof_rdf; do
    grep -Eq "^${crate} *= *(\"=0\.3\.35\"|\{[^}]*version *= *\"=0\.3\.35\")" Cargo.toml ||
        fail "$crate is not pinned in Cargo.toml"
done

dups=$(cargo tree -d -e normal --depth 0 2>/dev/null | grep -E '^(oxigraph|oxrdf|oxttl|oxrdfio) ' || true)
[ -z "$dups" ] || fail "duplicate crates: $(echo "$dups" | tr '\n' ' ')"

banned='RDFFormat|ReaderMode|from_path\(|merge_from_'
if grep -rqE "$banned" src; then
    fail "src/ hands Turtle text to rudof's parsers: $(grep -rnE "$banned" src | head -1)"
fi

grep -q 'rudof' .cargo/audit.toml || fail ".cargo/audit.toml does not mention rudof"
echo "shacl-dependency: ok"
