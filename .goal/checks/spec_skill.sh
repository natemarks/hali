#!/usr/bin/env bash
# shellcheck disable=SC2016  # the backticks and quotes in the phrases are literal markdown
set -euo pipefail

# Row 19 (#28): docs/spec.md says what #28 builds: the **Skill:** paragraph of 9.1, describe's
# `skill` key, and the SKILL_DIR_UNUSABLE rule of 9.4; "arrives with #28" is gone.
cd "$(dirname "${BASH_SOURCE[0]}")/../.."

spec=docs/spec.md
missing=0

# section <heading prefix>: the text of one ### section, up to the next heading.
section() {
    awk -v h="$1" 'index($0, h) == 1 { on = 1; next } on && /^##/ { exit } on { print }' "$spec"
}

need() {
    local heading="$1" phrase="$2"
    if ! section "$heading" | grep -Fq -- "$phrase"; then
        echo "spec_skill: '$heading' lacks: $phrase" >&2
        missing=1
    fi
}

need '### 9.1 ' '**Skill:**'
need '### 9.1 ' '"skill":{"installed"'
need '### 9.4 ' 'SKILL_DIR_UNUSABLE'

if grep -Fq 'arrives with #28' "$spec"; then
    echo 'spec_skill: docs/spec.md still says "arrives with #28"' >&2
    missing=1
fi

if [ "$missing" -ne 0 ]; then
    exit 1
fi
echo "spec_skill: ok"
