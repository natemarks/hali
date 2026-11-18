#!/usr/bin/env bash
set -uo pipefail
# Criterion 16 (#33): licenses, Cargo.toml, spec §13.3/§13.4/§16, README Install, AGENTS.md,
# .gitignore and CI. Everything is a read of files in this worktree.
cd "$(dirname "${BASH_SOURCE[0]}")/../.." || exit 1

status=0
fail() {
    echo "MISSING: $*"
    status=1
}

# Section of docs/spec.md from the line matching $1 up to (not including) the line matching $2.
section() {
    awk -v from="$1" -v to="$2" '
        $0 ~ from && !on { on = 1 }
        on && $0 ~ to && NR > start { exit }
        on { if (!start) start = NR; print }
    ' docs/spec.md
}

# --- licenses
for f in LICENSE-MIT LICENSE-APACHE; do
    [ -s "$f" ] || fail "$f does not exist or is empty"
    git ls-files --error-unmatch "$f" >/dev/null 2>&1 || fail "$f is not tracked by git"
done
[ ! -e LICENSE ] || fail "there must be no single LICENSE file (the license is MIT OR Apache-2.0 in two files)"
if [ -s LICENSE-MIT ]; then
    grep -qx 'Copyright (c) 2026 Nate Marks' LICENSE-MIT || fail "LICENSE-MIT lacks the line 'Copyright (c) 2026 Nate Marks'"
    grep -q 'Permission is hereby granted, free of charge' LICENSE-MIT || fail "LICENSE-MIT is not the MIT license text"
fi
if [ -s LICENSE-APACHE ]; then
    sum=$(sha256sum LICENSE-APACHE | cut -d' ' -f1)
    [ "$sum" = a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2 ] ||
        fail "LICENSE-APACHE is not the unmodified Apache-2.0 text shared by the registry crates (sha256 $sum, want a60eea81...f759f2)"
fi
grep -qx 'license = "MIT OR Apache-2.0"' Cargo.toml || fail 'Cargo.toml lacks license = "MIT OR Apache-2.0"'

# --- spec
s133=$(section '^### 13\.3 ' '^### 13\.4 ')
s134=$(section '^### 13\.4 ' '^### 13\.5 ')
s16=$(section '^## 16\. ' '^## 17\. ')
[ -n "$s133" ] || fail "spec §13.3 not found"
for want in 'cargo-edit' '0.13.14' 'Benchmark results' '1.0.0' 'resum' 'jq'; do
    printf '%s\n' "$s133" | grep -qi -- "$want" || fail "spec §13.3 does not mention '$want'"
done
publish_block=$(printf '%s\n' "$s133" | awk '/release-publish/ { on = 1 } /Release notes/ { on = 0 } on')
dist_at=$(printf '%s' "$publish_block" | grep -boi 'dist' | head -n 1 | cut -d: -f1)
tag_at=$(printf '%s' "$publish_block" | grep -boiE 'annotated|tag -a' | head -n 1 | cut -d: -f1)
if [ -z "$dist_at" ] || [ -z "$tag_at" ] || [ "$dist_at" -gt "$tag_at" ]; then
    fail "spec §13.3: the release-publish steps must run dist before the annotated tag is created"
fi
for want in 'LICENSE-MIT' 'LICENSE-APACHE' 'SHA256SUMS' 'reproducible'; do
    printf '%s\n' "$s134" | grep -qi -- "$want" || fail "spec §13.4 does not mention '$want'"
done
if printf '%s\n' "$s134" | grep -qE 'README, LICENSE,|, LICENSE, '; then
    fail "spec §13.4 still lists a single LICENSE file"
fi
printf '%s\n' "$s16" | grep -q 'MIT OR Apache-2.0' || fail "spec §16 does not record the license MIT OR Apache-2.0"
if printf '%s\n' "$s16" | grep -qi 'not chosen yet'; then
    fail "spec §16 still says the license is not chosen yet"
fi

# --- README
grep -qx '## Install' README.md || fail 'README.md has no "## Install" heading'
for want in "gh release download -R natemarks/hali" "sha256sum -c --ignore-missing SHA256SUMS" './hali-*/install.sh'; do
    grep -qF -- "$want" README.md || fail "README.md does not contain the §13.5 command fragment: $want"
done

# --- AGENTS.md
if grep -q 'arrive with their tickets' AGENTS.md; then
    fail 'AGENTS.md still says the release targets "arrive with their tickets"'
fi
for t in 'make dist' 'make release-prepare' 'make release-publish'; do
    grep -qF "\`$t" AGENTS.md || fail "AGENTS.md command table does not list \`$t\`"
done

# --- .gitignore
grep -qx '/dist' .gitignore || fail ".gitignore does not ignore /dist"

# --- CI: cargo-edit pinned in the test-external job
job=$(awk '/^  test-external:/ { on = 1; next } on && /^  [A-Za-z0-9_-]+:/ { on = 0 } on' .github/workflows/static-check.yml)
if ! printf '%s\n' "$job" | grep -Eq 'cargo install cargo-edit .*--locked.* --version 0\.13\.14|cargo install cargo-edit .*--version 0\.13\.14.*--locked'; then
    fail "the test-external job in .github/workflows/static-check.yml does not run: cargo install cargo-edit --locked --version 0.13.14"
fi
exit "$status"
