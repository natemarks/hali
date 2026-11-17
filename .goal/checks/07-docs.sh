#!/usr/bin/env bash
set -uo pipefail
# #4 row 14: docs name the installed paths, AGENTS.md says what `make install` does, and
# `make help` lists install.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

fail=0
# shellcheck disable=SC2088 # the spec writes literal "~/" paths
for path in \
    '~/.local/bin/hali' \
    '~/.local/share/man/man1/hali.1' \
    '~/.local/share/bash-completion/completions/hali' \
    '~/.local/share/zsh/site-functions/_hali' \
    '~/.config/fish/completions/hali.fish'; do
    if ! grep -qF -- "$path" docs/spec.md; then
        echo "MISSING: docs/spec.md does not name $path"
        fail=1
    fi
done
# §4 (locations) and §13.2 (make targets) each carry the zsh and fish paths.
sec4=$(awk '/^## 4\. /{f=1;next} /^## 5\. /{f=0} f' docs/spec.md)
sec132=$(awk '/^### 13\.2 /{f=1;next} /^### 13\.3 /{f=0} f' docs/spec.md)
for part in "$sec4" "$sec132"; do
    for path in '_hali' 'hali.fish'; do
        if ! grep -qF -- "$path" <<<"$part"; then
            echo "MISSING: spec §4 and §13.2 must each name $path"
            fail=1
        fi
    done
done
if grep -E 'arrive with their tickets' AGENTS.md | grep -q 'make install'; then
    echo "FOUND: AGENTS.md still lists make install among the targets that arrive with tickets"
    fail=1
fi
# shellcheck disable=SC2016 # the backticks are literal
if ! grep -E '^\| `make install`' AGENTS.md | grep -qi 'release'; then
    echo "MISSING: the AGENTS.md \`make install\` row does not say it installs the release build"
    fail=1
fi
if ! make -s help 2>/dev/null | grep -qE '^install:'; then
    echo "MISSING: make help does not list install"
    fail=1
fi
exit "$fail"
