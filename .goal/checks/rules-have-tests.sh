#!/usr/bin/env bash
# Goal #7, criterion 17 (spec §14.1): every new error rule is implemented and triggered by a
# CLI test; the new command has a core test, a CLI test and golden snapshots.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

fail() {
    echo "rules-have-tests: $*" >&2
    exit 1
}

required_rules="SHACL_VIOLATION SHAPES_INVALID"

# 1. The new rules are implemented...
for rule in $required_rules; do
    grep -rq "\"$rule\"" src || fail "rule $rule is not implemented in src/"
done

# 2. ...and every rule emitted by src/ is triggered by a CLI test or a CLI golden snapshot.
emitted=$(grep -rhoE '"[A-Z]{2,}(_[A-Z]+)+"' src | tr -d '"' |
    grep -vE '^(HALI_|XDG_|RUST_|NO_COLOR)' | sort -u)
for rule in $emitted $required_rules; do
    grep -q "$rule" tests/cli_*.rs tests/snapshots/*.snap 2>/dev/null || fail "no CLI test triggers rule $rule"
done

# 3. The validate command has a core test, a CLI test and golden snapshots.
grep -q 'validate::validate' tests/core_shacl.rs 2>/dev/null || fail "no core test for validate"
grep -q '"validate"' tests/cli_validate.rs 2>/dev/null || fail "no CLI test for validate"
for snap in cli_validate__validate_json_conforming cli_validate__validate_json_violations \
    cli_validate__validate_json_violations_error cli_shacl__shacl_violation_json_envelope; do
    [ -f "tests/snapshots/$snap.snap" ] || fail "missing snapshot $snap"
done
echo "rules-have-tests: ok"
