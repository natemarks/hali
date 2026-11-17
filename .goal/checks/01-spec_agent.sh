#!/usr/bin/env bash
# The patterns hold literal backticks, angle brackets and quotes, which must not expand.
# shellcheck disable=SC2016
set -uo pipefail
# Criterion 11 (#13): docs/spec.md section 9.6 describes agent identity (precedence, the
# empty/whitespace env, the name rules, minting and reuse, the scope: only records hali creates)
# and `hali --help` lists `--agent`.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
spec=docs/spec.md

if [ ! -f "$spec" ]; then
    echo "MISSING: $spec"
    exit 1
fi

sec=$(awk '/^### 9\.6 /{on=1; next} /^##+ /{on=0} on' "$spec")
if [ -z "$sec" ]; then
    echo "MISSING: spec section 9.6 (Agent identity)"
    exit 1
fi

need() { # text
    if ! printf '%s\n' "$sec" | grep -qF -- "$1"; then
        echo "MISSING: spec 9.6 does not mention: $1"
        status=1
    fi
}

need 'HALI_AGENT'
need 'prov:SoftwareAgent'
need 'prov:actedOnBehalfOf'
need 'foaf:name'
need 'whitespace'
need 'control character'
need 'UTF-8'
need 'usage error'
need 'trimmed'
need 'case-sensitive'
need 'schema'
need 'dry-run'
need '--agent ""'

if ! printf '%s\n' "$sec" | grep -qiE 'only.*(add|scan)|(add|scan).*only'; then
    echo "MISSING: spec 9.6 does not say attribution is written only for records hali creates (add, scan)"
    status=1
fi
if ! printf '%s\n' "$sec" | grep -qiE 'never recorded|not recorded|never an Agent|no SoftwareAgent'; then
    echo "MISSING: spec 9.6 does not say hali itself is never recorded as an Agent"
    status=1
fi

help=$(cargo run -q -- --help 2>&1) || {
    echo "MISSING: could not run 'hali --help'"
    exit 1
}
if ! printf '%s\n' "$help" | grep -qF -- '--agent'; then
    echo "MISSING: 'hali --help' does not list --agent"
    status=1
fi
if ! printf '%s\n' "$help" | grep -qF -- 'Attribute new catalog records to this AI agent (overrides HALI_AGENT)'; then
    echo "MISSING: --agent help text is not 'Attribute new catalog records to this AI agent (overrides HALI_AGENT)'"
    status=1
fi

exit "$status"
