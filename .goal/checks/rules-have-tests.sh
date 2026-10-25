#!/usr/bin/env bash
# Goal #3, criterion 10 (spec §14.1): every command has a core test, a CLI test and an
# insta golden snapshot; every error rule has a test that triggers it.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

fail() {
    echo "rules-have-tests: $*" >&2
    exit 1
}

required_rules="LABEL_TAKEN STORE_UNPARSEABLE STORE_IO CONFIG_INVALID"

# 1. The required rules are implemented...
for rule in $required_rules; do
    grep -rq "\"$rule\"" src || fail "rule $rule is not implemented in src/"
done

# 2. ...and every rule emitted by src/ is triggered by a CLI test.
emitted=$(grep -rhoE '"[A-Z]{2,}(_[A-Z]+)+"' src | tr -d '"' |
    grep -vE '^(HALI_|XDG_|RUST_|NO_COLOR)' | sort -u)
for rule in $emitted $required_rules; do
    grep -q "$rule" tests/cli_concept.rs tests/snapshots/cli_concept__*.snap 2>/dev/null || fail "no CLI test triggers rule $rule"
done

# 3. Each command has a core test, a CLI test and a golden snapshot.
grep -q 'concept::add' tests/core_concept.rs || fail "no core test for concept add"
grep -q 'concept::list' tests/core_concept.rs || fail "no core test for concept list"
grep -q '"concept", "add"' tests/cli_concept.rs || fail "no CLI test for concept add"
grep -q '"concept", "list"' tests/cli_concept.rs || fail "no CLI test for concept list"
for snap in concept_add_json concept_list_json label_taken_json_envelope; do
    [ -f "tests/snapshots/cli_concept__$snap.snap" ] || fail "missing snapshot $snap"
done
echo "rules-have-tests: ok"
